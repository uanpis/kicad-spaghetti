use crate::physics::*;
use crate::tree::{Node, QuadTree};
use crate::typed_idx::Idx;
use crate::utils::*;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use glam::Vec2;
use kicad_ipc_rs::{
    KiCadClientBlocking, KiCadError,
    model::{board::PcbItem, common::PcbObjectTypeCode},
};
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

pub enum Response {
    Snapshot(Snapshot),
}

const PARALLEL_CHUNK_SIZE: usize = 256;
const METRIC: f32 = 0.5;
const MIN_DIST: f32 = 0.05;

#[derive(Clone)]
pub struct SimSettings {
    pub fix_vias: BoolResettable,

    pub damping: F32Resettable,
    pub noodliness: F32Resettable,

    pub segment_size: F32Resettable,

    pub repulsion_degree: U32Resettable,
    pub self_repulsion: BoolResettable,

    pub collision_elasticity: F32Resettable,
    pub collision_iterations: UsizeResettable,
    pub self_collision: BoolResettable,

    pub limit_step: BoolResettable,
}

impl SimSettings {
    fn new() -> Self {
        Self {
            fix_vias: false.into(),

            damping: 1.0.into(),
            noodliness: 0.5.into(),

            segment_size: 3.0.into(),

            repulsion_degree: 3.into(),
            self_repulsion: false.into(),

            collision_elasticity: 0.5.into(),
            collision_iterations: 1.into(),
            self_collision: false.into(),

            limit_step: true.into(),
        }
    }
}

pub struct Sim {
    tx: Sender<Command>,
    rx: Receiver<Response>,
    pub sim_settings: SimSettings,
    pub snapshot: Snapshot,
    handle: Option<thread::JoinHandle<()>>,
}

impl Sim {
    pub fn new() -> Self {
        let (tx, _rx) = unbounded::<Command>();
        let (_tx, rx) = bounded::<Response>(1);
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
        if let Ok(Response::Snapshot(s)) = self.rx.try_recv() {
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
    pub net_map: HashMap<String, usize>,
    pub layer_map: HashMap<i32, usize>,
    pub points: Vec<Point>,
    pub footprints: Vec<Footprint>,
    pub vias: Vec<Via>,
    pub curves: Vec<Vec<Edge>>,
    pub polygons: Vec<Polygon>,
    pub trees: Vec<QuadTree<Point, PointNodeData>>,
    pub new: bool,
}

impl Snapshot {
    pub fn new() -> Self {
        let net_map = HashMap::new();
        let layer_map = HashMap::new();
        let points = Vec::new();
        let curves = Vec::new();
        let polygons = Vec::new();
        let footprints = Vec::new();
        let vias = Vec::new();
        let trees = Vec::new();
        Self {
            iterations: 0,
            center: Vec2::ZERO,
            radius: 0.0,
            net_map,
            layer_map,
            points,
            curves,
            polygons,
            footprints,
            vias,
            trees,
            new: true,
        }
    }

    pub fn get(&mut self) -> &mut Self {
        self
    }
}

pub struct Data {
    pub debug: bool,
    pub iterations: u64,
    pub kicad_client: KiCadClientBlocking,

    pub net_clearance: Vec<f32>,
    pub net_map: HashMap<String, usize>,
    pub layer_map: HashMap<i32, usize>,

    pub points: Vec<Point>,
    pub curves: Vec<Vec<Edge>>,
    pub polygons: Vec<Polygon>,
    pub footprints: Vec<Footprint>,
    pub vias: Vec<Via>,

    pub trees: Vec<QuadTree<Point, PointNodeData>>,
    pub net_trees: Vec<Vec<QuadTree<Point, PointNodeData>>>,

    pub min_rad: f32,
}

impl Data {
    fn new(debug: bool) -> Self {
        // TODO handle no kicad
        let kicad_client = KiCadClientBlocking::connect().expect("Could not connect to KiCad");

        let net_clearance = Vec::<f32>::new();
        let net_map = HashMap::new();
        let layer_map = HashMap::new();

        let points = Vec::<Point>::new();
        let curves = Vec::<Vec<Edge>>::new();
        let polygons = Vec::new();
        let footprints = Vec::<Footprint>::new();
        let vias = Vec::<Via>::new();

        let trees = Vec::<QuadTree<Point, PointNodeData>>::new();
        let net_trees = Vec::<Vec<QuadTree<Point, PointNodeData>>>::new();
        Self {
            debug,
            iterations: 0,
            kicad_client,

            net_clearance,
            net_map,
            layer_map,

            points,
            curves,
            polygons,
            footprints,
            vias,

            trees,
            net_trees,
            min_rad: f32::INFINITY,
        }
    }

    fn store_prev(&mut self) {
        self.points.iter_mut().for_each(|p| p.store_prev());
        self.vias.iter_mut().for_each(|v| v.store_prev());
    }

    fn rebuild_trees(&mut self) {
        for layer in 0..self.layer_map.len() {
            self.trees[layer].clear();
        }
        for i in 0..self.points.len() {
            let layer = self.points[i].layer;
            self.trees[layer].insert_item(None, &self.points, i);
        }
        for layer in 0..self.layer_map.len() {
            self.trees[layer].update_bottom_up(&self.points, &self.net_clearance);
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
            self.net_trees[layer][net].insert_item(None, &self.points, i);
        }
        for layer in 0..self.layer_map.len() {
            for net in 0..self.net_map.len() {
                self.net_trees[layer][net].update_bottom_up(&self.points, &self.net_clearance);
            }
        }
    }

    fn find_or_add_point(&mut self, point: Point) -> usize {
        let layer = point.layer;
        if let Some(i) = self.trees[layer].find_item(&self.points, point.pos) {
            let rad = self.points[i].rad.max(point.rad);
            self.points[i].rad = rad;
            i.as_usize()
        } else {
            self.points.push(point);
            let i = self.points.len() - 1;
            self.trees[layer].insert_item(None, &self.points, i);
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
        for index in self
            .footprints
            .iter_mut()
            .flat_map(|fp| fp.attached_points.iter_mut())
        {
            *index = inverse[*index];
        }
        for index in self
            .vias
            .iter_mut()
            .flat_map(|fp| fp.attached_points.iter_mut())
        {
            *index = inverse[*index];
        }
        for polygon in self.polygons.iter_mut() {
            for pt in polygon.points.iter_mut() {
                *pt = inverse[*pt];
            }
        }
    }

    fn import(&mut self, sim_settings: &SimSettings) -> Result<(), KiCadError> {
        let items = self.kicad_client.get_items_by_type_codes(vec![
            PcbObjectTypeCode::new_trace().code,
            PcbObjectTypeCode::new_footprint_instance().code,
            PcbObjectTypeCode::new_via().code,
        ])?;

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

        let pts: Vec<_> = tracks
            .iter()
            .flat_map(|t| [t.start_nm.unwrap().to_mm(), t.end_nm.unwrap().to_mm()])
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

        // build unsorted flat vector of edges, merging duplicate points
        let mut edges_flat = Vec::<Edge>::new();
        let mut i0_map = HashMap::<usize, HashSet<usize>>::new();
        let mut i1_map = HashMap::<usize, HashSet<usize>>::new();
        for track in tracks {
            let layer = *self.layer_map.get(&track.layer.id).unwrap();
            let netname = &track.net.as_ref().unwrap().name;
            let net = match self.net_map.get(netname) {
                Some(n) => *n,
                None => {
                    let n = self.net_map.len();
                    self.net_map.insert(netname.clone(), n);
                    n
                }
            };

            let w = track.width_nm.unwrap().to_mm();
            let point0 = Point::new_free(track.start_nm.unwrap().to_mm(), 0.5 * w, net, layer);
            let point1 = Point::new_free(track.end_nm.unwrap().to_mm(), 0.5 * w, net, layer);
            let l0 = (point1.pos - point0.pos).length();

            let i0 = self.find_or_add_point(point0);
            let i1 = self.find_or_add_point(point1);
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

            edges_flat.push(Edge::new(i0, i1, w, l0));
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
        while !edges_flat.is_empty() || !tmp_edges.is_empty() {
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
                continue;
            }

            // no connected edge found: switch to next curve
            backwards = true;
            self.curves.push(Vec::<Edge>::new());
            edge = edges_flat.swap_remove(0);
            if edges_flat.is_empty() && tmp_edges.is_empty() {
                self.curves.last_mut().unwrap().push(edge);
                break;
            }
        }

        // add footprints
        self.footprints = items
            .iter()
            .filter_map(|x| {
                if let PcbItem::FootprintInstance(fp) = x {
                    Some(fp)
                } else {
                    None
                }
            })
            .enumerate()
            .map(|(i, x)| Footprint::from_kicad(x, self, i))
            .collect();

        // add vias
        self.vias = items
            .iter()
            .filter_map(|x| {
                if let PcbItem::Via(via) = x {
                    Some(via)
                } else {
                    None
                }
            })
            .enumerate()
            .map(|(i, x)| Via::from_kicad(x, self, sim_settings, i))
            .collect();

        for layer in 0..self.layer_map.len() {
            self.net_trees
                .push(Vec::<QuadTree<Point, PointNodeData>>::new());
            for _ in 0..self.net_map.len() {
                self.net_trees[layer]
                    .push(QuadTree::<Point, PointNodeData>::new(root_pos, root_rad));
            }
        }

        if let Ok(x) = self.kicad_client.get_netclass_for_nets(
            self.net_map
                .keys()
                .map(|s| kicad_ipc_rs::model::board::BoardNet {
                    code: 0,
                    name: s.clone(),
                })
                .collect(),
        ) {
            self.net_clearance = x
                .iter()
                .map(|n| {
                    n.net_class
                        .board
                        .as_ref()
                        .unwrap()
                        .clearance_nm
                        .unwrap()
                        .to_mm()
                })
                .collect();
        }

        self.rebuild_trees();

        // find points inside via, set as children
        for v in 0..self.vias.len() {
            let via = &mut self.vias[v];
            let via_pos = via.pos;
            let mut extend = Vec::new();
            let mut stack = Vec::new();
            for p in via.attached_points.iter() {
                let point = &self.points[*p];
                let pos = point.pos;
                let rad = point.rad;
                let layer = point.layer;
                let aabb = AABB::center_radius(point.pos, point.rad);
                stack.clear();
                stack.push(self.trees[layer].root);
                while let Some(node) = stack.pop() {
                    if !aabb.collide_aabb(&self.trees[layer].nodes[node].data.aabb) {
                        continue;
                    }
                    if self.trees[layer].nodes[node].is_leaf {
                        let offset = self.trees[layer].nodes[node].items;
                        let nitems = self.trees[layer].nodes[node].nitems;
                        for j in self.trees[layer].leaf_items[offset..offset + nitems].iter() {
                            let point_pos = self.points[*j].pos;
                            //let point_rad = self.points[*j].rad;
                            if (point_pos - pos).length_squared() < rad * rad
                                && matches!(self.points[*j].point_type, PointType::Free { .. })
                            {
                                self.points[*j].point_type = PointType::Child {
                                    local_pos: point_pos - via_pos,
                                    parent: ParentIndex::Via(v),
                                    has_edge: true,
                                };
                                extend.push(j.as_usize());
                            }
                        }
                    } else {
                        for child in self.trees[layer].nodes[node]
                            .children
                            .iter()
                            .filter(|x| x.as_usize() != 0usize)
                        {
                            stack.push(*child);
                        }
                    }
                }
            }
            via.attached_points.extend(extend);
        }

        self.store_prev();

        self.compute_neighbors();
        for _ in 0..6 {
            self.resample(
                &mut Vec::<Point>::new(),
                &mut Vec::<Vec<Edge>>::new(),
                sim_settings,
            );
        }
        self.sort_points();
        self.rebuild_trees();
        self.rebuild_net_trees();
        self.store_prev();

        Ok(())
    }

    fn send(&self, tx: &Sender<Response>) {
        let trees = if self.debug {
            self.trees.clone()
        } else {
            Vec::<QuadTree<Point, PointNodeData>>::new()
        };
        let (center, radius) = if !trees.is_empty() {
            (trees[0].get_pos(), trees[0].rad)
        } else {
            (vec2!(0.0, 0.0), 1.0)
        };
        let snapshot = Snapshot {
            iterations: self.iterations,
            center,
            radius,
            net_map: self.net_map.clone(),
            layer_map: self.layer_map.clone(),
            points: self.points.clone(),
            curves: self.curves.clone(),
            polygons: self.polygons.clone(),
            footprints: self.footprints.clone(),
            vias: self.vias.clone(),
            trees,
            new: true,
        };
        let _ = tx.send(Response::Snapshot(snapshot));
    }

    fn resample(
        &mut self,
        points_buf: &mut Vec<Point>,
        curves_buf: &mut Vec<Vec<Edge>>,
        sim_settings: &SimSettings,
    ) {
        const UNSUB_MAX_ANGLE: f32 = 10.0;

        // move points and edges to back buffer, write resampled to front
        std::mem::swap(&mut self.points, points_buf);
        std::mem::swap(&mut self.curves, curves_buf);
        self.points.clear();
        self.curves.clear();
        self.trees.iter_mut().for_each(|t| t.clear());

        let mut child_point_map = HashMap::new();

        let mut i0;
        let mut i1;
        for curve_read in curves_buf.iter() {
            let mut curve_write = Vec::<Edge>::new();
            if curve_read.is_empty() {
                continue;
            }
            let first = &points_buf[curve_read[0].i0];
            let layer = first.layer;
            // first point
            i0 = if let Some(j) = self.trees[layer].find_item(&self.points, first.pos) {
                j.as_usize()
            } else {
                let j = self.points.len();
                self.points.push(first.clone());
                if matches!(first.point_type, PointType::Child { .. }) {
                    child_point_map.insert(curve_read[0].i0, j);
                }
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
                let segment_size = w * sim_settings.segment_size.get();

                let neighbors = match &points_buf[edge.i1].point_type {
                    PointType::Free { neighbors, .. } => *neighbors,
                    PointType::Child { .. } => 0,
                };

                // unsubdivide
                if len < 0.33 * segment_size
                    && !unsubdivide
                    && neighbors == 2
                    && edge.w == next_w
                    && !matches!(first.point_type, PointType::Child { .. })
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
                if len > 2.0 * segment_size && !unsubdivide {
                    let p0 = points_buf[edge.i0].pos;
                    let p1 = points_buf[edge.i1].pos;
                    let net = points_buf[edge.i0].net;
                    l0 *= 0.5;
                    let mut point = Point::new_free(0.5 * (p1 + p0), 0.5 * w, net, layer);
                    point.set_neighbors(2);
                    i1 = self.points.len();
                    self.points.push(point);
                    self.trees[layer].insert_item(None, &self.points, i1);
                    curve_write.push(Edge::new(i0, i1, w, l0));
                    i0 = i1;
                }

                unsubdivide = false;
                i1 = if (i == curve_read.len() - 1 || neighbors > 1)
                    && let Some(j) =
                        self.trees[layer].find_item(&self.points, points_buf[edge.i1].pos)
                {
                    j.as_usize()
                } else {
                    let point = points_buf[edge.i1].clone();
                    let j = self.points.len();
                    if matches!(point.point_type, PointType::Child { .. }) {
                        child_point_map.insert(edge.i1, j);
                    }
                    self.points.push(point);
                    self.trees[layer].insert_item(None, &self.points, j);
                    j
                };
                curve_write.push(Edge::new(i0, i1, w, l0));
                i0 = i1;
            }
            self.curves.push(curve_write);
        }
        for fp in self.footprints.iter_mut() {
            for pt in fp.attached_points.iter_mut() {
                let i = self.points.len();
                self.points.push(points_buf[*pt].clone());
                *pt = i;
            }
        }
        for via in self.vias.iter_mut() {
            let mut i = 0;
            while i < via.attached_points.len() {
                let pt = &mut via.attached_points[i];
                let len = self.points.len();
                if let PointType::Child { has_edge, .. } = points_buf[*pt].point_type {
                    if has_edge {
                        if let Some(child) = child_point_map.get(pt) {
                            *pt = *child
                        } else {
                            // TODO fix here, breaks when merging via points.
                            via.attached_points.remove(i);
                        }
                    } else {
                        self.points.push(points_buf[*pt].clone());
                        *pt = len;
                    }
                }
                i += 1;
            }
        }
        for polygon in self.polygons.iter_mut() {
            for pt in polygon.points.iter_mut() {
                let i = self.points.len();
                self.points.push(points_buf[*pt].clone());
                *pt = i;
            }
        }
    }

    fn compute_neighbors(&mut self) {
        for edge in self.curves.iter().flatten() {
            for i in [edge.i0, edge.i1] {
                if let PointType::Free {
                    ref mut neighbors, ..
                } = self.points[i].point_type
                {
                    *neighbors += 1;
                }
            }
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
                    force += f(delta, tree.nodes[node].data.mass, distsq);
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
        //points_buf: &mut [Point],
        curve_index: usize,
        edge_index: usize,
        elasticity: f32,
        self_collision: bool,
    ) {
        let mut stack = Vec::<Idx<Node<PointNodeData>>>::new();

        let i0 = self.curves[curve_index][edge_index].i0;
        let i1 = self.curves[curve_index][edge_index].i1;

        let parent0 = if let PointType::Child {
            parent: parent0, ..
        } = self.points[i0].point_type
        {
            parent0
        } else {
            ParentIndex::Via(usize::MAX)
        };
        let parent1 = if let PointType::Child {
            parent: parent1, ..
        } = self.points[i1].point_type
        {
            parent1
        } else {
            ParentIndex::Footprint(usize::MAX)
        };
        if parent0 == parent1 {
            return;
        }

        let net = self.points[self.curves[curve_index][edge_index].i0].net;
        let clearance = self.net_clearance[net];
        let layer = self.points[self.curves[curve_index][edge_index].i0].layer;
        let mut p0 = self.points[i0].pos;
        let mut p1 = self.points[i1].pos;
        let mut offset0 = Vec2::ZERO;
        let mut offset1 = Vec2::ZERO;
        let rad = 0.5 * self.curves[curve_index][edge_index].w;
        let mass = rad * self.curves[curve_index][edge_index].l0;
        let mut aabb = self.curves[curve_index][edge_index].get_aabb(&self.points, clearance);

        //self.curves[curve_index][edge_index].mark = false;

        let tree = &self.trees[layer];
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
                    if let PointType::Child { parent, .. } = self.points[*j].point_type
                        && (parent == parent0 || parent == parent1)
                    {
                        continue;
                    }
                    let point_net = self.points[*j].net;
                    let point_pos = self.points[*j].pos;
                    let point_rad = self.points[*j].rad;
                    let point_clearance = self.net_clearance[point_net];
                    let max_clearance = clearance.max(point_clearance);
                    // cheap check
                    if (point_net != net || point_rad == rad && self_collision)
                        && aabb.collide_square(point_pos, point_rad + max_clearance)
                    {
                        let e = p1 - p0;
                        let d0 = point_pos - p0;

                        let t = (e.dot(d0) / e.length_squared()).clamp(0.0, 1.0);

                        let normal = d0 - e * t;
                        let dist_sq = normal.length_squared();
                        let collision_dist = rad + point_rad + max_clearance;
                        if dist_sq != 0.0 && dist_sq < collision_dist * collision_dist {
                            //self.curves[curve_index][edge_index].mark = true;

                            // update points
                            let dist = dist_sq.sqrt();
                            let delta_pos = ((1.0 + elasticity) * normal * (dist - collision_dist)
                                / dist)
                                .clamp_length_max(self.min_rad);

                            let other_mass = self.points[*j].get_mass(&self.points, &self.vias);
                            let total_mass = mass + other_mass;
                            let mut weight = other_mass / total_mass;
                            if weight.is_nan() {
                                weight = 1.0;
                            }

                            Self::displace(
                                j.as_usize(),
                                -delta_pos * (1.0 - weight),
                                &mut self.points,
                                &mut self.vias,
                                &mut self.polygons,
                                &mut self.footprints,
                            );

                            let diff = delta_pos * weight;
                            if !self.points[i0].is_fixed() {
                                offset0 += diff * (1.0 - t);
                                p0 += diff * (1.0 - t);
                            }
                            if !self.points[i1].is_fixed() {
                                offset1 += diff * t;
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
        Self::displace(
            i0,
            offset0,
            &mut self.points,
            &mut self.vias,
            &mut self.polygons,
            &mut self.footprints,
        );
        Self::displace(
            i1,
            offset1,
            &mut self.points,
            &mut self.vias,
            &mut self.polygons,
            &mut self.footprints,
        );
    }

    // handle collisions between via and points
    //
    // potential optimization: only check non-edge points, via to edge point collision is already
    // handled in edge-to-point collisions. for best performance would require a non-edge point only
    // tree, but i'm not sure if it's worth it as i don't expect a large number of these.
    fn collide_via(&mut self, index: usize, sim_settings: &SimSettings) {
        if self.vias[index].fixed {
            return;
        }

        let mut stack = Vec::<Idx<Node<PointNodeData>>>::new();

        let net = self.vias[index].net;
        let clearance = self.net_clearance[net];
        let mass = self.vias[index].get_mass(&self.points);
        let mut pos_offset = Vec2::ZERO;

        for x in 0..self.vias[index].attached_points.len() {
            let i = self.vias[index].attached_points[x];
            let point = &self.points[i];
            if let PointType::Child { has_edge, .. } = point.point_type
                && has_edge
            {
                continue;
            }
            let layer = point.layer;
            let rad = point.rad;
            let pos = point.pos + pos_offset;
            let aabb = AABB::center_radius(point.pos, point.rad);

            let tree = &self.trees[layer];
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
                        // skip same point
                        if j.as_usize() == i {
                            continue;
                        }
                        // skip points attached to the same parent via
                        //
                        if let PointType::Child { parent, .. } = self.points[*j].point_type
                            && parent == ParentIndex::Via(index)
                        {
                            continue;
                        }
                        let other_net = self.points[*j].net;
                        if net == other_net && !sim_settings.self_collision.get() {
                            continue;
                        }
                        let other_pos = self.points[*j].pos;
                        let other_rad = self.points[*j].rad;
                        let other_clearance = self.net_clearance[other_net];
                        let max_clearance = clearance.max(other_clearance);

                        let delta = other_pos - pos;
                        let dist_sq = delta.length_squared();
                        let collision_dist = rad + other_rad + max_clearance;
                        if dist_sq != 0.0 && dist_sq < collision_dist * collision_dist {
                            // update points
                            let dist = dist_sq.sqrt();
                            let delta_pos = ((1.0 + sim_settings.collision_elasticity.get())
                                * delta
                                * (dist - collision_dist)
                                / dist)
                                .clamp_length_max(self.min_rad);

                            let other_mass = self.points[*j].get_mass(&self.points, &self.vias);
                            let total_mass = mass + other_mass;
                            let mut weight = other_mass / total_mass;
                            if weight.is_nan() {
                                weight = 1.0;
                            }

                            Self::displace(
                                j.as_usize(),
                                -delta_pos * (1.0 - weight),
                                &mut self.points,
                                &mut self.vias,
                                &mut self.polygons,
                                &mut self.footprints,
                            );

                            let diff = delta_pos * weight;
                            if !self.points[i].is_fixed() {
                                pos_offset += diff;
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
        }
        self.vias[index].pos += pos_offset;
        self.vias[index].update_points(&mut self.points)
    }

    fn collide_polygon(&mut self, index: usize, sim_settings: &SimSettings) {
        let mut stack = Vec::<Idx<Node<PointNodeData>>>::new();

        let net = self.vias[index].net;
        let clearance = self.net_clearance[net];
        let mass = self.polygons[index].mass;
        let mut pos_offset = Vec2::ZERO;

        for x in 0..self.polygons[index].points.len() {
            //
        }
    }

    fn displace(
        idx: usize,
        offset: Vec2,
        points: &mut [Point],
        vias: &mut [Via],
        polygons: &mut [Polygon],
        footprints: &mut [Footprint],
    ) {
        if offset.length_squared() == 0.0 {
            return;
        }
        match points[idx].point_type {
            PointType::Free { .. } => {
                points[idx].pos += offset;
            }
            PointType::Child { parent, .. } => {
                match parent {
                    ParentIndex::Via(i) => {
                        vias[i].pos += offset;
                        vias[i].update_points(points);
                    }
                    ParentIndex::Polygon(i) => {
                        //
                    }
                    ParentIndex::Footprint(i) => {
                        //
                    }
                }
            }
        }
    }
}

fn sim_loop(rx: Receiver<Command>, tx: Sender<Response>) {
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
                    data.import(&sim_settings)
                        .expect("Could not import PCB from KiCad");
                    data.send(&tx);
                }
                Command::Pause => {
                    paused = true;
                }
                Command::Resume => {
                    paused = false;
                }
                Command::UpdateSettings(settings) => {
                    if sim_settings.fix_vias.get() != settings.fix_vias.get() {
                        for via in data.vias.iter_mut() {
                            via.v = Vec2::ZERO;
                            via.pos_prev = via.pos;
                            via.fixed = settings.fix_vias.get();
                        }
                    }
                    sim_settings = settings;
                }
                Command::Reset => {
                    data = Data::new(true);
                    data.import(&sim_settings)
                        .expect("Could not import PCB from KiCad");
                    data.send(&tx);
                }
            }
        }

        if !paused {
            data.iterations += 1;
            // TODO parallelize
            if data.iterations.is_multiple_of(8) {
                data.resample(&mut points_buf, &mut edges_buf, &sim_settings);
                data.sort_points();
                data.rebuild_trees();
                data.rebuild_net_trees();
                data.store_prev();
            }

            // apply forces
            let k = 1.0; // global force multiplier
            let r = 0.5; // repulsion force multiplier
            let mut point_forces = vec![Vec2::ZERO; data.points.len()];
            let mut via_forces = vec![Vec2::ZERO; data.vias.len()];
            if sim_settings.noodliness.get() != 0.0 {
                point_forces
                    .par_chunks_mut(PARALLEL_CHUNK_SIZE)
                    .enumerate()
                    .for_each(|(chunk_idx, chunk)| {
                        chunk.iter_mut().enumerate().for_each(|(i, f)| {
                            *f = data.compute_force(
                                //&mut force_stack,
                                PARALLEL_CHUNK_SIZE * chunk_idx + i,
                                sim_settings.repulsion_degree.get(),
                                sim_settings.self_repulsion.get(),
                            ) * k
                                * r
                                * sim_settings.segment_size.get()
                                * sim_settings.noodliness.get();
                        });
                    });
            }

            for edge in data.curves.iter().flatten() {
                edge.apply_tension(
                    &data.points,
                    &mut point_forces,
                    k * (1.0 - sim_settings.noodliness.get()),
                );
            }

            // integrate
            if sim_settings.limit_step.get() {
                for (i, force) in point_forces.iter().enumerate() {
                    data.points[i].step_force_clamped(
                        *force,
                        delta,
                        0.5 * data.min_rad,
                        &mut via_forces,
                    )
                }
                for (i, via_force) in via_forces.iter().enumerate() {
                    data.vias[i].step_force_clamped(
                        *via_force,
                        delta,
                        0.5 * data.min_rad,
                        &mut data.points,
                    )
                }
            } else {
                data.points
                    .iter_mut()
                    .enumerate()
                    .for_each(|(i, pt)| pt.step_force(point_forces[i], delta));
            }

            data.rebuild_trees();
            data.rebuild_net_trees();

            // collide
            //points_buf.resize(data.points.len(), Point::new())
            for _ in 0..sim_settings.collision_iterations.get() {
                for i in 0..data.curves.len() {
                    for j in 0..data.curves[i].len() {
                        data.collide_edge(
                            //&mut data.points,
                            i,
                            j,
                            sim_settings.collision_elasticity.get(),
                            sim_settings.self_collision.get(),
                        );
                    }
                }
                if !sim_settings.fix_vias.get() {
                    for i in 0..data.vias.len() {
                        data.collide_via(i, &sim_settings);
                    }
                }
            }

            // calculate velocity
            for point in data.points.iter_mut() {
                point.update_velocity(delta, 0.99)
            }
            for via in data.vias.iter_mut() {
                via.update_velocity(delta, 0.99);
            }

            #[cfg(debug_assertions)]
            {
                for point in data.points.iter() {
                    match point.point_type {
                        PointType::Free { pos_prev, .. } => {
                            assert!(
                                (point.pos - pos_prev).length() <= 10.0 * data.min_rad,
                                "large position difference: {} -> {}",
                                pos_prev,
                                point.pos
                            );
                        }
                        PointType::Child { .. } => {
                            //
                        }
                    }
                }
                for via in data.vias.iter() {
                    assert!(
                        (via.pos - via.pos_prev).length() <= 10.0 * data.min_rad,
                        "large via movement: {} -> {}",
                        via.pos_prev,
                        via.pos,
                    )
                }
            }

            // copy current position to back
            data.store_prev();
        } else {
            thread::sleep(std::time::Duration::from_millis(16));
        }
        if tx.is_empty() {
            data.send(&tx);
        }
    }
}
