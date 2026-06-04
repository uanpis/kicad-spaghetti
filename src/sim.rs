use crate::physics::*;
use crate::tree::{Node, QTreeItem, QuadTree};
use crate::typed_idx::Idx;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use glam::Vec2;
use kicad_ipc_rs::{KiCadClientBlocking, KiCadError, model::board::PcbItem};
use std::collections::HashMap;
use std::thread::{self /*, yield_now*/};
//use thread_priority::{ThreadPriority, set_current_thread_priority};

pub enum Command {
    Kill,
    Import,
    Pause,
    Resume,
    Reset,
}

const SCALING_FACTOR: f32 = 1e-6f32; // nm -> mm

pub struct SimSettings {
    pub damping: f32,
}

pub struct Sim {
    tx: Sender<Command>,
    rx: Receiver<Snapshot>,
    pub sim_settings: SimSettings,
    pub snapshot: Snapshot,
    handle: Option<thread::JoinHandle<()>>,
}

impl Sim {
    pub fn new() -> Self {
        let (tx, _rx) = unbounded::<Command>();
        let (_tx, rx) = bounded::<Snapshot>(1);
        let handle = thread::spawn(move || {
            //set_current_thread_priority(ThreadPriority::Min).unwrap();
            sim_loop(_rx, _tx);
        });
        let snapshot = Snapshot::new();

        Self {
            tx,
            rx,
            snapshot,
            handle: Some(handle),
            sim_settings: SimSettings { damping: 1.0 },
        }
    }

    pub fn get_snapshot(&mut self) -> &mut Snapshot {
        if let Ok(s) = self.rx.try_recv() {
            self.snapshot = s;
        }
        self.snapshot.get()
    }

    pub fn cmd(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    pub fn kill(&mut self) {
        self.cmd(Command::Kill);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }

    pub fn import(&self) {
        self.cmd(Command::Import);
    }

    pub fn pause(&self) {
        self.cmd(Command::Pause);
    }

    pub fn resume(&self) {
        self.cmd(Command::Resume);
    }

    pub fn reset(&self) {
        self.cmd(Command::Reset);
    }
}

pub struct Snapshot {
    pub iterations: u64,
    pub center: Vec2,
    pub radius: f32,
    pub points: Vec<Point>,
    pub edges: Vec<Edge>,
    pub tree: Option<QuadTree<Point, PointNodeData>>, // only copy if debug
    pub new: bool,
}

impl Snapshot {
    pub fn new() -> Self {
        let points = Vec::<Point>::new();
        let edges = Vec::<Edge>::new();
        Self {
            iterations: 0,
            center: Vec2::ZERO,
            radius: 0.0,
            points,
            edges,
            tree: None,
            new: true,
        }
    }

    pub fn get(&mut self) -> &mut Self {
        self
    }
}

struct Data {
    debug: bool,
    iterations: u64,
    kicad_client: KiCadClientBlocking,
    net_map: HashMap<String, usize>,
    points: Vec<Point>,
    edges: Vec<Edge>,
    tree: Option<QuadTree<Point, PointNodeData>>,
    net_trees: Vec<QuadTree<Point, PointNodeData>>,
}

impl Data {
    fn new(debug: bool) -> Self {
        // TODO handle no kicad
        let kicad_client = KiCadClientBlocking::connect().expect("Could not connect to KiCad");
        let net_map = HashMap::new();
        let points = Vec::<Point>::new();
        let edges = Vec::<Edge>::new();
        let tree = None;
        let net_trees = Vec::<QuadTree<Point, PointNodeData>>::new();
        Self {
            debug,
            iterations: 0,
            kicad_client,
            net_map,
            points,
            edges,
            tree,
            net_trees,
        }
    }

    fn rebuild_tree(&mut self) {
        if let Some(tree) = self.tree.as_mut() {
            tree.clear();
            for i in 0..self.points.len() {
                tree.insert_item(None, &mut self.points, i);
            }
            tree.update_bottom_up(&self.points);
        }
    }

    fn rebuild_net_trees(&mut self) {
        for net in 0..self.net_map.len() {
            self.net_trees[net].clear();
        }
        for i in 0..self.points.len() {
            let net = self.points[i].net;
            self.net_trees[net].insert_item(None, &mut self.points, i);
        }
        for net in 0..self.net_map.len() {
            self.net_trees[net].update_bottom_up(&self.points);
        }
    }

    fn add_point(&mut self, point: Point) -> usize {
        if let Some(i) = self
            .tree
            .as_ref()
            .unwrap()
            .find_item(&self.points, point.get_pos())
        {
            i.as_usize()
        } else {
            self.points.push(point);
            let i = self.points.len() - 1;
            self.tree
                .as_mut()
                .unwrap()
                .insert_item(None, &mut self.points, i);
            i
        }
    }

    fn sort_points(&mut self) {
        fn f32_to_ordered_u32(f: f32) -> u32 {
            let bits = f.to_bits();
            if bits >> 31 == 0 {
                bits | 0x8000_0000
            } else {
                !bits
            }
        }
        fn morton(pos: Vec2) -> u64 {
            let xi = f32_to_ordered_u32(pos.x);
            let yi = f32_to_ordered_u32(pos.y);
            spread(xi) | (spread(yi) << 1)
        }
        fn spread(x: u32) -> u64 {
            let mut x = x as u64;
            x = (x | (x << 16)) & 0x0000_FFFF_0000_FFFF;
            x = (x | (x << 8)) & 0x00FF_00FF_00FF_00FF;
            x = (x | (x << 4)) & 0x0F0F_0F0F_0F0F_0F0F;
            x = (x | (x << 2)) & 0x3333_3333_3333_3333;
            x = (x | (x << 1)) & 0x5555_5555_5555_5555;
            x
        }

        let mut indices: Vec<usize> = (0..self.points.len()).collect();
        indices.sort_unstable_by_key(|&i| morton(self.points[i].pos));

        let mut inverse = vec![0usize; indices.len()];
        for (new, &old) in indices.iter().enumerate() {
            inverse[old] = new;
        }

        for i in 0..self.points.len() {
            if indices[i] == usize::MAX || indices[i] == i {
                indices[i] = usize::MAX;
                continue;
            }
            let mut current = i;
            loop {
                let target = indices[current];
                indices[current] = usize::MAX;
                if target == i || target == usize::MAX {
                    break;
                }
                self.points.swap(current, target);
                current = target;
            }
        }

        for edge in &mut self.edges {
            edge.i0 = inverse[edge.i0];
            edge.i1 = inverse[edge.i1];
        }
    }

    fn import(&mut self) -> Result<(), KiCadError> {
        let items = self.kicad_client.get_items_by_type_codes(vec![11])?;
        //println!("{:#?}", tracks);

        let tracks: Vec<_> = items
            .iter()
            .filter_map(|item| {
                if let PcbItem::Track(track) = item {
                    Some(track)
                } else {
                    None
                }
            })
            .collect();

        macro_rules! conv_nm {
            ($x:expr) => {
                SCALING_FACTOR * $x as f32
            };
        }
        macro_rules! conv_vec {
            ($v:expr) => {
                vec2!(conv_nm!($v.unwrap().x_nm), conv_nm!($v.unwrap().y_nm))
            };
        }
        macro_rules! conv_point {
            ($v:expr, $r:expr, $n:expr) => {
                point!(
                    conv_nm!($v.unwrap().x_nm),
                    conv_nm!($v.unwrap().y_nm),
                    $r,
                    $n
                )
            };
        }

        let pts: Vec<_> = tracks
            .iter()
            .flat_map(|t| [conv_vec!(t.start_nm), conv_vec!(t.end_nm)])
            .collect();

        // Get bounds
        let (min, max) = pts
            .iter()
            .fold(None, |acc: Option<(Vec2, Vec2)>, &p| {
                Some(match acc {
                    None => (p, p),
                    Some((_min, _max)) => (_min.min(p), _max.max(p)),
                })
            })
            .unwrap();

        // Set tree root position and size
        let root_pos = 0.5 * (max + min);
        let dif = max - min;
        let d = if dif.x > dif.y { dif.x } else { dif.y };
        let root_rad = 1.0 * d;

        self.tree = Some(QuadTree::<Point, PointNodeData>::new(root_pos, root_rad));

        /*
        for net in self.kicad_client.get_nets().unwrap() {
            println!("found net {} \"{}\"", net.code, net.name);
        }
        for layer in self.kicad_client.get_board_enabled_layers().unwrap().layers {
            println!("found layer {} \"{}\"", layer.id, layer.name);
        }
        */
        let firstlayer = self.kicad_client.get_board_enabled_layers().unwrap().layers[0].id;

        for track in tracks {
            let layer = track.layer.id;
            // only first layer for now TODO add multilayer support
            if layer != firstlayer {
                continue;
            }
            // net
            let netname = &track.net.as_ref().unwrap().name;
            let net;
            match self.net_map.get(netname) {
                Some(n) => {
                    net = *n;
                }
                None => {
                    net = self.net_map.len();
                    self.net_map.insert(netname.clone(), net);
                }
            }

            let w = conv_nm!(track.width_nm.unwrap());
            let p0 = conv_point!(track.start_nm, 0.5 * w, net);
            let p1 = conv_point!(track.end_nm, 0.5 * w, net);

            let l0 = (p1.pos - p0.pos).length();
            let i0 = self.add_point(p0);
            let i1 = self.add_point(p1);
            self.edges.push(edge!(i0, i1, w, l0));
        }

        for _ in 0..self.net_map.len() {
            self.net_trees
                .push(QuadTree::<Point, PointNodeData>::new(root_pos, root_rad));
        }

        self.resample();
        self.sort_points();
        self.compute_neighbors();
        self.rebuild_tree();
        self.rebuild_net_trees();

        Ok(())
    }

    fn send(&self, tx: &Sender<Snapshot>) {
        let tree = if self.debug { self.tree.clone() } else { None };
        let (center, radius) = if let Some(x) = tree.as_ref() {
            (x.get_pos(), x.get_rad())
        } else {
            (vec2!(0.0, 0.0), 1.0)
        };
        let snapshot = Snapshot {
            iterations: self.iterations,
            center,
            radius,
            points: self.points.clone(),
            edges: self.edges.clone(),
            tree,
            new: true,
        };
        let _ = tx.send(snapshot);
    }

    fn resample(&mut self) {
        for i in 0..self.edges.len() {
            let edge = &self.edges[i];
            let w = edge.w;
            let p0 = self.points[edge.i0].pos;
            let p1 = self.points[edge.i1].pos;
            let net = self.points[edge.i0].net;
            let len = (p1 - p0).length();
            let n = (len / w).round() as usize;
            let l0 = len / n as f32;
            if n > 1 {
                let delta = (p1 - p0) / n as f32;
                let mut end = point!(p0 + delta, 0.5 * w, net);
                let mut iend = self.add_point(end);

                self.edges[i].i1 = iend;
                self.edges[i].l0 = l0;

                let mut istart;
                for j in 2..n + 1 {
                    istart = iend;

                    end = point!(p0 + delta * j as f32, 0.5 * w, net);
                    iend = self.add_point(end);
                    self.edges.push(edge!(istart, iend, w, l0));
                }
            }
        }
    }

    fn compute_neighbors(&mut self) {
        for edge in &self.edges[..] {
            self.points[edge.i0].neighbors += 1;
            self.points[edge.i1].neighbors += 1;
        }
    }

    fn calculate_force(
        &mut self,
        stack: &mut Vec<(Idx<Node<Point, PointNodeData>>, f32)>,
        index: usize,
    ) -> Vec2 {
        let mut calc = |tree: &mut QuadTree<Point, PointNodeData>, i: usize| -> Vec2 {
            let mut force = Vec2::ZERO;
            stack.clear();
            stack.push((tree.root, tree.rad));
            while let Some((node, rad)) = stack.pop() {
                let pos = self.points[i].pos;
                let offset = pos - tree.nodes[node].data.all.pos;
                let distsq = offset.length_squared();
                let metric = 0.5;
                if rad * rad / distsq < metric * metric {
                    let distsq = distsq.max(0.1);
                    force += tree.nodes[node].data.all.radius * offset / (distsq * distsq);
                } else {
                    if tree.nodes[node].is_leaf {
                        for j in tree.nodes[node].items[0..tree.nodes[node].nitems].iter() {
                            let offset = pos - self.points[*j].pos;
                            let distsq = offset.length_squared().max(0.1);
                            force += self.points[*j].radius * offset / (distsq * distsq);
                        }
                    } else {
                        for child in tree.nodes[node]
                            .children
                            .iter()
                            .filter(|x| x.as_usize() != 0usize)
                        {
                            stack.push((*child, rad * 0.5));
                        }
                    }
                }
            }
            force
        };
        let all = calc(self.tree.as_mut().unwrap(), index);
        let net = calc(&mut self.net_trees[self.points[index].net], index);
        all - net
    }
}

fn sim_loop(rx: Receiver<Command>, tx: Sender<Snapshot>) {
    //TODO toggle debug
    let mut data = Data::new(true);
    let mut running = true;
    let mut paused = false;
    let delta = 0.05;

    let mut stack = Vec::<(Idx<Node<Point, PointNodeData>>, f32)>::new();

    while running {
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Command::Kill => {
                    running = false;
                }
                Command::Import => {
                    data.import().expect("Could not import PCB from KiCad");
                    data.send(&tx);
                }
                Command::Pause => {
                    paused = true;
                }
                Command::Resume => {
                    paused = false;
                }
                Command::Reset => {
                    data = Data::new(true);
                    data.import().expect("Could not import PCB from KiCad");
                    data.send(&tx);
                }
            }
        }
        if !paused {
            data.iterations += 1;

            for i in 0..data.points.len() {
                let force = data.calculate_force(&mut stack, i);
                let mass = data.points[i].radius;
                data.points[i].apply_force(mass * force);
            }

            for edge in &data.edges {
                edge.apply_tension(&mut data.points, 1.0);
            }

            for i in 0..data.points.len() {
                let point = &mut data.points[i];
                point.v *= 0.998;
                point.step(delta);
            }

            if data.iterations.is_multiple_of(64) {
                data.sort_points();
            }
            data.rebuild_tree();
            data.rebuild_net_trees();
        } else {
            thread::sleep(std::time::Duration::from_millis(16));
        }
        if tx.is_empty() {
            data.send(&tx);
        }
    }
}
