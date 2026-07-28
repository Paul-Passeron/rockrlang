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
    fmt::{Debug, Display},
    hash::Hash,
    marker::PhantomData,
};

pub struct BitSet<T> {
    words: Box<[usize]>,
    _phantom: PhantomData<fn() -> T>,
}

impl<T> PartialEq for BitSet<T> {
    fn eq(&self, other: &Self) -> bool {
        self.words == other.words
    }
}

impl<T> Eq for BitSet<T> {}

impl<T: 'static> Hash for BitSet<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        TypeId::of::<T>().hash(state);
        self.words.hash(state);
    }
}

impl<T> Clone for BitSet<T> {
    fn clone(&self) -> Self {
        Self { words: self.words.clone(), _phantom: PhantomData }
    }

    fn clone_from(&mut self, source: &Self) {
        self.words.clone_from(&source.words);
    }
}

impl<T: BitSetIdx + Debug> Debug for BitSet<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: BitSetIdx + Display> Display for BitSet<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter().map(|val| val.to_string())).finish()
    }
}

pub trait BitSetIdx {
    fn as_idx(&self) -> usize;
    fn from_idx(idx: usize) -> Self;
}

const WORD_BITS: usize = usize::BITS as usize;

impl<T: BitSetIdx> BitSet<T> {
    pub fn new(domain: usize) -> Self {
        Self {
            words: vec![0; domain.div_ceil(WORD_BITS).max(1)].into_boxed_slice(),
            _phantom: PhantomData,
        }
    }

    /// returns true if it has changed
    fn set(&mut self, at: &T, value: bool) -> bool {
        let idx = at.as_idx();
        let word = idx / WORD_BITS;
        let bit = idx % WORD_BITS;
        let word_ref = &mut self.words[word];
        let prev = *word_ref;
        if value {
            *word_ref |= 1 << bit;
        } else {
            *word_ref &= !(1 << bit);
        }
        prev != *word_ref
    }

    /// returns false if the element was already in the set
    pub fn insert(&mut self, at: &T) -> bool {
        self.set(at, true)
    }

    /// returns true if the element was removed from the set and false if it
    /// wasn't there at all
    pub fn remove(&mut self, at: &T) -> bool {
        self.set(at, false)
    }

    pub fn contains(&self, at: &T) -> bool {
        self.get(at)
    }

    fn get(&self, at: &T) -> bool {
        let idx = at.as_idx();
        let word = idx / WORD_BITS;
        let bit = idx % WORD_BITS;
        let masked = self.words[word] & 1 << bit;
        masked != 0
    }

    pub fn word_domain(&self) -> usize {
        self.words.len()
    }

    pub fn domain(&self) -> usize {
        self.word_domain() * WORD_BITS
    }

    /// returns true if it has changed
    pub fn union(&mut self, other: &Self) -> bool {
        let mut changed = false;
        assert!(self.word_domain() == other.word_domain());
        for (a, b) in self.words.iter_mut().zip(other.words.iter().copied()) {
            let prev = *a;
            *a |= b;
            if *a != prev {
                changed = true;
            }
        }
        changed
    }

    pub fn intersect(&mut self, other: &Self) -> bool {
        let mut changed = false;
        assert!(self.word_domain() == other.word_domain());
        for (a, b) in self.words.iter_mut().zip(other.words.iter().copied()) {
            let prev = *a;
            *a &= b;
            if *a != prev {
                changed = true;
            }
        }
        changed
    }

    pub fn substract(&mut self, other: &Self) -> bool {
        let mut changed = false;
        assert!(self.word_domain() == other.word_domain());
        for (a, b) in self.words.iter_mut().zip(other.words.iter().copied()) {
            let prev = *a;
            *a &= !b;
            if *a != prev {
                changed = true;
            }
        }
        changed
    }

    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.words.iter().enumerate().flat_map(|(w, &word)| {
            (0..WORD_BITS).filter_map(move |b| {
                (word & (1 << b) != 0).then_some(w * WORD_BITS + b).map(T::from_idx)
            })
        })
    }
}
