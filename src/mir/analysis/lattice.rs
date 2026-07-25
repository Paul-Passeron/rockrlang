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
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    hash::Hash,
};

use crate::mir::{MIRBlockID, MIRLocalID, Mir, analysis::loans::MIRStmtIndex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatticeChange {
    Changed,
    Unchanged,
}

impl From<bool> for LatticeChange {
    fn from(changed: bool) -> Self {
        if changed { Self::Changed } else { Self::Unchanged }
    }
}

impl LatticeChange {
    pub fn change(self) -> bool {
        matches!(self, Self::Changed)
    }
}

// Funny, LatticeChange is actually a lattice too (a very small one, just top
// and bottom, but still).
// This is more for fun and fireworks than anything else :)
impl Lattice for LatticeChange {
    fn bottom() -> Self {
        Self::Changed
    }

    fn join(&self, other: &Self) -> Self {
        if self == other { *self } else { Self::Changed }
    }
}

pub trait Lattice: Eq + Clone {
    fn bottom() -> Self;

    #[must_use]
    fn join(&self, other: &Self) -> Self;

    fn join_assign(&mut self, other: &Self) -> LatticeChange {
        let joined = self.join(other);
        let changed = (joined != *self).into();
        *self = joined;
        changed
    }
}

// A map from key to lattice values is itself a lattice. This is gonna be useful
// when we have locals to lattice values and want to relate that to blocks,
// etc...

impl<L> Lattice for HashSet<L>
where
    L: Eq + Hash + Clone,
{
    fn bottom() -> Self {
        Self::new()
    }

    fn join(&self, other: &Self) -> Self {
        self.iter().chain(other).cloned().collect()
    }

    fn join_assign(&mut self, other: &Self) -> LatticeChange {
        let l = self.len();
        self.extend(other.iter().cloned());
        (self.len() != l).into()
    }
}

impl<L> Lattice for BTreeSet<L>
where
    L: Eq + Ord + Clone,
{
    fn bottom() -> Self {
        Self::new()
    }

    fn join(&self, other: &Self) -> Self {
        self.iter().chain(other).cloned().collect()
    }

    fn join_assign(&mut self, other: &Self) -> LatticeChange {
        let l = self.len();
        self.extend(other.iter().cloned());
        (self.len() != l).into()
    }
}

pub trait MapLike {
    type Key;
    type Value;
    fn new_empty() -> Self;
    fn get_mut(&mut self, key: &Self::Key) -> Option<&mut Self::Value>;
    fn insert(&mut self, key: Self::Key, value: Self::Value);
    fn iter(&self) -> impl Iterator<Item = (&Self::Key, &Self::Value)>;
}

impl<K: Eq + Hash, V> MapLike for HashMap<K, V> {
    type Key = K;
    type Value = V;
    fn new_empty() -> Self {
        Self::new()
    }
    fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        Self::get_mut(self, key)
    }
    fn insert(&mut self, key: K, value: V) {
        Self::insert(self, key, value);
    }
    fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        Self::iter(self)
    }
}

impl<K: Ord, V> MapLike for BTreeMap<K, V> {
    type Key = K;
    type Value = V;

    fn new_empty() -> Self {
        Self::new()
    }
    fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        Self::get_mut(self, key)
    }
    fn insert(&mut self, key: K, value: V) {
        Self::insert(self, key, value);
    }
    fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        Self::iter(self)
    }
}

impl<M> Lattice for M
where
    M: MapLike + Clone + Eq,
    M::Key: Clone,
    M::Value: Lattice,
{
    fn bottom() -> Self {
        M::new_empty()
    }

    fn join(&self, other: &Self) -> Self {
        let mut res = self.clone();
        for (key, other_value) in other.iter() {
            if let Some(value) = res.get_mut(key) {
                value.join_assign(other_value);
            } else {
                res.insert(key.clone(), other_value.clone());
            }
        }
        res
    }

    fn join_assign(&mut self, other: &Self) -> LatticeChange {
        let mut change = LatticeChange::Unchanged;
        for (key, other_value) in other.iter() {
            if let Some(value) = self.get_mut(key) {
                let changed = value.join_assign(other_value);
                change.join_assign(&changed);
            } else {
                self.insert(key.clone(), other_value.clone());
                if other_value != &M::Value::bottom() {
                    change = LatticeChange::Changed;
                }
            }
        }
        change
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

pub type LocalMap<T> = HashMap<MIRLocalID, T>;
pub type BlockMap<L> = HashMap<MIRBlockID, L>;

pub struct FixedPointIterRes<Key, L>
where
    L: Lattice,
    Key: Hash,
{
    pub block_in: HashMap<Key, L>,
    pub block_out: HashMap<Key, L>,
}

pub type FixedPointBlockRes<L> = FixedPointIterRes<MIRBlockID, L>;
pub type FixedPointStmtRes<L> = FixedPointIterRes<MIRStmtIndex, L>;

impl Mir {
    pub fn get_block_bottoms<L: Lattice>(&self) -> impl Iterator<Item = (MIRBlockID, L)> {
        self.blocks.keys().map(|blk| (blk, L::bottom()))
    }

    pub fn fixed_point_iter<L: Lattice>(
        &self,
        direction: Direction,
        transfer: impl Fn(MIRBlockID, &L) -> L,
        in_seed: Option<&BlockMap<L>>,
        out_seed: Option<&BlockMap<L>>,
    ) -> FixedPointBlockRes<L> {
        let mut block_in = BlockMap::from_iter(self.get_block_bottoms::<L>());

        let mut block_out = BlockMap::from_iter(self.get_block_bottoms::<L>());

        let succs = self.successors();
        let preds = self.predecessors();

        let mut worklist: BTreeSet<_> = BTreeSet::from_iter(self.blocks.keys());

        while let Some(blk) = worklist.pop_last() {
            let propagate = match direction {
                Direction::Forward => {
                    let from_preds = preds
                        .get(&blk)
                        .into_iter()
                        .flatten()
                        .fold(L::bottom(), |l, p| l.join(&block_out[p]));
                    let new_in = match in_seed.as_ref().and_then(|s| s.get(&blk)) {
                        Some(seed) => from_preds.join(seed),
                        None => from_preds,
                    };
                    block_in.get_mut(&blk).unwrap().join_assign(&new_in);
                    let new_out = transfer(blk, &block_in[&blk]);
                    block_out.get_mut(&blk).unwrap().join_assign(&new_out)
                }
                Direction::Backward => {
                    let from_succs = succs
                        .get(&blk)
                        .into_iter()
                        .flatten()
                        .fold(L::bottom(), |l, p| l.join(&block_in[p]));
                    let new_out = match out_seed.as_ref().and_then(|s| s.get(&blk)) {
                        Some(seed) => from_succs.join(seed),
                        None => from_succs,
                    };
                    block_out.get_mut(&blk).unwrap().join_assign(&new_out);
                    let new_in = transfer(blk, &block_out[&blk]);
                    block_in.get_mut(&blk).unwrap().join_assign(&new_in)
                }
            };

            if propagate.change() {
                match direction {
                    Direction::Forward => worklist.extend(succs[&blk].iter().copied()),
                    Direction::Backward => worklist.extend(preds[&blk].iter().copied()),
                }
            }
        }

        FixedPointBlockRes { block_in, block_out }
    }

    pub fn stmt_index_iter(&self) -> impl Iterator<Item = MIRStmtIndex> {
        self.blocks.keys().flat_map(|blk| {
            (0..=self.blocks[blk].stmts.len()).map(move |i| MIRStmtIndex(blk, i))
        })
    }

    pub fn get_stmt_bottoms<L: Lattice>(
        &self,
    ) -> impl Iterator<Item = (MIRStmtIndex, L)> {
        self.stmt_index_iter().map(|idx| (idx, L::bottom()))
    }

    pub fn stmt_successors(&self) -> HashMap<MIRStmtIndex, HashSet<MIRStmtIndex>> {
        let mut res: HashMap<_, _> =
            self.stmt_index_iter().map(|idx| (idx, HashSet::new())).collect();
        let actual = self.successors();
        for (blk, succs) in actual {
            let stmts = &self.blocks[*blk].stmts;
            for (i, _) in stmts.iter().enumerate() {
                if i < stmts.len() - 1 {
                    res.get_mut(&MIRStmtIndex(*blk, i))
                        .unwrap()
                        .insert(MIRStmtIndex(*blk, i + 1));
                }
            }
            res.get_mut(&MIRStmtIndex(*blk, stmts.len()))
                .unwrap()
                .extend(succs.iter().map(|succ| MIRStmtIndex(*succ, 0)));
        }
        res
    }

    pub fn stmt_predecessors(
        &self,
        successors: &HashMap<MIRStmtIndex, HashSet<MIRStmtIndex>>,
    ) -> HashMap<MIRStmtIndex, HashSet<MIRStmtIndex>> {
        let mut res: HashMap<_, _> =
            self.stmt_index_iter().map(|idx| (idx, HashSet::new())).collect();
        for (idx, succs) in successors {
            for succ in succs {
                res.get_mut(succ).unwrap().insert(*idx);
            }
        }
        res
    }

    pub fn fixed_point_iter_stmt<L: Lattice>(
        &self,
        direction: Direction,
        transfer: impl Fn(MIRStmtIndex, &L) -> L,
        in_seed: Option<&HashMap<MIRStmtIndex, L>>,
        out_seed: Option<&HashMap<MIRStmtIndex, L>>,
    ) -> FixedPointStmtRes<L> {
        let mut stmt_in: HashMap<_, _> = HashMap::from_iter(self.get_stmt_bottoms::<L>());
        let mut stmt_out: HashMap<_, _> =
            HashMap::from_iter(self.get_stmt_bottoms::<L>());

        let succs = self.stmt_successors();
        let preds = self.stmt_predecessors(&succs);

        let mut worklist: BTreeSet<_> = BTreeSet::from_iter(self.stmt_index_iter());

        while let Some(blk) = worklist.pop_last() {
            let propagate = match direction {
                Direction::Forward => {
                    let from_preds = preds
                        .get(&blk)
                        .into_iter()
                        .flatten()
                        .fold(L::bottom(), |l, p| l.join(&stmt_out[p]));
                    let new_in = match in_seed.as_ref().and_then(|s| s.get(&blk)) {
                        Some(seed) => from_preds.join(seed),
                        None => from_preds,
                    };
                    stmt_in.get_mut(&blk).unwrap().join_assign(&new_in);
                    let new_out = transfer(blk, &stmt_in[&blk]);
                    stmt_out.get_mut(&blk).unwrap().join_assign(&new_out)
                }
                Direction::Backward => {
                    let from_succs = succs
                        .get(&blk)
                        .into_iter()
                        .flatten()
                        .fold(L::bottom(), |l, p| l.join(&stmt_in[p]));
                    let new_out = match out_seed.as_ref().and_then(|s| s.get(&blk)) {
                        Some(seed) => from_succs.join(seed),
                        None => from_succs,
                    };
                    stmt_out.get_mut(&blk).unwrap().join_assign(&new_out);
                    let new_in = transfer(blk, &stmt_out[&blk]);
                    stmt_in.get_mut(&blk).unwrap().join_assign(&new_in)
                }
            };

            if propagate.change() {
                match direction {
                    Direction::Forward => worklist.extend(succs[&blk].iter().copied()),
                    Direction::Backward => worklist.extend(preds[&blk].iter().copied()),
                }
            }
        }

        FixedPointStmtRes { block_in: stmt_in, block_out: stmt_out }
    }
}

impl<L: Lattice> Lattice for Option<L> {
    fn bottom() -> Self {
        Self::None
    }

    fn join(&self, other: &Self) -> Self {
        match (self, other) {
            (Some(a), Some(b)) => Some(a.join(b)),
            (Some(value), None) | (None, Some(value)) => Some(value.clone()),
            (None, None) => None,
        }
    }

    fn join_assign(&mut self, other: &Self) -> LatticeChange {
        match (self, other) {
            (Some(a), Some(b)) => a.join_assign(b),
            (this @ None, Some(value)) => {
                *this = Some(value.clone());
                LatticeChange::Changed
            }
            (_, None) => LatticeChange::Unchanged,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum Flat<T: Clone + PartialEq + Eq> {
    Top,
    Value(T),
    Bottom,
}

impl<T: Clone + PartialEq + Eq> Lattice for Flat<T> {
    fn bottom() -> Self {
        Self::Bottom
    }

    fn join(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Top, _) | (_, Self::Top) => Self::Top,
            (Self::Value(a), Self::Value(b)) => {
                if a == b {
                    Self::Value(a.clone())
                } else {
                    Self::Top
                }
            }
            (Self::Value(a), _) | (_, Self::Value(a)) => Self::Value(a.clone()),
            _ => Self::Bottom,
        }
    }
}

impl<T: Clone + PartialEq + Eq> Flat<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Value(v) => Some(v),
            _ => None,
        }
    }
}
