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

use std::{fmt::Debug, hash::Hash, marker::PhantomData, ops::Index};

use crate::common::frozen::Frozen;

pub struct Idx<T>(usize, PhantomData<T>);

pub struct Arena<T> {
    data: Frozen<T>,
}

impl<T> Arena<T> {
    pub fn insert(&self, value: T) -> Idx<T> {
        let idx = self.data.len();
        self.data.push(value);
        Idx(idx, PhantomData::default())
    }
}

impl<T> Eq for Idx<T> {}
impl<T> PartialEq for Idx<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Hash for Idx<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
        self.1.hash(state);
    }
}

impl<T> Debug for Idx<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Idx").field(&self.0).finish()
    }
}

impl<T> Clone for Idx<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData::default())
    }
}

impl<T> Copy for Idx<T> {}

impl<T> Arena<T> {
    pub fn iter<'a>(&'a self) -> impl Iterator<Item = (Idx<T>, &'a T)> {
        self.data
            .iter()
            .enumerate()
            .map(|(i, t)| (Idx::<T>(i, Default::default()), t))
    }

    pub fn iter_values<'a>(&'a self) -> impl Iterator<Item = &'a T> {
        self.data.iter()
    }

    pub fn iter_idx<'a>(&'a self) -> impl Iterator<Item = Idx<T>> {
        self.iter().map(|val| val.0)
    }
}

impl<T> Index<Idx<T>> for Arena<T> {
    type Output = T;

    fn index(&self, index: Idx<T>) -> &T {
        self.data.get(index.0)
            .expect("Out of bound index inside an arena. This should only ever happen if you index into a different arena than the one you got the Idx from.")
    }
}
