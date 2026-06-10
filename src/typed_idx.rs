use std::clone::Clone;
use std::cmp::{Eq, Ord, Ordering, PartialEq, PartialOrd};
use std::marker::{Copy, PhantomData};
use std::ops::{Index, IndexMut};

// typed index
#[derive(Debug)]
#[repr(transparent)]
pub struct Idx<T>(usize, PhantomData<T>);
impl<T> Copy for Idx<T> {}
impl<T> Clone for Idx<T> {
    fn clone(&self) -> Idx<T> {
        *self
    }
}
impl<T> PartialEq for Idx<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}
impl<T> PartialOrd for Idx<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Eq for Idx<T> {}
impl<T> Ord for Idx<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}
impl<T> Index<Idx<T>> for [T] {
    type Output = T;
    fn index(&self, i: Idx<T>) -> &T {
        &self[i.0]
    }
}
impl<T> IndexMut<Idx<T>> for [T] {
    fn index_mut(&mut self, i: Idx<T>) -> &mut T {
        &mut self[i.0]
    }
}
impl<T> Index<Idx<T>> for Vec<T> {
    type Output = T;
    fn index(&self, i: Idx<T>) -> &T {
        &self[i.0]
    }
}
impl<T> IndexMut<Idx<T>> for Vec<T> {
    fn index_mut(&mut self, i: Idx<T>) -> &mut T {
        &mut self[i.0]
    }
}
impl<T> Idx<T> {
    pub const ZERO: Idx<T> = Idx::<T>(0usize, PhantomData::<T>);
    pub const ONE: Idx<T> = Idx::<T>(1usize, PhantomData::<T>);
    pub fn as_usize(&self) -> usize {
        self.0
    }
    pub fn is_zero(&self) -> bool {
        self.0 == 0usize
    }
}

pub fn idx<T>(i: usize) -> Idx<T> {
    Idx::<T>(i, PhantomData::<T>)
}
