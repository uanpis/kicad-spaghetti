use crate::tree::{Node, QTreeData, QTreeItem};
use crate::typed_idx::*;
use glam::Vec2;
use std::cmp::PartialEq;

const MERGE_RADIUS: f32 = 0.000005;

#[derive(Debug, Clone)]
pub struct Point {
    pub pos: Vec2,
    pub radius: f32,
    pub net: usize,
    pub v: Vec2,
    pub f: Vec2,
    pub neighbors: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct VirtualPoint {
    pub pos: Vec2,
    pub radius: f32,
}

#[derive(Debug, Clone)]
pub struct PointNodeData {
    pub all: VirtualPoint,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub i0: usize,
    pub i1: usize,
    pub w: f32,
    pub l0: f32,
}

impl Point {
    pub fn new(v: Vec2, m: f32, net: usize) -> Self {
        Self {
            pos: v,
            radius: m,
            net,
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
    fn new() -> Self {
        Self {
            all: VirtualPoint {
                pos: Vec2::ZERO,
                radius: 0.0,
            },
        }
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
    fn new() -> Self {
        PointNodeData::new()
    }

    fn update_leaf(
        self_idx: Idx<Node<Point, PointNodeData>>,
        nodes: &mut [Node<Point, PointNodeData>],
        items: &[Point],
    ) {
        let nitems = nodes[self_idx].nitems;
        nodes[self_idx].data.all.radius = nodes[self_idx].items[0..nitems]
            .iter()
            .map(|x| items[*x].radius)
            .sum::<f32>();
        nodes[self_idx].data.all.pos = nodes[self_idx].items[0..nitems]
            .iter()
            .map(|x| items[*x].pos * items[*x].radius)
            .sum::<Vec2>()
            / nodes[self_idx].data.all.radius;
    }

    fn update_internal(
        self_idx: Idx<Node<Point, PointNodeData>>,
        nodes: &mut [Node<Point, PointNodeData>],
    ) {
        nodes[self_idx].data.all.radius = nodes[self_idx]
            .children
            .iter()
            .filter(|x| x.as_usize() != 0usize)
            .map(|x| nodes[*x].data.all.radius)
            .sum::<f32>();
        nodes[self_idx].data.all.pos = nodes[self_idx]
            .children
            .iter()
            .filter(|x| x.as_usize() != 0usize)
            .map(|x| nodes[*x].data.all.pos * nodes[*x].data.all.radius)
            .sum::<Vec2>()
            / nodes[self_idx].data.all.radius;
    }
}

impl QTreeItem for Point {
    fn get_pos(&self) -> Vec2 {
        self.pos
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
    ($x:expr, $y:expr, $r:expr, $n:expr) => {
        Point::new(vec2![$x, $y], $r, $n)
    };
    ($v:expr, $r:expr, $n:expr) => {
        Point::new($v, $r, $n)
    };
}
macro_rules! edge {
    ($i0:expr, $i1:expr, $w:expr, $l0:expr) => {
        Edge::new($i0, $i1, $w, $l0)
    };
}
pub(crate) use {edge, point, vec2};
