use crate::physics::*;
use crate::tree::{Node, QTreeItem, QuadTree};
use crate::typed_idx::Idx;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use glam::Vec2;
use kicad_ipc_rs::{KiCadClientBlocking, KiCadError, model::board::PcbItem};
use std::collections::{HashMap, HashSet};
use std::thread::{self /*, yield_now*/};
//use thread_priority::{ThreadPriority, set_current_thread_priority};

pub enum Command {
    Kill,
    Import,
    Pause,
    Resume,
    Reset,
    UpdateSettings(SimSettings),
}

const SCALING_FACTOR: f32 = 1e-6f32; // nm -> mm
const METRIC: f32 = 0.5;
const MIN_DIST: f32 = 0.05;

#[derive(Clone)]
pub struct SimSettings {
    pub damping: f32,
    pub noodliness: f32,
    pub collision_elasticity: f32,
    pub limit_step: bool,
    pub self_collision: bool,
}

impl SimSettings {
    fn new() -> Self {
        Self {
            damping: 1.0,
            noodliness: 0.5,
            collision_elasticity: 0.5,
            limit_step: true,
            self_collision: false,
        }
    }
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
            sim_settings: SimSettings::new(),
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

    pub fn update_settings(&self) {
        self.cmd(Command::UpdateSettings(self.sim_settings.clone()));
    }
}

pub struct Snapshot {
    pub iterations: u64,
    pub center: Vec2,
    pub radius: f32,
    pub points: Vec<Point>,
    pub curves: Vec<Vec<Edge>>,
    pub tree: Option<QuadTree<Point, PointNodeData>>, // only copy if debug
    pub new: bool,
}

impl Snapshot {
    pub fn new() -> Self {
        let points = Vec::<Point>::new();
        let curves = Vec::<Vec<Edge>>::new();
        Self {
            iterations: 0,
            center: Vec2::ZERO,
            radius: 0.0,
            points,
            curves,
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
    curves: Vec<Vec<Edge>>,
    tree: Option<QuadTree<Point, PointNodeData>>,
    net_trees: Vec<QuadTree<Point, PointNodeData>>,

    min_rad: f32,
}

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

impl Data {
    fn new(debug: bool) -> Self {
        // TODO handle no kicad
        let kicad_client = KiCadClientBlocking::connect().expect("Could not connect to KiCad");
        let net_map = HashMap::new();
        let points = Vec::<Point>::new();
        let curves = Vec::<Vec<Edge>>::new();
        let tree = None;
        let net_trees = Vec::<QuadTree<Point, PointNodeData>>::new();
        Self {
            debug,
            iterations: 0,
            kicad_client,
            net_map,
            points,
            curves,
            tree,
            net_trees,
            min_rad: f32::INFINITY,
        }
    }

    fn points_to_back(&mut self) {
        // TODO parallel
        self.points.iter_mut().for_each(|p| p.store_prev());
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
            self.points[i].rad = self.points[i].rad.max(point.rad);
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

        for edge in self.curves.iter_mut().flatten() {
            edge.i0 = inverse[edge.i0];
            edge.i1 = inverse[edge.i1];
        }
    }

    /*
    fn sort_edges(&mut self) {
        self.edges
            .sort_unstable_by_key(|x| morton(self.points[x.i0].pos + self.points[x.i1].pos));
    }
    */

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

        // build unsorted flat vector of edges
        let mut edges_flat = Vec::<Edge>::new();
        let mut i0_map = HashMap::<usize, HashSet<usize>>::new();
        let mut i1_map = HashMap::<usize, HashSet<usize>>::new();
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
            let point0 = conv_point!(track.start_nm, 0.5 * w, net);
            let point1 = conv_point!(track.end_nm, 0.5 * w, net);
            let l0 = (point1.pos - point0.pos).length();

            let i0 = self.add_point(point0);
            let i1 = self.add_point(point1);
            if let Some(x) = i0_map.get_mut(&i0) {
                x.insert(edges_flat.len());
            } else {
                let mut hash_set = HashSet::new();
                hash_set.insert(edges_flat.len());
                i0_map.insert(i0, hash_set);
            }
            if let Some(x) = i1_map.get_mut(&i1) {
                x.insert(edges_flat.len());
            } else {
                let mut hash_set = HashSet::new();
                hash_set.insert(edges_flat.len());
                i1_map.insert(i1, hash_set);
            }

            edges_flat.push(edge!(i0, i1, w, l0, usize::MAX));
            self.min_rad = self.min_rad.min(0.5 * w);
        }

        // build curves:
        // 1. walk backward until no more edges are found
        // 2. walk forwards adding edges to curve, until no more edges are found
        // 3. switch to next curve and repeat until all edges are exhausted.
        let mut iterations = 0;
        let mut tmp_edges = Vec::<Edge>::new();
        let mut to_add: HashSet<usize> = (0..edges_flat.len()).collect();
        let mut edge = edges_flat[to_add.take(&0).unwrap()].clone();
        // remove first edge from pool
        i0_map.remove(&edge.i0);
        i1_map.remove(&edge.i1);
        #[cfg(debug_assertions)]
        println!("{:#?}", edges_flat);
        self.curves.push(Vec::<Edge>::new());
        while !to_add.is_empty() || !tmp_edges.is_empty() {
            //println!("{:#?}", self.curves);
            #[cfg(debug_assertions)]
            {
                println!();
                println!("current edge: {:?}", (edge.i0, edge.i1));
                println!(
                    "tmp_edges: {:?}",
                    tmp_edges.iter().map(|x| (x.i0, x.i1)).collect::<Vec<_>>()
                );
                println!("edges left to add: {:?}", to_add.iter().collect::<Vec<_>>());
            }

            if let Some(mut prevs) = i0_map.remove(&edge.i0) {
                let prev = *prevs.iter().next().unwrap();
                prevs.remove(&prev);
                if !prevs.is_empty() {
                    i0_map.insert(edge.i0, prevs);
                }
                if let Some(i) = to_add.take(&prev) {
                    // walk backwards, with swap
                    tmp_edges.push(edge);
                    #[cfg(debug_assertions)]
                    println!("edge that matched i0 = i0: {:?}", i);
                    edge = edges_flat[i].clone();
                    edge.swap();
                    continue;
                }
            }
            if let Some(mut prevs) = i1_map.remove(&edge.i0) {
                let prev = *prevs.iter().next().unwrap();
                prevs.remove(&prev);
                if !prevs.is_empty() {
                    i1_map.insert(edge.i0, prevs);
                }
                if let Some(i) = to_add.take(&prev) {
                    // walk backwards
                    tmp_edges.push(edge);
                    #[cfg(debug_assertions)]
                    println!("edge that matched i1 = i0: {:?}", i);
                    edge = edges_flat[i].clone();
                    continue;
                }
            }
            // insert, walk forwards
            let i1 = edge.i1;
            self.curves.last_mut().unwrap().push(edge);

            if let Some(next) = tmp_edges.pop() {
                // if tmp list not empty, consume
                edge = next;
                if to_add.is_empty() && tmp_edges.is_empty() {
                    self.curves.last_mut().unwrap().push(edge);
                    #[cfg(debug_assertions)]
                    println!("break !!!");
                    break;
                }
                continue;
            }
            if let Some(mut nexts) = i0_map.remove(&i1) {
                let next = *nexts.iter().next().unwrap();
                nexts.remove(&next);
                if !nexts.is_empty() {
                    i0_map.insert(i1, nexts);
                }
                if let Some(i) = to_add.take(&next) {
                    // walk forwards
                    #[cfg(debug_assertions)]
                    println!("edge that matched i0 = i1: {:?}", i);
                    edge = edges_flat[i].clone();
                    continue;
                }
            }
            if let Some(mut nexts) = i1_map.remove(&i1) {
                let next = *nexts.iter().next().unwrap();
                nexts.remove(&next);
                if !nexts.is_empty() {
                    i1_map.insert(i1, nexts);
                }
                if let Some(i) = to_add.take(&next) {
                    // walk forwards, with swap
                    #[cfg(debug_assertions)]
                    println!("edge that matched i1 = i1: {:?}", i);
                    edge = edges_flat[i].clone();
                    edge.swap();
                    continue;
                }
            }
            // no connected edge found: switch to next curve
            self.curves.push(Vec::<Edge>::new());
            let n = *to_add.iter().next().unwrap();
            edge = edges_flat[to_add.take(&n).unwrap()].clone();
            if to_add.is_empty() && tmp_edges.is_empty() {
                self.curves.last_mut().unwrap().push(edge);
                #[cfg(debug_assertions)]
                println!("break !!!");
                break;
            }
            i0_map.remove(&n);
            i1_map.remove(&n);

            #[cfg(debug_assertions)]
            {
                iterations += 1;
                if iterations > 1000000 {
                    println!("{:#?}", edges_flat);
                    panic!("stuck in curve insertion!");
                }
            }
        }
        #[cfg(debug_assertions)]
        {
            println!("hello!!!!!!!!!!");
            println!("{:#?}", self.curves);
        }

        for _ in 0..self.net_map.len() {
            self.net_trees
                .push(QuadTree::<Point, PointNodeData>::new(root_pos, root_rad));
        }
        self.points_to_back();

        self.resample(&mut Vec::<Point>::new(), &mut Vec::<Vec<Edge>>::new());
        self.resample(&mut Vec::<Point>::new(), &mut Vec::<Vec<Edge>>::new());
        self.resample(&mut Vec::<Point>::new(), &mut Vec::<Vec<Edge>>::new());
        self.sort_points();
        //self.sort_edges();
        self.compute_neighbors();
        self.rebuild_tree();
        self.rebuild_net_trees();
        self.points_to_back();

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
            curves: self.curves.clone(),
            tree,
            new: true,
        };
        let _ = tx.send(snapshot);
    }

    fn resample(&mut self, points_buf: &mut Vec<Point>, curves_buf: &mut Vec<Vec<Edge>>) {
        const ASPECT: f32 = 2.0;

        // move points and edges to back buffer, write resampled to front
        std::mem::swap(&mut self.points, points_buf);
        std::mem::swap(&mut self.curves, curves_buf);
        self.points.clear();
        self.curves.clear();
        self.tree.as_mut().unwrap().clear();

        let mut i0;
        let mut i1;
        for curve_read in curves_buf.iter() {
            let mut curve_write = Vec::<Edge>::new();
            if !curve_read.is_empty() {
                // insert first point
                i0 = if let Some((i, _)) = self
                    .points
                    .iter()
                    .enumerate()
                    .find(|(_, x)| x.pos == points_buf[curve_read[0].i0].pos)
                {
                    i
                } else {
                    self.points.push(points_buf[curve_read[0].i0].clone());
                    self.points.len() - 1
                };
            } else {
                continue;
            }
            let mut l0 = 0.0;
            let mut unsubdivide = false;
            for i in 0..curve_read.len() {
                let edge = &curve_read[i];
                let next_w = if i + 1 < curve_read.len() {
                    curve_read[i + 1].w
                } else {
                    -1.0
                };

                let w = edge.w;
                if !unsubdivide {
                    l0 = 0.0;
                }
                l0 += edge.l0;
                let len = edge.length(points_buf);
                let segment_size = w * ASPECT;

                if len < 0.5 * segment_size
                    && !unsubdivide
                    && points_buf[edge.i1].neighbors == 2
                    && edge.w == next_w
                {
                    // unsubdivide
                    unsubdivide = true;
                    continue;
                }

                if len > 1.5 * segment_size && !unsubdivide {
                    // subdivide
                    let p0 = points_buf[edge.i0].pos;
                    let p1 = points_buf[edge.i1].pos;
                    let net = points_buf[edge.i0].net;
                    l0 *= 0.5;
                    let point = point!(0.5 * (p1 + p0), w, net);
                    i1 = self.points.len();
                    self.points.push(point);
                    curve_write.push(edge!(i0, i1, w, l0, usize::MAX));
                    i0 = i1;
                }

                unsubdivide = false;
                let point = points_buf[edge.i1].clone();
                i1 = if i == curve_read.len() - 1
                    && let Some((i, _)) = self
                        .points
                        .iter()
                        .enumerate()
                        .find(|(_, x)| x.pos == points_buf[curve_read[0].i0].pos)
                {
                    i
                } else {
                    self.points.push(point);
                    self.points.len() - 1
                };
                curve_write.push(edge!(i0, i1, w, l0, usize::MAX));
                i0 = i1;
            }
            self.curves.push(curve_write);
        }
        /*
        let n = (ASPECT * len / w).round() as usize;
        let l0 = len / n as f32;

        if n > 1 {
            let delta = (p1 - p0) / n as f32;
            let mut end = point!(p0 + delta, 0.5 * w, net);
            end.neighbors = 2;
            let mut iend = self.add_point(end);

            self.edges[i].i1 = iend;
            self.edges[i].l0 = l0;

            let mut istart;
            let mut prev = self.edges[i].prev;
            for j in 2..n + 1 {
                istart = iend;

                end = point!(p0 + delta * j as f32, 0.5 * w, net);
                end.neighbors = 2;
                iend = self.add_point(end);
                self.edges.push(edge!(istart, iend, w, l0, prev));
                prev = self.edges.len() - 1;
            }
        }
        */
    }

    fn compute_neighbors(&mut self) {
        for edge in self.curves.iter().flatten() {
            self.points[edge.i0].neighbors += 1;
            self.points[edge.i1].neighbors += 1;
        }
    }

    fn compute_force(
        &self,
        stack: &mut Vec<(Idx<Node<PointNodeData>>, f32)>,
        index: usize,
    ) -> Vec2 {
        let pos = self.points[index].pos;
        let net = self.points[index].net;

        let mut calc = |tree: &QuadTree<Point, PointNodeData>| -> Vec2 {
            let mut force = Vec2::ZERO;
            stack.clear();
            stack.push((tree.root, tree.rad));
            while let Some((node, rad)) = stack.pop() {
                let delta = pos - tree.nodes[node].data.pos;
                let distsq = delta.length_squared();
                if distsq == 0.0 {
                    continue;
                };

                if rad * rad / distsq < METRIC * METRIC {
                    let distsq = distsq.max(MIN_DIST);
                    force += tree.nodes[node].data.rad * delta / (distsq * distsq);
                } else {
                    if tree.nodes[node].is_leaf {
                        let offset = tree.nodes[node].items;
                        let nitems = tree.nodes[node].nitems;
                        for j in tree.leaf_items[offset..offset + nitems].iter() {
                            let delta = pos - self.points[*j].pos;
                            let distsq = delta.length_squared();
                            if distsq == 0.0 {
                                continue;
                            };
                            let distsq = distsq.max(MIN_DIST);

                            force += self.points[*j].rad * delta / (distsq * distsq);
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
        let mass = self.points[index].rad;
        mass * (calc(self.tree.as_ref().unwrap()) - calc(&self.net_trees[net]))
    }

    fn collide_edge(
        &mut self,
        stack: &mut Vec<Idx<Node<PointNodeData>>>,
        curve_index: usize,
        edge_index: usize,
        elasticity: f32,
    ) {
        let i0 = self.curves[curve_index][edge_index].i0;
        let i1 = self.curves[curve_index][edge_index].i1;
        let net = self.points[self.curves[curve_index][edge_index].i0].net;
        let mut p0 = self.points[i0].pos;
        let mut p1 = self.points[i1].pos;
        let r = 0.5 * self.curves[curve_index][edge_index].w;
        let mut aabb = self.curves[curve_index][edge_index].get_aabb(&self.points);

        self.curves[curve_index][edge_index].mark = false;

        let tree = self.tree.as_ref().unwrap();
        stack.clear();
        stack.push(tree.root);
        while let Some(node) = stack.pop() {
            if !aabb.collide_aabb(&tree.nodes[node].data.aabb) {
                continue;
            }
            if tree.nodes[node].is_leaf {
                let offset = tree.nodes[node].items;
                let nitems = tree.nodes[node].nitems;
                for j in tree.leaf_items[offset..offset + nitems].iter() {
                    if j.as_usize() == i0 || j.as_usize() == i1 {
                        continue;
                    }
                    let point_net = self.points[*j].net;
                    let point_pos = self.points[*j].pos;
                    let point_rad = self.points[*j].rad;
                    // cheap check
                    if point_net != net && aabb.collide_square(point_pos, point_rad) {
                        let e = p1 - p0;
                        let d0 = point_pos - p0;

                        let t = (e.dot(d0) / e.length_squared()).clamp(0.0, 1.0);

                        let normal = d0 - e * t;
                        let dist_sq = normal.length_squared();
                        let collision_dist = r + point_rad;
                        // TODO netclass clearance
                        if dist_sq != 0.0 && dist_sq < collision_dist * collision_dist {
                            self.curves[curve_index][edge_index].mark = true;

                            // update points
                            let dist = dist_sq.sqrt();
                            let delta_pos = ((1.0 + elasticity) * normal * (dist - collision_dist)
                                / dist)
                                .clamp_length_max(self.min_rad);

                            let weight = if self.points[*j].neighbors < 2 {
                                1.0
                            } else {
                                self.points[j.as_usize()].pos -= delta_pos * r / collision_dist;
                                point_rad / collision_dist
                            };
                            let diff = delta_pos * weight;
                            if self.points[i0].neighbors > 1 {
                                p0 += diff * (1.0 - t);
                            }
                            if self.points[i1].neighbors > 1 {
                                p1 += diff * t;
                            }
                            aabb = AABB::edge(p0, p1, r);
                            #[cfg(debug_assertions)]
                            {
                                assert!(!self.points[*j].pos.is_nan());
                                assert!(!p0.is_nan());
                                assert!(!p1.is_nan());
                            }
                        }
                    }
                }
            } else {
                for child in tree.nodes[node]
                    .children
                    .iter()
                    .filter(|x| x.as_usize() != 0usize)
                {
                    stack.push(*child);
                }
            }
        }
        self.points[i0].pos = p0;
        self.points[i1].pos = p1;
    }
}

fn sim_loop(rx: Receiver<Command>, tx: Sender<Snapshot>) {
    let mut data = Data::new(true);
    let mut running = true;
    let mut paused = false;
    let delta = 0.05;

    let mut force_stack = Vec::<(Idx<Node<PointNodeData>>, f32)>::new();
    let mut collision_stack = Vec::<Idx<Node<PointNodeData>>>::new();
    let mut sim_settings = SimSettings::new();

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
                Command::UpdateSettings(settings) => {
                    sim_settings = settings;
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
            // TODO parallelize
            if data.iterations.is_multiple_of(64) {
                //data.resample();
                data.sort_points();
                //data.sort_edges();
                data.rebuild_tree();
                data.rebuild_net_trees();
                data.points_to_back();
            }

            // apply forces
            let k = 2.0;
            for i in 0..data.points.len() {
                data.points[i].f =
                    data.compute_force(&mut force_stack, i) * k * sim_settings.noodliness;
            }
            for edge in data.curves.iter().flatten() {
                edge.apply_tension(&mut data.points, k * (1.0 - sim_settings.noodliness));
            }

            // integrate
            if sim_settings.limit_step {
                for i in 0..data.points.len() {
                    data.points[i].step_clamped(delta, data.min_rad);
                }
            } else {
                for i in 0..data.points.len() {
                    data.points[i].step(delta);
                }
            }

            data.rebuild_tree();
            data.rebuild_net_trees();

            // collide
            for _ in 0..4 {
                for i in 0..data.curves.len() {
                    for j in 0..data.curves[i].len() {
                        data.collide_edge(
                            &mut collision_stack,
                            i,
                            j,
                            sim_settings.collision_elasticity,
                        );
                    }
                }
            }

            // calculate velocity
            for i in 0..data.points.len() {
                data.points[i].update_velocity(delta);
                data.points[i].v = data.points[i].v.clamp_length_max(data.min_rad / delta);
                data.points[i].v *= 0.99;
            }

            #[cfg(debug_assertions)]
            for point in data.points.iter() {
                assert!(
                    (point.pos - point.pos_prev).length() <= 10.0 * data.min_rad,
                    "large position difference: {} -> {}",
                    point.pos_prev,
                    point.pos
                );
            }

            // copy current position to back
            data.points_to_back();
        } else {
            thread::sleep(std::time::Duration::from_millis(16));
        }
        if tx.is_empty() {
            data.send(&tx);
        }
    }
}
