use std::marker::PhantomData;

// typed index
#[derive(Debug)]
pub struct Idx<T>(usize, PhantomData<T>);
impl<T> std::marker::Copy for Idx<T> {}
impl<T> std::clone::Clone for Idx<T> {
    fn clone(&self) -> Idx<T> {
        *self
    }
}
impl<T> std::cmp::PartialEq for Idx<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}
impl<T> std::cmp::PartialOrd for Idx<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}
impl<T> std::cmp::Eq for Idx<T> {}
impl<T> std::cmp::Ord for Idx<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}
impl<T> std::ops::Index<Idx<T>> for [T] {
    type Output = T;
    fn index(&self, i: Idx<T>) -> &T {
        &self[i.0]
    }
}
impl<T> std::ops::IndexMut<Idx<T>> for [T] {
    fn index_mut(&mut self, i: Idx<T>) -> &mut T {
        &mut self[i.0]
    }
}
impl<T> std::ops::Index<Idx<T>> for Vec<T> {
    type Output = T;
    fn index(&self, i: Idx<T>) -> &T {
        &self[i.0]
    }
}
impl<T> std::ops::IndexMut<Idx<T>> for Vec<T> {
    fn index_mut(&mut self, i: Idx<T>) -> &mut T {
        &mut self[i.0]
    }
}
impl<T> Idx<T> {
    pub fn as_usize(&self) -> usize {
        self.0
    }
}

pub fn idx<T>(i: usize) -> Idx<T> {
    Idx::<T>(i, PhantomData::<T>)
}
