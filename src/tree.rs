use crate::typed_idx::*;
use glam::Vec2;
const BUCKET_SIZE: usize = 32;
#[cfg(debug_assertions)]
const INSERT_LIMIT: usize = 100;

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
    fn new() -> Self;
    fn update_leaf(
        self_idx: Idx<Node<D>>,
        nodes: &mut [Node<D>],
        leaf_items: &[Idx<T>],
        items: &[T],
    );
    fn update_internal(self_idx: Idx<Node<D>>, nodes: &mut [Node<D>]);
}

pub trait QTreeItem {
    fn get_pos(&self) -> Vec2;
}

#[derive(Clone)]
pub struct QuadTree<T, D> {
    pub nodes: Vec<Node<D>>,
    pub leaf_items: Vec<Idx<T>>,
    pub root: Idx<Node<D>>,
    pub rad: f32,
}

#[derive(Debug)]
pub struct Node<D> {
    pub is_leaf: bool,

    // geometry
    pub pos: Vec2,
    pub data: D,

    // indices of items
    pub nitems: usize,
    pub items: usize, // start index in leaf_items

    pub children: [Idx<Node<D>>; 4], // indices of child nodes
}

impl<D: Clone> std::clone::Clone for Node<D> {
    fn clone(&self) -> Self {
        Self {
            is_leaf: self.is_leaf,

            pos: self.pos,
            data: self.data.clone(),

            nitems: self.nitems,
            items: self.items,

            children: self.children,
        }
    }
}

impl<T: QTreeItem, D: QTreeData<T, D> + Clone> QuadTree<T, D> {
    pub fn new(pos: Vec2, rad: f32) -> Self {
        let leaf_items = vec![Idx::<T>::ZERO; BUCKET_SIZE];
        let root_node = Node::<D> {
            is_leaf: true,

            pos,
            data: D::new(),

            nitems: 0usize,
            items: 0usize,
            children: [Idx::<Node<D>>::ZERO; 4],
        };
        let root = idx::<Node<D>>(0usize);
        let nodes = vec![root_node];
        Self {
            nodes,
            leaf_items,
            root,
            rad,
        }
    }

    fn new_node(&mut self, pos: Vec2) -> Node<D> {
        let items = self.leaf_items.len();
        self.leaf_items
            .append(&mut vec![Idx::<T>::ZERO; BUCKET_SIZE]);
        Node::<D> {
            is_leaf: true,

            pos,
            data: D::new(),

            nitems: 0usize,
            items,
            children: [Idx::<Node<D>>::ZERO; 4],
        }
    }

    fn insert_into_leaf(&mut self, node: Idx<Node<D>>, item: Idx<T>) -> usize {
        let i = self.nodes[node].nitems;
        self.nodes[node].nitems += 1;
        self.leaf_items[self.nodes[node].items + i] = item;
        i
    }

    pub fn get_pos(&self) -> Vec2 {
        self.nodes[self.root].pos
    }

    pub fn get_rad(&self) -> f32 {
        self.rad
    }

    pub fn clear(&mut self) {
        self.leaf_items.clear();
        let root_node = self.new_node(self.nodes[self.root].pos);
        self.nodes.clear();
        self.nodes.push(root_node);
    }

    fn descend(&mut self, rad: f32, node_index: Idx<Node<D>>, item_pos: Vec2) -> Idx<Node<D>> {
        // find quadrant
        let node_pos = self.nodes[node_index].pos;
        let quadrant =
            ((item_pos.x > node_pos.x) as usize) << 1 | (item_pos.y > node_pos.y) as usize;
        let child = self.nodes[node_index].children[quadrant];
        if !child.is_zero() {
            child
        } else {
            // create leaf node
            let new_node_pos = node_pos + 0.5 * rad * CORNERS[quadrant];
            let new_node = self.new_node(new_node_pos);

            let new_index = idx::<Node<D>>(self.nodes.len());
            self.nodes.push(new_node);
            self.nodes[node_index].children[quadrant] = new_index;
            new_index
        }
    }

    pub fn insert_item(&mut self, node_index: Option<Idx<Node<D>>>, items: &mut [T], index: usize) {
        let index = idx::<T>(index);
        let item_pos = items[index].get_pos();

        let mut node_index = match node_index {
            Some(x) => x,
            None => {
                if (item_pos - self.get_pos()).length() > self.rad {
                    return;
                }
                self.root
            }
        };

        let mut rad = self.rad;
        #[cfg(debug_assertions)]
        let mut i = 0;
        loop {
            #[cfg(debug_assertions)]
            {
                i += 1;
                if i > INSERT_LIMIT {
                    panic!(
                        "Tree Insertion recursion limit of {} surpassed: item at {:?}",
                        INSERT_LIMIT,
                        items[index].get_pos()
                    );
                }
            }

            let is_leaf = self.nodes[node_index].is_leaf;
            if is_leaf {
                let len = self.nodes[node_index].nitems;
                if len < BUCKET_SIZE {
                    // add item to bucket
                    self.insert_into_leaf(node_index, index);
                    break;
                } else {
                    // split
                    self.nodes[node_index].is_leaf = false;
                    for i in 0..BUCKET_SIZE {
                        let item_idx = self.leaf_items[self.nodes[node_index].items + i];
                        let item_pos = items[item_idx].get_pos();
                        let child_idx = self.descend(rad, node_index, item_pos);
                        self.insert_into_leaf(child_idx, item_idx);
                    }
                }
            }
            if !is_leaf {
                node_index = self.descend(rad, node_index, item_pos);
                rad *= 0.5;
            }
        }
    }

    pub fn find_item(&self, items: &[T], pos: Vec2) -> Option<Idx<T>> {
        let mut node_index = self.root;
        let mut x = None;

        loop {
            let is_leaf = self.nodes[node_index].is_leaf;
            if is_leaf {
                for i in 0..self.nodes[node_index].nitems {
                    let idx = self.leaf_items[self.nodes[node_index].items + i];
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
                let child = self.nodes[node_index].children[quadrant];
                if !child.is_zero() {
                    node_index = child;
                } else {
                    break;
                }
            }
        }
        x
    }

    pub fn update_bottom_up(&mut self, items: &[T]) {
        for i in (0..self.nodes.len()).rev() {
            let i = idx::<Node<D>>(i);
            let node = &mut self.nodes[i];
            if node.is_leaf {
                D::update_leaf(i, &mut self.nodes, &self.leaf_items, items);
            } else {
                D::update_internal(i, &mut self.nodes);
            }
        }
    }

    fn div(&self, r: f32, lines: &mut Vec<[Vec2; 2]>, node_index: Idx<Node<D>>) {
        let p = self.nodes[node_index].pos;
        lines.push([p + r * UP, p + r * DOWN]);
        lines.push([p + r * LEFT, p + r * RIGHT]);
        for i in self.nodes[node_index]
            .children
            .into_iter()
            .filter(|x| !x.is_zero())
        {
            if !self.nodes[i].is_leaf {
                self.div(0.5 * r, lines, i);
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
            self.div(r, &mut lines, self.root);
        }
        lines
    }
}
