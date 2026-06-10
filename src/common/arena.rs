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
    marker::PhantomData,
    ops::{Index, IndexMut},
};

use crate::common::frozen::Frozen;

pub struct Arena<'a, T> {
    inner: Frozen<T>,
    _brand: PhantomData<fn(&'a T) -> &'a T>,
}

pub type Idx<'a, T> = (usize, PhantomData<&'a T>);

impl<'a, T> Arena<'a, T> {
    pub fn new() -> Self {
        Self {
            inner: Frozen::new(),
            _brand: PhantomData,
        }
    }

    pub fn insert(&'a self, elem: T) -> Idx<'a, T> {
        let id = self.next_id();
        self.inner.push(elem);
        id
    }

    pub fn next_id(&'a self) -> Idx<'a, T> {
        (self.inner.len(), Default::default())
    }
}

impl<'a, T> IndexMut<Idx<'a, T>> for Arena<'a, T> {
    fn index_mut(&mut self, index: Idx<'a, T>) -> &mut T {
        &mut self.inner[index.0]
    }
}

impl<'a, T> Index<Idx<'a, T>> for Arena<'a, T> {
    type Output = T;

    fn index(&self, index: Idx<'a, T>) -> &T {
        &self.inner[index.0]
    }
}

impl<'a, T> Default for Arena<'a, T> {
    fn default() -> Self {
        Self::new()
    }
}
