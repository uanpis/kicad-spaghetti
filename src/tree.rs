use crate::typed_idx::*;
use glam::Vec2;
const BUCKET_SIZE: usize = 32;

const UP: Vec2 = Vec2::new(1.0, 0.0);
const DOWN: Vec2 = Vec2::new(-1.0, 0.0);
const LEFT: Vec2 = Vec2::new(0.0, -1.0);
const RIGHT: Vec2 = Vec2::new(0.0, 1.0);

const TOP_LEFT: Vec2 = Vec2::new(-1.0, -1.0);
const BOTTOM_LEFT: Vec2 = Vec2::new(-1.0, 1.0);
const TOP_RIGHT: Vec2 = Vec2::new(1.0, -1.0);
const BOTTOM_RIGHT: Vec2 = Vec2::new(1.0, 1.0);

const CORNERS: [Vec2; 4] = [TOP_LEFT, BOTTOM_LEFT, TOP_RIGHT, BOTTOM_RIGHT];

pub trait QTreeData<T, D> {
    // TODO make generic
    fn new(node: Option<Idx<Node<T, D>>>) -> D;
    fn set_node(&mut self, node: Option<Idx<Node<T, D>>>);
    fn update_pos(data: &mut [D], idx: Idx<D>, nodes: &[Node<T, D>], items: &[T]);
    fn update_mass(&mut self, items: &[T], item: Idx<T>, mass: f32);
}

pub trait QTreeItem<T, D> {
    fn get_pos(&self) -> Vec2;
    fn set_parent(&mut self, parent: Option<Idx<Node<T, D>>>);
    fn get_parent(&self) -> Option<Idx<Node<T, D>>>;
    fn set_index(&mut self, index: usize);
    fn get_index(&self) -> usize;
}

#[derive(Clone)]
pub struct QuadTree<T, D> {
    pub nodes: Vec<Node<T, D>>,
    pub data: Vec<D>,
    pub free_list: Vec<Idx<Node<T, D>>>,

    pub root: Idx<Node<T, D>>,
}

#[derive(Debug, Clone)]
pub struct Node<T, D> {
    pub is_leaf: bool,
    pub len: usize,

    // geometry
    pub pos: Vec2,
    pub rad: f32,
    pub data: Option<Idx<D>>,

    // indices of items
    pub nitems: usize,
    pub items: [Option<Idx<T>>; BUCKET_SIZE],
    pub free_list: [usize; BUCKET_SIZE],

    pub parent: Option<Idx<Node<T, D>>>, // index of parent node
    pub child_count: usize,
    pub children: [Option<Idx<Node<T, D>>>; 4], // indices of child nodes
}

impl<T, D> Node<T, D> {
    fn new(pos: Vec2, rad: f32, parent: Option<Idx<Node<T, D>>>, data: Option<Idx<D>>) -> Self {
        Self {
            is_leaf: true,
            len: 0usize,

            pos,
            rad,
            data,

            nitems: 0usize,
            items: [None; BUCKET_SIZE],
            free_list: std::array::from_fn(|i| i),

            parent,
            child_count: 0usize,
            children: [None; 4],
        }
    }

    fn insert(&mut self, item: Idx<T>) -> usize {
        let i = self.free_list[self.nitems];
        self.nitems += 1;
        self.items[i] = Some(item);
        i
    }
    fn remove(&mut self, i: usize) {
        if i >= BUCKET_SIZE {
            println!(
                "free list too long. node at {} with radius{}",
                self.pos, self.rad
            );
        }
        self.nitems -= 1;
        self.free_list[self.nitems] = i;
    }
}

impl<T: QTreeItem<T, D>, D: QTreeData<T, D>> QuadTree<T, D> {
    pub fn new(pos: Vec2, rad: f32) -> Self {
        let root_data = D::new(Some(idx::<Node<T, D>>(0)));
        let root_node = Node::new(pos, rad, None, Some(idx::<D>(0)));
        let data = vec![root_data];
        let nodes = vec![root_node];
        let free_list = Vec::<Idx<Node<T, D>>>::new();
        let root = idx::<Node<T, D>>(0usize);
        Self {
            data,
            nodes,
            free_list,
            root,
        }
    }

    pub fn get_pos(&self) -> Vec2 {
        self.nodes[self.root].pos
    }

    pub fn get_rad(&self) -> f32 {
        self.nodes[self.root].rad
    }

    fn descend(&mut self, node_index: Idx<Node<T, D>>, item_pos: Vec2) -> Idx<Node<T, D>> {
        let new_index;
        // find quadrant
        let node_pos = self.nodes[node_index].pos;
        let quadrant = if item_pos.x < node_pos.x {
            if item_pos.y < node_pos.y { 0 } else { 1 }
        } else {
            if item_pos.y < node_pos.y { 2 } else { 3 }
        };
        if let Some(child) = self.nodes[node_index].children[quadrant] {
            new_index = child;
        } else {
            // create leaf node
            let node_rad = self.nodes[node_index].rad;

            let new_node_pos = node_pos + 0.5 * node_rad * CORNERS[quadrant];
            let new_node_rad = 0.5 * node_rad;

            let new_node = Node::<T, D>::new(new_node_pos, new_node_rad, Some(node_index), None);

            if !self.free_list.is_empty() {
                new_index = self.free_list.pop().unwrap();
                let new_node_data = D::new(Some(new_index));
                let dataidx = Some(idx::<D>(new_index.as_usize()));
                self.nodes[new_index] = new_node;
                self.nodes[new_index].data = dataidx;
                self.data[new_index.as_usize()] = new_node_data;
            } else {
                new_index = idx::<Node<T, D>>(self.nodes.len());
                let new_node_data = D::new(Some(new_index));
                let dataidx = Some(idx::<D>(new_index.as_usize()));
                self.nodes.push(new_node);
                self.nodes[new_index].data = dataidx;
                self.data.push(new_node_data);
            }
            self.nodes[node_index].children[quadrant] = Some(new_index);
            self.nodes[node_index].child_count += 1;
        }
        new_index
    }

    pub fn insert_item(
        &mut self,
        node_index: Option<Idx<Node<T, D>>>,
        items: &mut [T],
        index: usize,
    ) {
        let index = idx::<T>(index);
        let item_pos = items[index].get_pos();

        let mut node_index = match node_index {
            Some(x) => x,
            None => self.root,
        };

        loop {
            let is_leaf = self.nodes[node_index].is_leaf;
            if is_leaf {
                let len = self.nodes[node_index].len;
                if len < BUCKET_SIZE {
                    // add item to bucket
                    self.nodes[node_index].len += 1;
                    self.data[self.nodes[node_index].data.unwrap()].update_mass(items, index, 1.0);
                    let i = self.nodes[node_index].insert(index);
                    items[index].set_index(i);
                    items[index].set_parent(Some(node_index));
                    break;
                } else {
                    // split
                    self.nodes[node_index].is_leaf = false;
                    for i in 0..BUCKET_SIZE {
                        let Some(item_idx) = self.nodes[node_index].items[i] else {
                            continue;
                        };
                        let item_pos = items[item_idx].get_pos();
                        let child_idx = self.descend(node_index, item_pos);
                        self.nodes[node_index].remove(i);

                        self.nodes[child_idx].len += 1;
                        self.data[self.nodes[child_idx].data.unwrap()]
                            .update_mass(items, index, 1.0);
                        let j = self.nodes[child_idx].insert(item_idx);
                        items[item_idx].set_index(j);
                        items[item_idx].set_parent(Some(child_idx));
                    }
                }
            }
            if !is_leaf {
                self.nodes[node_index].len += 1;
                self.data[self.nodes[node_index].data.unwrap()].update_mass(items, index, 1.0);
                node_index = self.descend(node_index, item_pos);
            }
        }
    }

    pub fn find_item(&self, items: &[T], pos: Vec2) -> Option<Idx<T>> {
        let mut node_index = self.root;
        let mut x = None;

        loop {
            let is_leaf = self.nodes[node_index].is_leaf;
            if is_leaf {
                for i in 0..BUCKET_SIZE {
                    let Some(idx) = self.nodes[node_index].items[i] else {
                        continue;
                    };
                    if items[idx].get_pos() == pos {
                        x = Some(idx);
                    }
                }
                break;
            } else {
                // find quadrant
                let node_pos = self.nodes[node_index].pos;
                let quadrant = if pos.x < node_pos.x {
                    if pos.y < node_pos.y { 0 } else { 1 }
                } else {
                    if pos.y < node_pos.y { 2 } else { 3 }
                };
                if let Some(child) = self.nodes[node_index].children[quadrant] {
                    node_index = child;
                } else {
                    break;
                }
            }
        }
        x
    }

    fn is_inside(&self, node_index: Idx<Node<T, D>>, pos: Vec2) -> bool {
        (self.nodes[node_index].pos - pos).abs().max_element() <= self.nodes[node_index].rad
    }

    pub fn update_item(&mut self, items: &mut [T], index: usize) {
        let index = idx::<T>(index);
        let Some(mut parent_index) = items[index].get_parent() else {
            // no parent
            return;
        };
        let pos = items[index].get_pos();
        // skip if item is still within parent
        if self.is_inside(parent_index, pos) {
            return;
        }
        // ascend until inside grandparent
        for iteration in 0.. {
            self.nodes[parent_index].len -= 1;
            self.data[self.nodes[parent_index].data.unwrap()].update_mass(items, index, -1.0);
            if self.is_inside(parent_index, pos) {
                break;
            }
            let grandparent_option = self.nodes[parent_index].parent;
            if self.nodes[parent_index].is_leaf {
                // first leaf: remove item
                if iteration == 0 {
                    let i = items[index].get_index();
                    self.nodes[parent_index].remove(i);
                }
                if self.nodes[parent_index].len == 0 {
                    if let Some(grandparent_index) = grandparent_option {
                        self.free_list.push(parent_index);
                        for i in 0..4 {
                            if self.nodes[grandparent_index].children[i] == Some(parent_index) {
                                self.nodes[grandparent_index].children[i] = None;
                                self.nodes[grandparent_index].child_count -= 1;
                                if self.nodes[grandparent_index].child_count == 0 {
                                    self.nodes[grandparent_index].is_leaf = true;
                                }
                            }
                        }
                    } else {
                        break;
                    }
                }
            }
            if let Some(grandparent_index) = grandparent_option {
                parent_index = grandparent_index;
            } else {
                break;
            }
        }
        // insert item
        self.insert_item(Some(parent_index), items, index.as_usize());
    }

    pub fn update_bottom_up(&mut self, items: &[T]) {
        /*
        let mut done = vec![false; self.nodes.len()];
        for (i, leaf) in self.nodes.iter().filter(|x| x.is_leaf).enumerate() {
            D::update_pos(&mut self.data, leaf.data.unwrap(), &self.nodes, items);
            done[i] = true;
            let mut j = leaf.parent.unwrap().as_usize();
            while !done[j] {
                D::update_pos(
                    &mut self.data,
                    self.nodes[leaf.parent.unwrap()].data.unwrap(),
                    &self.nodes,
                    items,
                );
                done[j] = true;
                j = self.nodes[leaf.parent.unwrap()].parent.unwrap().as_usize();
            }
        }
        */

        struct Update<'s, D> {
            f: &'s dyn Fn(&Update<D>, usize, &mut Vec<D>),
        }
        let update = Update::<D> {
            f: &|update, node: usize, data: &mut Vec<D>| {
                for child in self.nodes[node].children.iter().flatten() {
                    (update.f)(update, child.as_usize(), data);
                }
                D::update_pos(data, self.nodes[node].data.unwrap(), &self.nodes, items);
            },
        };
        (update.f)(&update, self.root.as_usize(), &mut self.data);
    }

    fn div(&self, lines: &mut Vec<[Vec2; 2]>, node_index: Idx<Node<T, D>>) {
        let p = self.nodes[node_index].pos;
        let r = self.nodes[node_index].rad;
        lines.push([p + r * UP, p + r * DOWN]);
        lines.push([p + r * LEFT, p + r * RIGHT]);
        for i in self.nodes[node_index].children.into_iter().flatten() {
            if !self.nodes[i].is_leaf {
                self.div(lines, i);
            }
        }
    }

    pub fn get_viz(&self) -> Vec<[Vec2; 2]> {
        let mut lines = Vec::<[Vec2; 2]>::new();
        let p = self.get_pos();
        let r = self.get_rad();
        // frame
        lines.push([p + r * TOP_LEFT, p + r * TOP_RIGHT]);
        lines.push([p + r * TOP_RIGHT, p + r * BOTTOM_RIGHT]);
        lines.push([p + r * BOTTOM_RIGHT, p + r * BOTTOM_LEFT]);
        lines.push([p + r * BOTTOM_LEFT, p + r * TOP_LEFT]);
        // divisions
        if !self.nodes[self.root].is_leaf {
            self.div(&mut lines, self.root);
        }
        lines
    }
}
