use crate::sim::Data;
use crate::tree::{HasPos, Node, QTreeData};
use crate::typed_idx::*;
use crate::utils::*;
use glam::{Mat2, Vec2};
use kicad_ipc_rs::model::board::{
    PcbFootprintInstance, PcbGraphicShapeGeometry, PcbItem, PcbPadStack, PcbPadStackLayer, PcbVia,
};
use std::f32::consts::PI;
use std::ops::Index;

#[derive(Debug, Clone)]
pub struct Point {
    pub pos: Vec2,
    pub rad: f32,
    pub net: usize,
    pub layer: usize,

    pub point_type: PointType,
}

impl Point {
    //TODO remove this.
    pub fn is_fixed(&self) -> bool {
        match self.point_type {
            PointType::Free { neighbors, .. } => neighbors < 2,
            PointType::Child { .. } => true,
        }
    }

    pub fn get_mass(&self, points: &[Point], vias: &[Via]) -> f32 {
        match self.point_type {
            PointType::Free { .. } => {
                if self.is_fixed() {
                    f32::INFINITY
                } else {
                    PI * self.rad * self.rad
                }
            }
            PointType::Child { parent, .. } => match parent {
                ParentIndex::Via(i) => {
                    let sum: f32 = vias[i]
                        .attached_points
                        .iter()
                        .map(|j| PI * points[*j].rad * points[*j].rad)
                        .sum();
                    sum
                    //f32::INFINITY
                }
                _ => f32::INFINITY,
            },
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ParentIndex {
    Via(usize),
    Footprint(usize),
    Polygon(usize),
}

#[derive(Debug, Clone)]
pub enum PointType {
    Free {
        pos_prev: Vec2,
        v: Vec2,
        neighbors: u32,
    },
    Child {
        local_pos: Vec2,
        parent: ParentIndex,
        has_edge: bool,
    },
}

#[derive(Debug, Clone)]
pub struct PointNodeData {
    pub pos: Vec2,
    pub mass: f32,
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
pub struct Polygon {
    pub points: Vec<usize>,
    pub rad: f32,
    pub mass: f32,
    pub net: usize,
    pub layer: usize,
    pub parent_index: ParentIndex,
    pub triangulation: Vec<[usize; 3]>,
}

#[derive(Debug, Clone)]
pub struct Outline {
    pub points: Vec<OutlinePoint>,
    pub layer: bool, // front courtyard : true, back courtyard : false
}

#[derive(Debug, Clone)]
pub struct OutlinePoint {
    pub pos: Vec2,
    pub w: f32,
}

#[derive(Debug, Clone)]
pub struct Footprint {
    pub id: String,

    pub pos: Vec2,
    pub rot: f32,

    pub v: Vec2,
    pub angular_v: f32,

    pub outlines: Vec<Outline>,

    pub polygons: Vec<usize>,
    pub attached_points: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct Via {
    pub id: String,

    pub pos: Vec2,
    pub pos_prev: Vec2,
    pub v: Vec2,

    pub net: usize,

    pub attached_points: Vec<usize>,
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
    pub fn new_free(pos: Vec2, rad: f32, net: usize, layer: usize) -> Self {
        Self {
            pos,
            rad,
            net,
            layer,
            point_type: PointType::Free {
                pos_prev: pos,
                v: Vec2::ZERO,
                neighbors: 0,
            },
        }
    }

    pub fn new_child(
        local_pos: Vec2,
        rad: f32,
        net: usize,
        layer: usize,
        parent: ParentIndex,
    ) -> Self {
        Self {
            pos: Vec2::ZERO,
            rad,
            net,
            layer,
            point_type: PointType::Child {
                local_pos,
                parent,
                has_edge: false,
            },
        }
    }

    pub fn step_force(&mut self, force: Vec2, delta: f32) {
        match self.point_type {
            PointType::Free {
                ref mut v,
                neighbors,
                ..
            } => {
                if neighbors > 1 {
                    *v += force * delta;
                    self.pos += *v * delta;
                }
            }
            PointType::Child { .. } => {
                //
            }
        }
    }

    pub fn step_force_clamped(
        &mut self,
        force: Vec2,
        delta: f32,
        clamp_length: f32,
        via_forces: &mut [Vec2],
    ) {
        match self.point_type {
            PointType::Free {
                ref mut v,
                neighbors,
                ..
            } => {
                if neighbors > 1 {
                    *v += force * delta / self.rad;
                    self.pos += (*v * delta).clamp_length_max(clamp_length);
                }
            }
            PointType::Child { parent, .. } => match parent {
                ParentIndex::Via(i) => {
                    via_forces[i] += force;
                }
                ParentIndex::Footprint(i) => {
                    //
                }
                _ => (),
            },
        }
    }

    pub fn set_neighbors(&mut self, new_neighbors: u32) {
        if let PointType::Free {
            ref mut neighbors, ..
        } = self.point_type
        {
            *neighbors = new_neighbors;
        }
    }

    pub fn store_prev(&mut self) {
        if let PointType::Free {
            ref mut pos_prev, ..
        } = self.point_type
        {
            *pos_prev = self.pos;
        }
    }

    pub fn update_velocity(&mut self, delta: f32, damping: f32) {
        match self.point_type {
            PointType::Free {
                ref mut v,
                pos_prev,
                ..
            } => {
                *v = (self.pos - pos_prev) / delta;
                //v = v.clamp_length_max(data.min_rad / delta);
                *v *= damping;
            }
            PointType::Child { .. } => {
                //
            }
        }
    }
}

impl PointNodeData {
    fn new() -> Self {
        Self {
            pos: Vec2::ZERO,
            mass: 0.0,
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
    pub fn get_aabb(&self, points: &[Point], expand: f32) -> AABB {
        AABB::edge(
            points[self.i0].pos,
            points[self.i1].pos,
            0.5 * self.w + expand,
        )
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

impl Polygon {
    pub fn new(
        indices: &[usize],
        net: usize,
        layer: usize,
        rad: f32,

        parent_index: ParentIndex,
        pts: &mut [Point],
    ) -> Self {
        let points = indices.into();
        let mut polygon = Self {
            points,
            net,
            layer,
            rad,
            mass: 0.0,
            parent_index,
            triangulation: Vec::new(),
        };
        polygon.triangulate(pts);
        let area = polygon.get_area(pts);
        if area < 0.0 {
            polygon.points.reverse();
        }
        polygon.mass = area.abs();
        polygon
    }

    fn update_points(&self, offset: Vec2, rotation: f32, points: &mut [Point]) {
        let transform = Mat2::from_angle(rotation);
        for i in self.points.iter() {
            let point = &mut points[*i];
            if let PointType::Child { local_pos, .. } = point.point_type {
                point.pos = offset + transform.mul_vec2(local_pos);
            }
        }
    }

    fn triangulate(&mut self, points: &[Point]) {
        // TODO ear clipping algorithm for concave polygons
        self.triangulation.clear();
        let npoints = self.points.len();
        let mut pivot = 0usize;
        let mut p1 = 1usize;
        let mut p2 = 2usize;
        while p2 < npoints {
            self.triangulation.push([pivot, p1, p2]);
            p1 = p2;
            p2 += 1;
        }
    }

    pub fn get_area(&self, points: &[Point]) -> f32 {
        let npoints = self.points.len();
        let mut area: f32 = (0..npoints)
            .map(|i| {
                let p0 = points[self.points[i]].pos;
                let p1 = points[self.points[(i + 1) % npoints]].pos;
                p0.perp_dot(p1 - p0)
            })
            .sum();
        let sign = area.signum();

        if self.rad != 0.0 {
            // approximate area of border, doesn't take into account overlapping borders in
            // concave corners
            let perimeter: f32 = (0..npoints)
                .map(|i| {
                    (points[self.points[i]].pos - points[self.points[(i + 1) % npoints]].pos)
                        .length()
                })
                .sum();
            area += perimeter * self.rad * sign;
            // assume winding number of 1
            area += PI * self.rad * self.rad * sign;
        }
        area
    }
}

fn import_padstack(
    padstack: &PcbPadStack,
    offset: Vec2,
    rotation: f32,
    net: usize,
    data: &mut Data,
    parent: ParentIndex,
) -> (Vec<usize>, Vec<usize>) {
    let mut points = Vec::<usize>::new();
    let mut polygons = Vec::<usize>::new();

    let angle = -padstack.angle.unwrap() as f32 * PI / 180.0;
    let mut add_padstack_layer = |padstack_layer: &PcbPadStackLayer,
                                  layer_override: Option<usize>| {
        let layer = match layer_override {
            Some(x) => x,
            None => *data.layer_map.get(&padstack_layer.layer).unwrap(),
        };
        let offset = offset + padstack_layer.offset.to_mm();
        let trapezoid_delta = match padstack_layer.trapezoid_delta {
            Some(x) => x.to_mm(),
            None => Vec2::ZERO,
        };

        macro_rules! add_point {
            ($rad: expr) => {
                let i = data.points.len();
                data.points
                    .push(Point::new_child(offset, $rad, net, layer, parent));
                points.push(i);
            };
        }

        macro_rules! add_edge {
            ($size: expr) => {
                let rad = 0.5 * $size.x.min($size.y);
                let x = 0.5 * $size - Vec2::new(rad, rad);
                let i = data.points.len();
                let p0 = Point::new_child(x + offset, rad, net, layer, parent);
                let p1 = Point::new_child(-x + offset, rad, net, layer, parent);
                data.points.push(p0);
                data.points.push(p1);
                points.push(i);
                points.push(i + 1);
                data.curves
                    .push(vec![Edge::new(i, i + 1, 2.0 * rad, 2.0 * x.length())])
            };
        }

        macro_rules! add_polygon {
            ($size: expr, $rad: expr) => {
                let halfwidth = 0.5 * $size.x;
                let halfheight = 0.5 * $size.y;
                let i = data.polygons.len();
                let mut poly_points = Vec::new();
                let t = 0.5 * trapezoid_delta;
                let corners = [
                    [(halfwidth - $rad) + t.y, (halfheight - $rad) - t.x],
                    [(halfwidth - $rad) - t.y, -(halfheight - $rad) + t.x],
                    [-(halfwidth - $rad) + t.y, -(halfheight - $rad) - t.x],
                    [-(halfwidth - $rad) - t.y, (halfheight - $rad) + t.x],
                ];
                for corner in corners {
                    let j = data.points.len();
                    let corner_vec2: Vec2 = corner.into();
                    data.points.push(Point::new_child(
                        corner_vec2 + offset,
                        $rad,
                        net,
                        layer,
                        //ParentIndex::Polygon(i),
                        parent,
                    ));
                    poly_points.push(j);
                }
                let poly = Polygon::new(&poly_points, net, layer, $rad, parent, &mut data.points);
                data.polygons.push(poly);
                polygons.push(i)
                //points.extend(poly_points);
            };
        }

        // TODO impl all pad shapes
        let size = padstack_layer.size.to_mm();
        match padstack_layer.shape {
            1 => {
                // circle
                add_point!(0.5 * size.x);
            }
            2 => {
                // sharp rectangle
                add_polygon!(size, 0.0);
            }
            3 => {
                // oval
                if size.x != size.y {
                    add_edge!(size);
                } else {
                    add_point!(0.5 * size.x);
                }
            }
            4 => {
                // trapezoid
                add_polygon!(size, 0.0);
            }
            5 => {
                // rounded rectangle
                let rad = size.x.min(size.y) * padstack_layer.corner_rounding_ratio as f32;
                if padstack_layer.corner_rounding_ratio >= 0.5 {
                    if size.x != size.y {
                        add_edge!(size);
                    } else {
                        add_point!(rad);
                    }
                } else {
                    add_polygon!(size, rad);
                }
            }
            _ => {
                // unhandled cases become sharp rectangle
                add_polygon!(size, 0.0);
            }
        }
    };

    match padstack.stack_type.as_deref().unwrap_or("") {
        "PST_NORMAL" => {
            for layer in padstack
                .layers
                .iter()
                .flat_map(|x| data.layer_map.get(&x.id).copied())
            {
                add_padstack_layer(&padstack.copper_layers[0], Some(layer));
            }
        }
        "PST_FRONT_INNER_BACK" => {
            let last = padstack.layers.len();
            for (i, layer) in padstack
                .layers
                .iter()
                .flat_map(|x| data.layer_map.get(&x.id).copied())
                .enumerate()
            {
                if i == 0 {
                    add_padstack_layer(&padstack.copper_layers[0], Some(layer));
                } else if i > 0 && i < last {
                    add_padstack_layer(&padstack.copper_layers[1], Some(layer));
                } else if i == last {
                    add_padstack_layer(&padstack.copper_layers[2], Some(layer));
                }
            }
        }
        "PST_CUSTOM" => {
            for padstack_layer in padstack.copper_layers.iter() {
                add_padstack_layer(padstack_layer, None);
            }
        }
        _ => (),
    }
    (points, polygons)
}

impl Via {
    pub fn from_kicad(via: &PcbVia, data: &mut Data, index: usize) -> Self {
        let via_pos = via.position_nm.to_mm();
        let netname = &via.net.as_ref().unwrap().name;
        let net = match data.net_map.get(netname) {
            Some(n) => *n,
            None => {
                let n = data.net_map.len();
                data.net_map.insert(netname.clone(), n);
                n
            }
        };
        let i = ParentIndex::Via(index);

        let (attached_points, _) = import_padstack(
            via.pad_stack.as_ref().unwrap(),
            Vec2::ZERO,
            0.0,
            net,
            data,
            i,
        );

        let via = Self {
            id: via.id.as_ref().unwrap_or(&"".into()).clone(),

            pos: via_pos,
            pos_prev: via_pos,

            v: Vec2::ZERO,
            net,

            attached_points,
        };
        via.update_points(&mut data.points);
        via
    }

    pub fn update_points(&self, points: &mut [Point]) {
        self.attached_points.iter().for_each(|i| {
            let offset = if let PointType::Child { local_pos, .. } = points[*i].point_type {
                local_pos
            } else {
                Vec2::ZERO
            };
            // TODO remove offset
            points[*i].pos = self.pos + offset;
        });
    }

    pub fn get_mass(&self, points: &[Point]) -> f32 {
        let sum: f32 = self.attached_points.iter().map(|j| points[*j].rad).sum();
        sum
    }

    pub fn step_force_clamped(
        &mut self,
        force: Vec2,
        delta: f32,
        clamp_length: f32,
        points: &mut [Point],
    ) {
        self.v += force * delta / self.get_mass(points);
        self.pos += (self.v * delta).clamp_length_max(clamp_length);
        self.update_points(points);
    }

    pub fn update_velocity(&mut self, delta: f32, damping: f32) {
        self.v = (self.pos - self.pos_prev) / delta;
        self.v *= damping;
    }

    pub fn store_prev(&mut self) {
        self.pos_prev = self.pos;
    }
}

impl Footprint {
    pub fn from_kicad(fp: &PcbFootprintInstance, data: &mut Data, i: usize) -> Self {
        let pos = fp.position_nm.to_mm();
        let rot = -fp.orientation_deg.unwrap() as f32 * PI / 180.0;
        let inverse = Mat2::from_angle(rot).inverse();

        let segments: Vec<_> = if let Some(fp_def) = fp.definition.as_ref() {
            fp_def
                .items
                .iter()
                .filter_map(|item| {
                    match item {
                        PcbItem::BoardGraphicShape(shape) => {
                            let w = shape.stroke_width_nm.to_mm();
                            let layer = shape.layer.id;

                            if let Some(l) = match layer {
                                49 => Some(false), // back courtyard
                                50 => Some(true),  // front courtyard
                                _ => None,
                            } && let Some(geo) = &shape.geometry
                            {
                                match geo {
                                    PcbGraphicShapeGeometry::Segment { start_nm, end_nm } => {
                                        Some((
                                            inverse.mul_vec2(start_nm.to_mm() - pos),
                                            inverse.mul_vec2(end_nm.to_mm() - pos),
                                            w,
                                            l,
                                        ))
                                    }
                                    // TODO impl other courtyard graphic shapes
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        }
                        _ => None,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        let (attached_points, polygons) = if let Some(fp_def) = fp.definition.as_ref() {
            fp_def
                .items
                .iter()
                .filter_map(|item| match item {
                    PcbItem::Pad(pad) => {
                        let netname = &pad.net.as_ref().unwrap().name;
                        let net = match data.net_map.get(netname) {
                            Some(n) => *n,
                            None => {
                                let n = data.net_map.len();
                                data.net_map.insert(netname.clone(), n);
                                n
                            }
                        };
                        let (attached_points, polygons) = import_padstack(
                            pad.pad_stack.as_ref().unwrap(),
                            inverse.mul_vec2(pad.position_nm.to_mm() - pos),
                            rot,
                            net,
                            data,
                            ParentIndex::Footprint(i),
                        );
                        Some((attached_points, polygons))
                    }
                    _ => None,
                })
                .fold(
                    (Vec::new(), Vec::new()),
                    |(mut acc_points, mut acc_polygons), (a, b)| {
                        acc_points.extend(a);
                        acc_polygons.extend(b);
                        (acc_points, acc_polygons)
                    },
                )
        } else {
            (Vec::new(), Vec::new())
        };

        let mut done = vec![false; segments.len()];

        let mut outlines = Vec::<Outline>::new();
        let mut current;
        while let Some((i, next, layer)) = segments.iter().enumerate().find_map(|(i, x)| {
            if !done[i] {
                Some((i, OutlinePoint { pos: x.0, w: x.2 }, x.3))
            } else {
                None
            }
        }) {
            current = next;
            let idx = outlines.len();
            outlines.push(Outline {
                points: vec![current.clone()],
                layer,
            });
            let current_curve = &mut outlines[idx].points;
            done[i] = true;
            while let Some((i, next)) = segments.iter().enumerate().find_map(|(i, x)| {
                if done[i] {
                    None
                } else if x.0 == current.pos {
                    Some((i, OutlinePoint { pos: x.1, w: x.2 }))
                } else if x.1 == current.pos {
                    Some((i, OutlinePoint { pos: x.0, w: x.2 }))
                } else {
                    None
                }
            }) {
                current = next;
                current_curve.push(current.clone());
                done[i] = true;
            }
        }

        let footprint = Self {
            id: fp.id.as_ref().unwrap_or(&"".into()).clone(),

            pos,
            rot,

            outlines,

            v: Vec2::ZERO,
            angular_v: 0.0,

            attached_points,
            polygons,
        };
        footprint.update_points(&mut data.polygons, &mut data.points);
        footprint
    }

    fn update_points(&self, polygons: &mut [Polygon], points: &mut [Point]) {
        let transform = Mat2::from_angle(self.rot);
        for i in self.attached_points.iter() {
            let point = &mut points[*i];
            if let PointType::Child { local_pos, .. } = point.point_type {
                point.pos = self.pos + transform.mul_vec2(local_pos);
            }
        }
        for i in self.polygons.iter().flat_map(|i| &polygons[*i].points) {
            let point = &mut points[*i];
            if let PointType::Child { local_pos, .. } = point.point_type {
                point.pos = self.pos + transform.mul_vec2(local_pos);
            }
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
        net_clearance: &[f32],
    ) {
        let offset = nodes[self_idx].items;
        let nitems = nodes[self_idx].nitems;

        let mut aabb = AABB::center_radius(items[leaf_items[offset]].pos, 0.0);
        let mut mass = 0.0;
        let mut pos = Vec2::ZERO;
        for item in leaf_items[offset..offset + nitems].iter() {
            let net = items[*item].net;
            let clearance = net_clearance[net];

            let item_pos = items[*item].pos;
            let item_radius = items[*item].rad;

            let minx = item_pos.x - item_radius - clearance;
            let maxx = item_pos.x + item_radius + clearance;
            aabb.minx = aabb.minx.min(minx);
            aabb.maxx = aabb.maxx.max(maxx);

            let miny = item_pos.y - item_radius - clearance;
            let maxy = item_pos.y + item_radius + clearance;
            aabb.miny = aabb.miny.min(miny);
            aabb.maxy = aabb.maxy.max(maxy);

            let item_mass = PI * item_radius * item_radius;
            mass += item_mass;
            pos += item_pos * item_mass;
        }
        pos /= mass;
        nodes[self_idx].data.mass = mass;
        nodes[self_idx].data.pos = pos;
        nodes[self_idx].data.aabb = aabb;
    }

    fn update_internal(self_idx: Idx<Node<PointNodeData>>, nodes: &mut [Node<PointNodeData>]) {
        nodes[self_idx].data.mass = nodes[self_idx]
            .children
            .iter()
            .filter(|x| x.as_usize() != 0usize)
            .map(|x| nodes[*x].data.mass)
            .sum::<f32>();
        nodes[self_idx].data.pos = nodes[self_idx]
            .children
            .iter()
            .filter(|x| x.as_usize() != 0usize)
            .map(|x| nodes[*x].data.pos * nodes[*x].data.mass)
            .sum::<Vec2>()
            / nodes[self_idx].data.mass;

        let mut bounds: Option<AABB> = None;
        nodes[self_idx]
            .children
            .iter()
            .filter(|x| x.as_usize() != 0usize)
            .for_each(|node| {
                if let Some(b) = &mut bounds {
                    let minx = nodes[*node].data.aabb.minx;
                    let maxx = nodes[*node].data.aabb.maxx;
                    b.minx = b.minx.min(minx);
                    b.maxx = b.maxx.max(maxx);

                    let miny = nodes[*node].data.aabb.miny;
                    let maxy = nodes[*node].data.aabb.maxy;
                    b.miny = b.miny.min(miny);
                    b.maxy = b.maxy.max(maxy);
                } else {
                    bounds = Some(nodes[*node].data.aabb.clone())
                }
            });
        nodes[self_idx].data.aabb = bounds.unwrap();
    }
}

impl HasPos for Point {
    fn get_pos(&self) -> Vec2 {
        self.pos
    }
}

macro_rules! vec2 {
    ($x:expr, $y:expr) => {
        glam::Vec2::new($x, $y)
    };
}
pub(crate) use vec2;
