use std::{marker::PhantomData, ops::{Index, IndexMut}};

use crate::common::frozen::Frozen;

pub struct Arena<'a, T> {
    inner: Frozen<T>,
    _brand: PhantomData<fn(&'a T) -> &'a T>
}

pub type Idx<'a, T> = (usize, PhantomData<&'a T>);

impl <'a, T> Arena<'a, T> {
    pub fn insert(&'a self, elem: T) -> Idx<'a, T> {
        let id = self.next_id();
        self.inner.push(elem);
        id
    }

    pub fn next_id(&'a self) -> Idx<'a, T> {
        (self.inner.len(), Default::default())
    }
}


impl <'a, T> IndexMut<Idx<'a, T>> for Arena<'a, T> {
    fn index_mut(&mut self, index: Idx<'a, T>) -> &mut T {
        &mut self.inner[index.0]
    }
}

impl <'a, T> Index<Idx<'a, T>> for Arena<'a, T> {
    type Output = T;

    fn index(&self, index: Idx<'a, T>) -> &T {
        &self.inner[index.0]
    }
}


