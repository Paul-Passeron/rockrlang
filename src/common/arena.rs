/* Rockr programming language
Copyright (C) 2026  NoRezap

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use std::{
    any::TypeId,
    hash::Hash,
    marker::PhantomData,
    ops::{Index, IndexMut},
};

use crate::common::frozen::Frozen;


pub struct Arena<T> {
    inner: Frozen<T>,
}

#[derive(Debug)]
pub struct Idx<T>(usize, PhantomData<T>);

impl<T> PartialEq for Idx<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<T> Eq for Idx<T> {}

impl<T> PartialOrd for Idx<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Idx<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<T> Clone for Idx<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Hash for Idx<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
        TypeId::of::<T>().hash(state);
    }
}

impl<T> Copy for Idx<T> {}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self {
            inner: Frozen::new(),
        }
    }

    pub fn insert(&self, elem: T) -> Idx<T> {
        let id = self.next_id();
        self.inner.push(elem);
        id
    }

    pub fn next_id(&self) -> Idx<T> {
        Idx(self.inner.len(), Default::default())
    }

    pub fn get(&self, idx: Idx<T>) -> &T {
        &self[idx]
    }

    pub fn get_mut(&mut self, idx: Idx<T>) -> &mut T {
        &mut self[idx]
    }
}

impl<T> IndexMut<Idx<T>> for Arena<T> {
    fn index_mut(&mut self, index: Idx<T>) -> &mut T {
        &mut self.inner[index.0]
    }
}

impl<T> Index<Idx<T>> for Arena<T> {
    type Output = T;

    fn index(&self, index: Idx<T>) -> &T {
        &self.inner[index.0]
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Idx<T> {
    pub fn raw(&self) -> usize {
        self.0
    }
}

impl<T> Arena<T> {
    pub fn into_values(self) -> impl Iterator<Item = T> {
        self.inner.into_iter()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

impl <T: PartialEq> PartialEq for Arena<T> {
    fn eq(&self, other: &Self) -> bool {
        self.inner.iter().eq(&other.inner)
    }
}


impl <T: PartialEq + Eq> Eq for Arena<T> {}
