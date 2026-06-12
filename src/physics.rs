use crate::tree::{Node, QTreeData, QTreeItem};
use crate::typed_idx::*;
use glam::Vec2;
use std::cmp::PartialEq;
use std::ops::Index;

const MERGE_RADIUS: f32 = 0.000005;

#[derive(Debug, Clone)]
pub struct Point {
    pub pos: Vec2,
    pub pos_prev: Vec2,
    pub v: Vec2,

    pub rad: f32,
    pub net: usize,
    pub layer: usize,
    pub neighbors: u32,
}

#[derive(Debug, Clone)]
pub struct PointNodeData {
    pub pos: Vec2,
    pub rad: f32,
    pub aabb: AABB,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub i0: usize,
    pub i1: usize,
    pub w: f32,
    pub l0: f32,
    // visual debug
    pub mark: bool,
}

#[derive(Debug, Clone)]
#[allow(clippy::upper_case_acronyms)]
pub struct AABB {
    pub minx: f32,
    pub miny: f32,
    pub maxx: f32,
    pub maxy: f32,
}

impl Point {
    pub fn new(pos: Vec2, rad: f32, net: usize, layer: usize) -> Self {
        Self {
            pos,
            pos_prev: pos,
            rad,
            net,
            layer,
            v: Vec2::ZERO,
            neighbors: 0,
        }
    }

    pub fn step_force(&mut self, force: Vec2, delta: f32) {
        if self.neighbors > 1 {
            self.v += force * delta;
            self.pos += self.v * delta;
        }
    }

    pub fn step_force_clamped(&mut self, force: Vec2, delta: f32, clamp_length: f32) {
        if self.neighbors > 1 {
            self.v += force * delta;
            self.pos += (self.v * delta).clamp_length_max(clamp_length);
        }
    }

    pub fn set_neighbors(&mut self, neighbors: u32) {
        self.neighbors = neighbors;
    }

    pub fn store_prev(&mut self) {
        self.pos_prev = self.pos;
    }

    pub fn update_velocity(&mut self, delta: f32) {
        self.v = (self.pos - self.pos_prev) / delta;
    }
}

impl PointNodeData {
    fn new() -> Self {
        Self {
            pos: Vec2::ZERO,
            rad: 0.0,
            aabb: AABB::ZERO,
        }
    }
}

impl Edge {
    pub fn new(i0: usize, i1: usize, w: f32, l0: f32) -> Self {
        Self {
            i0,
            i1,
            w,
            l0,
            mark: false,
        }
    }

    pub fn get_aabb(&self, points: &[Point]) -> AABB {
        AABB::edge(points[self.i0].pos, points[self.i1].pos, 0.5 * self.w)
    }

    pub fn swap(&mut self) {
        std::mem::swap(&mut self.i0, &mut self.i1);
    }

    pub fn length(&self, points: &[Point]) -> f32 {
        (points[self.i0].pos - points[self.i1].pos).length()
    }

    pub fn apply_tension(&self, pts: &[Point], forces: &mut [Vec2], coef: f32) {
        let delta = pts[self.i1].pos - pts[self.i0].pos;
        let force = coef * delta / self.l0;
        if !force.is_nan() {
            forces[self.i0] += force;
            forces[self.i1] -= force;
        }
    }
}

impl AABB {
    pub const ZERO: Self = Self {
        minx: 0.0,
        miny: 0.0,
        maxx: 0.0,
        maxy: 0.0,
    };

    pub fn center_radius(center: Vec2, radius: f32) -> Self {
        Self {
            minx: center.x - radius,
            miny: center.y - radius,
            maxx: center.x + radius,
            maxy: center.y + radius,
        }
    }

    pub fn edge(p0: Vec2, p1: Vec2, r: f32) -> Self {
        let x0 = p0.x;
        let y0 = p0.y;
        let x1 = p1.x;
        let y1 = p1.y;
        let (minx, maxx) = if x0 < x1 {
            (x0 - r, x1 + r)
        } else {
            (x1 - r, x0 + r)
        };
        let (miny, maxy) = if y0 < y1 {
            (y0 - r, y1 + r)
        } else {
            (y1 - r, y0 + r)
        };
        AABB {
            minx,
            miny,
            maxx,
            maxy,
        }
    }

    pub fn collide_square(&self, center: Vec2, radius: f32) -> bool {
        center.x + radius >= self.minx
            && center.x - radius <= self.maxx
            && center.y + radius >= self.miny
            && center.y - radius <= self.maxy
    }

    pub fn collide_aabb(&self, other: &Self) -> bool {
        other.maxx >= self.minx
            && other.minx <= self.maxx
            && other.maxy >= self.miny
            && other.miny <= self.maxy
    }
}

impl Index<usize> for AABB {
    type Output = f32;
    fn index(&self, i: usize) -> &f32 {
        match i {
            0 => &self.minx,
            1 => &self.miny,
            2 => &self.maxx,
            3 => &self.maxy,
            _ => {
                panic!("Invalid AABB index: {}.\nOnly indices 0..=3 are valid.", i);
            }
        }
    }
}

impl QTreeData<Point, PointNodeData> for PointNodeData {
    fn new() -> Self {
        PointNodeData::new()
    }

    fn update_leaf(
        self_idx: Idx<Node<PointNodeData>>,
        nodes: &mut [Node<PointNodeData>],
        leaf_items: &[Idx<Point>],
        items: &[Point],
    ) {
        let offset = nodes[self_idx].items;
        let nitems = nodes[self_idx].nitems;

        let mut aabb = AABB::center_radius(items[leaf_items[offset]].pos, 0.0);
        let mut rad = 0.0;
        let mut pos = Vec2::ZERO;
        for item in leaf_items[offset..offset + nitems].iter() {
            let item_pos = items[*item].pos;
            let item_radius = items[*item].rad;

            let minx = item_pos.x - item_radius;
            let maxx = item_pos.x + item_radius;
            if minx < aabb.minx {
                aabb.minx = minx;
            } else if maxx > aabb.maxx {
                aabb.maxx = maxx;
            }
            let miny = item_pos.y - item_radius;
            let maxy = item_pos.y + item_radius;
            if miny < aabb.miny {
                aabb.miny = miny;
            } else if maxy > aabb.maxy {
                aabb.maxy = maxy;
            }

            rad += item_radius;
            pos += item_pos * item_radius;
        }
        pos /= rad;
        nodes[self_idx].data.rad = rad;
        nodes[self_idx].data.pos = pos;
        nodes[self_idx].data.aabb = aabb;
    }

    fn update_internal(self_idx: Idx<Node<PointNodeData>>, nodes: &mut [Node<PointNodeData>]) {
        nodes[self_idx].data.rad = nodes[self_idx]
            .children
            .iter()
            .filter(|x| x.as_usize() != 0usize)
            .map(|x| nodes[*x].data.rad)
            .sum::<f32>();
        nodes[self_idx].data.pos = nodes[self_idx]
            .children
            .iter()
            .filter(|x| x.as_usize() != 0usize)
            .map(|x| nodes[*x].data.pos * nodes[*x].data.rad)
            .sum::<Vec2>()
            / nodes[self_idx].data.rad;

        let mut bounds: Option<AABB> = None;
        nodes[self_idx]
            .children
            .iter()
            .filter(|x| x.as_usize() != 0usize)
            .for_each(|node| {
                if let Some(b) = &mut bounds {
                    let minx = nodes[*node].data.aabb.minx;
                    let maxx = nodes[*node].data.aabb.maxx;
                    if minx < b.minx {
                        b.minx = minx;
                    }
                    if maxx > b.maxx {
                        b.maxx = maxx;
                    }

                    let miny = nodes[*node].data.aabb.miny;
                    let maxy = nodes[*node].data.aabb.maxy;
                    if miny < b.miny {
                        b.miny = miny;
                    }
                    if maxy > b.maxy {
                        b.maxy = maxy;
                    }
                } else {
                    bounds = Some(nodes[*node].data.aabb.clone())
                }
            });
        nodes[self_idx].data.aabb = bounds.unwrap();
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
    ($x:expr, $y:expr, $rad:expr, $net:expr, $layer: expr) => {
        Point::new(vec2![$x, $y], $rad, $net, $layer)
    };
    ($pos:expr, $rad:expr, $net:expr, $layer:expr) => {
        Point::new($pos, $rad, $net, $layer)
    };
}
macro_rules! edge {
    ($i0:expr, $i1:expr, $w:expr, $l0:expr) => {
        Edge::new($i0, $i1, $w, $l0)
    };
}
pub(crate) use {edge, point, vec2};
