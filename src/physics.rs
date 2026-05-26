use crate::tree::{Node, QTreeData, QTreeItem};
use crate::typed_idx::*;
use ::itertools::Itertools;
use glam::Vec2;
use std::cmp::PartialEq;
use std::collections::HashMap;

const N_SORTED: usize = 8;
const MERGE_RADIUS: f32 = 0.000005;

#[derive(Debug, Clone)]
pub struct Point {
    pub pos: Vec2,
    pub mass: f32,
    pub parent: Option<Idx<Node<Self, PointNodeData>>>,
    pub net: usize,
    pub index: usize,
    pub v: Vec2,
    pub f: Vec2,
    pub neighbors: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct VirtualPoint {
    pub pos: Vec2,
    pub other_pos: Vec2,
    pub mass: f32,
    pub net: usize,
}

#[derive(Debug, Clone)]
pub struct PointNodeData {
    pub node: Option<Idx<Node<Point, PointNodeData>>>,
    pub sum_all: VirtualPoint,
    pub virtual_points: Vec<VirtualPoint>,
    pub net_table: HashMap<usize, usize>,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub i0: usize,
    pub i1: usize,
    pub w: f32,
    pub l0: f32,
}

impl Point {
    pub fn new(
        v: Vec2,
        m: f32,
        parent: Option<Idx<Node<Self, PointNodeData>>>,
        net: usize,
    ) -> Self {
        Self {
            pos: v,
            mass: m,
            parent,
            net,
            index: 0,
            v: Vec2::ZERO,
            f: Vec2::ZERO,
            neighbors: 0,
        }
    }
    pub fn step(&mut self, delta: f32) {
        if self.neighbors > 1 {
            self.v += self.f * delta;
            self.pos += self.v * delta;
        }
        self.f = Vec2::ZERO;
    }
    pub fn apply_force(&mut self, force: Vec2) {
        self.f += force;
    }
}

impl PointNodeData {
    fn new(node: Option<Idx<Node<Point, PointNodeData>>>) -> Self {
        Self {
            node,
            sum_all: VirtualPoint{
                pos: Vec2::ZERO,
                other_pos: Vec2::ZERO,
                mass: 0.0,
                net: 0usize,
            },
            virtual_points: Vec::<VirtualPoint>::new(),
            net_table: HashMap::<usize, usize>::new(),
        }
    }

    fn update_pos(
        data: &mut [PointNodeData],
        idx: Idx<PointNodeData>,
        nodes: &[Node<Point, PointNodeData>],
        items: &[Point],
    ) {
        // calculate center of mass
        if nodes[data[idx].node.unwrap()].is_leaf {
            for i in 0..data[idx].net_table.len() {
                let net = data[idx].virtual_points[i].net;
                data[idx].virtual_points[i].pos = nodes[data[idx].node.unwrap()]
                    .items
                    .into_iter()
                    .flatten()
                    .filter(|x| items[*x].net == net)
                    .fold(Vec2::ZERO, |acc, x| acc + items[x].pos * items[x].mass)
                    / data[idx].virtual_points[i].mass;
            }
        } else {
            for i in 0..data[idx].net_table.len() {
                let net = data[idx].virtual_points[i].net;
                data[idx].virtual_points[i].pos = nodes[data[idx].node.unwrap()]
                    .children
                    .into_iter()
                    .flatten()
                    .filter_map(|child| {
                        let child_data = &data[nodes[child].data.unwrap()];
                        child_data.net_table.get(&net).map(|i| (child_data, i))
                    })
                    .fold(Vec2::ZERO, {
                        |acc, (child_data, i)| {
                            acc + child_data.virtual_points[*i].pos * child_data.virtual_points[*i].mass
                        }
                    })
                    / data[idx].virtual_points[i].mass;
            }
        }
        let total_mass = data[idx].sum_all.mass;
        let weighed_sum = data[idx]
            .virtual_points
            .iter()
            .fold(Vec2::ZERO, |acc, x| acc + x.pos * x.mass);
        if data[idx].net_table.len() > 1 {
            for i in 0..data[idx].net_table.len() {
                let local_mass = data[idx].virtual_points[i].mass;
                data[idx].virtual_points[i].other_pos =
                    (weighed_sum - data[idx].virtual_points[i].pos * local_mass)
                    / (total_mass - local_mass);
            }
        }
        data[idx].sum_all.pos = weighed_sum / total_mass;
    }

    fn update_mass(&mut self, items: &[Point], item: Idx<Point>, delta_mass: f32) {
        // update total mass
        let net = items[item].net;

        self.sum_all.mass += delta_mass;
        if let Some(i) = self.net_table.get(&net) {
            // net already exists
            let mut i = *i;
            // update total mass
            let new_mass = self.virtual_points[i].mass + delta_mass;
            self.virtual_points[i].mass = new_mass;

            let last = self.virtual_points.len() - 1;
            let last_sorted = last.min(N_SORTED);
            if new_mass > self.virtual_points[last_sorted].mass {
                if delta_mass > 0.0 {
                    // upward bubble sort
                    while i > 0 {
                        if new_mass > self.virtual_points[i - 1].mass {
                            let other_point = self.virtual_points[i - 1];
                            // swap nets
                            self.net_table.insert(net, i - 1);
                            self.net_table.insert(other_point.net, i);
                            // swap points
                            self.virtual_points[i - 1] = self.virtual_points[i];
                            self.virtual_points[i] = other_point;
                            // repeat upward
                            i -= 1;
                        } else {
                            break;
                        }
                    }
                } else {
                    // downward bubble sort
                    while i < last_sorted {
                        if new_mass < self.virtual_points[i + 1].mass {
                            let other_point = self.virtual_points[i + 1];
                            // swap nets
                            self.net_table.insert(net, i + 1);
                            self.net_table.insert(other_point.net, i);
                            // swap points
                            self.virtual_points[i + 1] = self.virtual_points[i];
                            self.virtual_points[i] = other_point;
                            // repeat downward
                            i += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
        } else {
            // add net
            let idx =
            if !self.virtual_points.is_empty() {
                let last = self.virtual_points.len() - 1;
                let last_sorted = last.min(N_SORTED);
                self.virtual_points[0..last_sorted].partition_point(|x| x.mass > delta_mass)
            } else {
                self.sum_all.pos = items[item].pos;
                0
            };
            if idx < N_SORTED {
                self.virtual_points.insert(
                    idx,
                    VirtualPoint{
                        pos: items[item].pos,
                        other_pos: self.sum_all.pos,
                        mass: delta_mass,
                        net
                    });
                for i in idx..self.virtual_points.len() {
                    self.net_table.insert(self.virtual_points[i].net, i);
                }
            } else {
                self.virtual_points.push(
                    VirtualPoint{
                        pos: items[item].pos,
                        other_pos: self.sum_all.pos,
                        mass: delta_mass,
                        net
                });
                self.net_table.insert(net, self.virtual_points.len()-1);
            }
        };
    }
}

impl Edge {
    pub fn new(i0: usize, i1: usize, w: f32, l0: f32) -> Self {
        Self { i0, i1, w, l0 }
    }
    pub fn apply_tension(&self, pts: &mut [Point], coef: f32) {
        let delta = pts[self.i1].pos - pts[self.i0].pos;
        let force = coef * delta / self.l0;
        pts[self.i0].apply_force(force);
        pts[self.i1].apply_force(-force);
    }
}

impl QTreeData<Point, PointNodeData> for PointNodeData {
    fn new(node: Option<Idx<Node<Point, PointNodeData>>>) -> Self {PointNodeData::new(node)}
    fn set_node(&mut self, node: Option<Idx<Node<Point, PointNodeData>>>) {
        self.node = node;
    }
    fn update_pos(
        data: &mut [PointNodeData],
        idx: Idx<PointNodeData>,
        nodes: &[Node<Point, PointNodeData>],
        items: &[Point]
        ) {
        Self::update_pos(data, idx, nodes, items);
    }
    fn update_mass(&mut self, items: &[Point], item: Idx<Point>, mass: f32) {
        self.update_mass(items, item, mass);
    }
}

impl QTreeItem<Point, PointNodeData> for Point {
    fn get_pos(&self) -> Vec2 {
        self.pos
    }
    fn set_parent(&mut self, parent: Option<Idx<Node<Self, PointNodeData>>>) {
        self.parent = parent;
    }
    fn get_parent(&self) -> Option<Idx<Node<Self, PointNodeData>>> {
        self.parent
    }
    fn set_index(&mut self, index: usize) {
        self.index = index;
    }
    fn get_index(&self) -> usize {
        self.index
    }
}
impl PartialEq for Point {
    fn eq(&self, other: &Point) -> bool {
        (self.pos - other.pos).length() < MERGE_RADIUS
    }
}

macro_rules! vec2 {
    ($x:expr, $y:expr) => {
        glam::Vec2::new($x, $y)
    };
}
macro_rules! point {
    ($x:expr, $y:expr, $m:expr, $p:expr, $n:expr) => {
        Point::new(vec2![$x, $y], $m, $p, $n)
    };
    ($v:expr, $m:expr, $p:expr, $n:expr) => {
        Point::new($v, $m, $p, $n)
    };
}
macro_rules! edge {
    ($i0:expr, $i1:expr, $w:expr, $l0:expr) => {
        Edge::new($i0, $i1, $w, $l0)
    };
}
pub(crate) use {edge, point, vec2};
