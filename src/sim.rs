use crate::physics::*;
use crate::tree::{Node, QTreeItem, QuadTree};
use crate::typed_idx::Idx;
use crate::utils::*;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use glam::Vec2;
use kicad_ipc_rs::{KiCadClientBlocking, KiCadError, model::board::PcbItem};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::f32::consts::*;
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

const PARALLEL_CHUNK_SIZE: usize = 256;
const SCALING_FACTOR: f32 = 1e-6f32; // nm -> mm
const METRIC: f32 = 0.5;
const MIN_DIST: f32 = 0.05;

#[derive(Clone)]
pub struct SimSettings {
    pub damping: f32,
    pub noodliness: f32,

    pub repulsion_degree: u32,
    pub self_repulsion: bool,

    pub collision_elasticity: f32,
    pub collision_iterations: usize,
    pub self_collision: bool,

    pub limit_step: bool,
}

impl SimSettings {
    fn new() -> Self {
        Self {
            damping: 1.0,
            noodliness: 0.5,

            repulsion_degree: 3,
            self_repulsion: true,

            collision_elasticity: 0.5,
            collision_iterations: 3,
            self_collision: false,

            limit_step: true,
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
    pub trees: Vec<QuadTree<Point, PointNodeData>>,
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
            trees: Vec::<QuadTree<Point, PointNodeData>>::new(),
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
    layer_map: HashMap<i32, usize>,

    points: Vec<Point>,
    curves: Vec<Vec<Edge>>,

    trees: Vec<QuadTree<Point, PointNodeData>>,
    net_trees: Vec<Vec<QuadTree<Point, PointNodeData>>>,

    min_rad: f32,
}

impl Data {
    fn new(debug: bool) -> Self {
        // TODO handle no kicad
        let kicad_client = KiCadClientBlocking::connect().expect("Could not connect to KiCad");
        let net_map = HashMap::new();
        let layer_map = HashMap::new();
        let points = Vec::<Point>::new();
        let curves = Vec::<Vec<Edge>>::new();
        let trees = Vec::<QuadTree<Point, PointNodeData>>::new();
        let net_trees = Vec::<Vec<QuadTree<Point, PointNodeData>>>::new();
        Self {
            debug,
            iterations: 0,
            kicad_client,
            net_map,
            layer_map,
            points,
            curves,
            trees,
            net_trees,
            min_rad: f32::INFINITY,
        }
    }

    fn points_to_back(&mut self) {
        // TODO parallel
        self.points.iter_mut().for_each(|p| p.store_prev());
    }

    fn rebuild_trees(&mut self) {
        for layer in 0..self.layer_map.len() {
            self.trees[layer].clear();
        }
        for i in 0..self.points.len() {
            let layer = self.points[i].layer;
            self.trees[layer].insert_item(None, &mut self.points, i);
        }
        for layer in 0..self.layer_map.len() {
            self.trees[layer].update_bottom_up(&self.points);
        }
    }

    fn rebuild_net_trees(&mut self) {
        for layer in 0..self.layer_map.len() {
            for net in 0..self.net_map.len() {
                self.net_trees[layer][net].clear();
            }
        }
        for i in 0..self.points.len() {
            let net = self.points[i].net;
            let layer = self.points[i].layer;
            self.net_trees[layer][net].insert_item(None, &mut self.points, i);
        }
        for layer in 0..self.layer_map.len() {
            for net in 0..self.net_map.len() {
                self.net_trees[layer][net].update_bottom_up(&self.points);
            }
        }
    }

    fn add_point(&mut self, point: Point) -> usize {
        let layer = point.layer;
        if let Some(i) = self.trees[layer].find_item(&self.points, point.get_pos()) {
            self.points[i].rad = self.points[i].rad.max(point.rad);
            i.as_usize()
        } else {
            self.points.push(point);
            let i = self.points.len() - 1;
            self.trees[layer].insert_item(None, &mut self.points, i);
            i
        }
    }

    fn sort_points(&mut self) {
        let mut indices: Vec<usize> = (0..self.points.len()).collect();
        indices.sort_unstable_by_key(|&i| {
            (self.points[i].layer as u128) << 64 | morton(self.points[i].pos) as u128
        });

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

        // add layers to layer_map
        self.kicad_client
            .get_board_stackup()?
            .layers
            .iter()
            .filter(|x| x.layer_type == kicad_ipc_rs::model::board::BoardStackupLayerType::Copper)
            .enumerate()
            .for_each(|(n, stackuplayer)| {
                self.layer_map.insert(stackuplayer.layer.id, n);
            });

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
            ($pos:expr, $rad:expr, $net:expr, $layer:expr) => {
                point!(conv_vec!($pos), $rad, $net, $layer)
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

        for _ in 0..self.layer_map.len() {
            self.trees
                .push(QuadTree::<Point, PointNodeData>::new(root_pos, root_rad));
        }

        // build unsorted flat vector of edges
        let mut edges_flat = Vec::<Edge>::new();
        let mut i0_map = HashMap::<usize, HashSet<usize>>::new();
        let mut i1_map = HashMap::<usize, HashSet<usize>>::new();
        for track in tracks {
            let layer = *self.layer_map.get(&track.layer.id).unwrap();
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
            let point0 = conv_point!(track.start_nm, 0.5 * w, net, layer);
            let point1 = conv_point!(track.end_nm, 0.5 * w, net, layer);
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

            edges_flat.push(edge!(i0, i1, w, l0));
            self.min_rad = self.min_rad.min(0.5 * w);
        }

        // build curves:
        // 1. walk backward until no more edges are found
        // 2. walk forwards adding edges to curve, until no more edges are found
        // 3. switch to next curve and repeat until all edges are exhausted.
        let mut backwards = true;
        let mut tmp_edges = Vec::<Edge>::new();
        //let mut to_add: HashSet<usize> = (0..edges_flat.len()).collect();
        let mut edge = edges_flat.swap_remove(0);
        // remove first edge from pool
        self.curves.push(Vec::<Edge>::new());
        #[cfg(debug_assertions)]
        let mut iterations = 0;
        #[cfg(debug_assertions)]
        println!("{:#?}", edges_flat);
        while !edges_flat.is_empty() || !tmp_edges.is_empty() {
            //println!("{:#?}", self.curves);
            #[cfg(debug_assertions)]
            {
                println!();
                println!("current edge: {:?}", (edge.i0, edge.i1));
                println!(
                    "tmp_edges: {:?}",
                    tmp_edges.iter().map(|x| (x.i0, x.i1)).collect::<Vec<_>>()
                );
                println!(
                    "edges left to add: {:?}",
                    edges_flat.iter().collect::<Vec<_>>()
                );
            }

            if backwards {
                // walk backwards
                let found_edges: Vec<(bool, usize)> = edges_flat
                    .iter()
                    .enumerate()
                    .filter_map(|(i, e)| {
                        if e.i0 == edge.i0 {
                            Some((true, i))
                        } else if e.i1 == edge.i0 {
                            Some((false, i))
                        } else {
                            None
                        }
                    })
                    .collect();

                if !found_edges.is_empty()
                    && let (swap, index) = found_edges[0]
                {
                    tmp_edges.push(edge);
                    edge = edges_flat.swap_remove(index);
                    if swap {
                        edge.swap();
                    }
                    #[cfg(debug_assertions)]
                    println!("walk backward, matched edge: {:?}", index);
                    continue;
                }
            }
            // insert, walk forwards
            backwards = false;
            let i1 = edge.i1;
            self.curves.last_mut().unwrap().push(edge);

            // if tmp list not empty, consume
            if let Some(next) = tmp_edges.pop() {
                edge = next;
                if edges_flat.is_empty() && tmp_edges.is_empty() {
                    self.curves.last_mut().unwrap().push(edge);
                    #[cfg(debug_assertions)]
                    println!("break !!!");
                    break;
                }
                continue;
            }
            // walk forwards
            let found_edges: Vec<(bool, usize)> = edges_flat
                .iter()
                .enumerate()
                .filter_map(|(i, e)| {
                    if e.i0 == i1 {
                        Some((false, i))
                    } else if e.i1 == i1 {
                        Some((true, i))
                    } else {
                        None
                    }
                })
                .collect();
            if !found_edges.is_empty()
                && let (swap, index) = found_edges[0]
            {
                edge = edges_flat.swap_remove(index);
                if swap {
                    edge.swap();
                }
                #[cfg(debug_assertions)]
                println!("walk forward, matched edge: {:?}", index);
                continue;
            }

            // no connected edge found: switch to next curve
            backwards = true;
            self.curves.push(Vec::<Edge>::new());
            edge = edges_flat.swap_remove(0);
            if edges_flat.is_empty() && tmp_edges.is_empty() {
                self.curves.last_mut().unwrap().push(edge);
                #[cfg(debug_assertions)]
                println!("break !!!");
                break;
            }

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

        for layer in 0..self.layer_map.len() {
            self.net_trees
                .push(Vec::<QuadTree<Point, PointNodeData>>::new());
            for _ in 0..self.net_map.len() {
                self.net_trees[layer]
                    .push(QuadTree::<Point, PointNodeData>::new(root_pos, root_rad));
            }
        }

        self.points_to_back();

        self.compute_neighbors();
        self.resample(&mut Vec::<Point>::new(), &mut Vec::<Vec<Edge>>::new());
        self.resample(&mut Vec::<Point>::new(), &mut Vec::<Vec<Edge>>::new());
        self.resample(&mut Vec::<Point>::new(), &mut Vec::<Vec<Edge>>::new());
        /*
         */
        self.sort_points();
        self.rebuild_trees();
        self.rebuild_net_trees();
        self.points_to_back();

        Ok(())
    }

    fn send(&self, tx: &Sender<Snapshot>) {
        let trees = if self.debug {
            self.trees.clone()
        } else {
            Vec::<QuadTree<Point, PointNodeData>>::new()
        };
        let (center, radius) = if !trees.is_empty() {
            (trees[0].get_pos(), trees[0].get_rad())
        } else {
            (vec2!(0.0, 0.0), 1.0)
        };
        let snapshot = Snapshot {
            iterations: self.iterations,
            center,
            radius,
            points: self.points.clone(),
            curves: self.curves.clone(),
            trees,
            new: true,
        };
        let _ = tx.send(snapshot);
    }

    fn resample(&mut self, points_buf: &mut Vec<Point>, curves_buf: &mut Vec<Vec<Edge>>) {
        const ASPECT: f32 = 2.0;
        const UNSUB_MAX_ANGLE: f32 = 10.0;

        // move points and edges to back buffer, write resampled to front
        std::mem::swap(&mut self.points, points_buf);
        std::mem::swap(&mut self.curves, curves_buf);
        self.points.clear();
        self.curves.clear();
        self.trees.iter_mut().for_each(|t| t.clear());

        let mut i0;
        let mut i1;
        for curve_read in curves_buf.iter() {
            let mut curve_write = Vec::<Edge>::new();
            if curve_read.is_empty() {
                continue;
            }
            let layer = points_buf[curve_read[0].i0].layer;
            // first point
            i0 = if let Some(j) =
                self.trees[layer].find_item(&self.points, points_buf[curve_read[0].i0].pos)
            {
                j.as_usize()
            } else {
                let j = self.points.len();
                self.points.push(points_buf[curve_read[0].i0].clone());
                self.trees[layer].insert_item(None, &self.points, j);
                j
            };
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

                // unsubdivide
                if len < 0.5 * segment_size
                    && !unsubdivide
                    && points_buf[edge.i1].neighbors == 2
                    && edge.w == next_w
                    && {
                        let p0 = points_buf[edge.i0].pos;
                        let p1 = points_buf[edge.i1].pos;
                        let p2 = points_buf[curve_read[i + 1].i1].pos;
                        // angle
                        (p0 - p1).angle_to(p2 - p1) < UNSUB_MAX_ANGLE * 180.0 / PI
                    }
                {
                    unsubdivide = true;
                    continue;
                }

                // subdivide
                if len > 1.5 * segment_size && !unsubdivide {
                    let p0 = points_buf[edge.i0].pos;
                    let p1 = points_buf[edge.i1].pos;
                    let net = points_buf[edge.i0].net;
                    l0 *= 0.5;
                    let mut point = point!(0.5 * (p1 + p0), 0.5 * w, net, layer);
                    point.set_neighbors(2);
                    i1 = self.points.len();
                    self.points.push(point);
                    self.trees[layer].insert_item(None, &self.points, i1);
                    curve_write.push(edge!(i0, i1, w, l0));
                    i0 = i1;
                }

                unsubdivide = false;
                i1 = if (i == curve_read.len() - 1 || points_buf[edge.i1].neighbors > 1)
                    && let Some(j) =
                        self.trees[layer].find_item(&self.points, points_buf[edge.i1].pos)
                {
                    j.as_usize()
                } else {
                    let point = points_buf[edge.i1].clone();
                    let j = self.points.len();
                    self.points.push(point);
                    self.trees[layer].insert_item(None, &self.points, j);
                    j
                };
                curve_write.push(edge!(i0, i1, w, l0));
                i0 = i1;
            }
            self.curves.push(curve_write);
        }
    }

    fn compute_neighbors(&mut self) {
        for edge in self.curves.iter().flatten() {
            self.points[edge.i0].neighbors += 1;
            self.points[edge.i1].neighbors += 1;
        }
    }

    fn compute_force(
        &self,
        //stack: &mut Vec<(Idx<Node<PointNodeData>>, f32)>,
        index: usize,
        degree: u32,
        self_repulsion: bool,
    ) -> Vec2 {
        let pos = self.points[index].pos;
        let net = self.points[index].net;
        let layer = self.points[index].layer;
        let mut stack = Vec::<(Idx<Node<PointNodeData>>, f32)>::new();

        let f = |delta: Vec2, rad2: f32, distsq: f32| {
            let distsq = distsq.max(MIN_DIST);
            let divisor = if (degree + 1).is_multiple_of(2) {
                powu(distsq, (degree + 1) >> 1)
            } else {
                let dist = distsq.sqrt();
                powu(dist, degree + 1)
            };
            rad2 * delta / divisor
        };

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
                    force += f(delta, tree.nodes[node].data.rad, distsq);
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
                            force += f(delta, self.points[*j].rad, distsq);
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
        let all = calc(&self.trees[layer]);
        let same = if self_repulsion {
            Vec2::ZERO
        } else {
            calc(&self.net_trees[layer][net])
        };
        mass * (all - same)
    }

    fn collide_edge(
        &mut self,
        curve_index: usize,
        edge_index: usize,
        elasticity: f32,
        self_collision: bool,
    ) {
        let mut stack = Vec::<Idx<Node<PointNodeData>>>::new();

        let i0 = self.curves[curve_index][edge_index].i0;
        let i1 = self.curves[curve_index][edge_index].i1;
        let net = self.points[self.curves[curve_index][edge_index].i0].net;
        let layer = self.points[self.curves[curve_index][edge_index].i0].layer;
        let mut p0 = self.points[i0].pos;
        let mut p1 = self.points[i1].pos;
        let rad = 0.5 * self.curves[curve_index][edge_index].w;
        let mut aabb = self.curves[curve_index][edge_index].get_aabb(&self.points);

        self.curves[curve_index][edge_index].mark = false;

        let tree = &mut self.trees[layer];
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
                    if (point_net != net || point_rad == rad && self_collision)
                        && aabb.collide_square(point_pos, point_rad)
                    {
                        let e = p1 - p0;
                        let d0 = point_pos - p0;

                        let t = (e.dot(d0) / e.length_squared()).clamp(0.0, 1.0);

                        let normal = d0 - e * t;
                        let dist_sq = normal.length_squared();
                        let collision_dist = rad + point_rad;
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
                                self.points[j.as_usize()].pos -= delta_pos * rad / collision_dist;
                                point_rad / collision_dist
                            };
                            let diff = delta_pos * weight;
                            if self.points[i0].neighbors > 1 {
                                p0 += diff * (1.0 - t);
                            }
                            if self.points[i1].neighbors > 1 {
                                p1 += diff * t;
                            }
                            aabb = AABB::edge(p0, p1, rad);
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

    let mut sim_settings = SimSettings::new();

    let mut points_buf = Vec::<Point>::new();
    let mut edges_buf = Vec::<Vec<Edge>>::new();

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
            if data.iterations.is_multiple_of(8) {
                data.resample(&mut points_buf, &mut edges_buf);
                data.sort_points();
                data.rebuild_trees();
                data.rebuild_net_trees();
                data.points_to_back();
            }

            // apply forces
            let k = 2.0;
            let mut forces = vec![Vec2::ZERO; data.points.len()];
            forces
                .par_chunks_mut(PARALLEL_CHUNK_SIZE)
                .enumerate()
                .for_each(|(chunk_idx, chunk)| {
                    chunk.iter_mut().enumerate().for_each(|(i, f)| {
                        *f = data.compute_force(
                            //&mut force_stack,
                            PARALLEL_CHUNK_SIZE * chunk_idx + i,
                            sim_settings.repulsion_degree,
                            sim_settings.self_repulsion,
                        ) * k
                            * sim_settings.noodliness;
                    });
                });

            for edge in data.curves.iter().flatten() {
                edge.apply_tension(
                    &data.points,
                    &mut forces,
                    k * (1.0 - sim_settings.noodliness),
                );
            }

            // integrate
            if sim_settings.limit_step {
                data.points
                    .iter_mut()
                    .enumerate()
                    .for_each(|(i, pt)| pt.step_force_clamped(forces[i], delta, data.min_rad));
            } else {
                data.points
                    .iter_mut()
                    .enumerate()
                    .for_each(|(i, pt)| pt.step_force(forces[i], delta));
            }

            data.rebuild_trees();
            data.rebuild_net_trees();

            // collide
            for _ in 0..sim_settings.collision_iterations {
                for i in 0..data.curves.len() {
                    for j in 0..data.curves[i].len() {
                        data.collide_edge(
                            i,
                            j,
                            sim_settings.collision_elasticity,
                            sim_settings.self_collision,
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
