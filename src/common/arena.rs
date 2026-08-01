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
    any::{TypeId, type_name},
    fmt,
    hash::Hash,
    iter::{Enumerate, Map},
    marker::PhantomData,
    ops::{Index, IndexMut},
};

use crate::common::frozen::{Frozen, FrozenIntoIter, FrozenIter};

pub struct Arena<T> {
    inner: Frozen<T>,
}

pub struct Idx<T>(usize, PhantomData<T>);

impl<T> Idx<T> {
    pub fn into_raw(self) -> usize {
        self.0
    }
}

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

impl<T> fmt::Debug for Idx<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple(format!("Idx<{}>", type_name::<T>()).as_str())
            .field(&self.0)
            .finish()
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
        Self { inner: Frozen::new() }
    }

    pub fn insert(&self, elem: T) -> Idx<T> {
        let id = self.next_id();
        self.inner.push(elem);
        id
    }

    pub fn next_id(&self) -> Idx<T> {
        Idx(self.inner.len(), PhantomData)
    }

    pub fn get(&self, idx: Idx<T>) -> &T {
        &self[idx]
    }

    pub fn get_mut(&mut self, idx: Idx<T>) -> &mut T {
        &mut self[idx]
    }

    #[allow(clippy::needless_pass_by_ref_mut)]
    pub fn get_shared_mut(&mut self, idx: Idx<T>) -> &mut T {
        self.inner.get_mut(idx.0).expect("index out of bounds")
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
    pub fn raw(self) -> usize {
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

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl<T: PartialEq> PartialEq for Arena<T> {
    fn eq(&self, other: &Self) -> bool {
        self.inner.iter().eq(&other.inner)
    }
}

impl<T: PartialEq + Eq> Eq for Arena<T> {}

impl<T> Arena<T> {
    pub fn iter(&self) -> impl Iterator<Item = (Idx<T>, &T)> {
        self.inner.iter().enumerate().map(|(i, value)| (Idx(i, PhantomData), value))
    }

    pub fn keys(&self) -> impl Iterator<Item = Idx<T>> {
        self.iter().map(|e| e.0)
    }
}

impl<T> IntoIterator for Arena<T> {
    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter().enumerate().map(|(i, value)| (Idx(i, PhantomData), value))
    }

    type Item = (Idx<T>, T);

    type IntoIter = Map<Enumerate<FrozenIntoIter<T>>, fn((usize, T)) -> (Idx<T>, T)>;
}

impl<'a, T> IntoIterator for &'a Arena<T> {
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter().enumerate().map(|(i, value)| (Idx(i, PhantomData), value))
    }

    type Item = (Idx<T>, &'a T);

    type IntoIter =
        Map<Enumerate<FrozenIter<'a, T>>, fn((usize, &'a T)) -> (Idx<T>, &'a T)>;
}

impl<T> Idx<T> {
    pub fn from_raw(raw: usize) -> Self {
        Self(raw, PhantomData)
    }
}
