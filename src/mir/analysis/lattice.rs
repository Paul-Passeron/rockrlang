use std::{
    collections::{BTreeSet, HashMap, HashSet},
    hash::Hash,
};

use crate::mir::{MIR, MIRBlockID};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatticeChange {
    Changed,
    Unchanged,
}

impl From<bool> for LatticeChange {
    fn from(changed: bool) -> Self {
        match changed {
            true => Self::Changed,
            false => Self::Unchanged,
        }
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
        if self != other { Self::Changed } else { *self }
    }
}

pub trait Lattice: Eq + Clone {
    fn bottom() -> Self;

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
        HashSet::new()
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

impl<K, V> Lattice for HashMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Lattice,
{
    fn bottom() -> Self {
        HashMap::new()
    }

    fn join(&self, other: &Self) -> Self {
        let mut res = self.clone();
        for (key, other_value) in other {
            if let Some(value) = res.get_mut(key) {
                value.join_assign(other_value);
            } else {
                // Like joining with bottom
                res.insert(key.clone(), other_value.clone());
            }
        }
        res
    }

    fn join_assign(&mut self, other: &Self) -> LatticeChange {
        let mut change = LatticeChange::Unchanged;
        for (key, other_value) in other {
            if let Some(value) = self.get_mut(key) {
                let changed = value.join_assign(other_value);
                change.join_assign(&changed);
            } else {
                // Like joining with bottom. Wonder if we should
                self.insert(key.clone(), other_value.clone());
                if other_value == &V::bottom() {
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

pub type BlockMap<L> = HashMap<MIRBlockID, L>;

pub struct FixedPointIterRes<L: Lattice> {
    pub block_in: BlockMap<L>,
    pub block_out: BlockMap<L>,
}

impl MIR {
    pub fn get_block_bottoms<L: Lattice>(&self) -> impl Iterator<Item = (MIRBlockID, L)> {
        self.blocks.keys().map(|blk| (blk, L::bottom()))
    }

    pub fn fixed_point_iter<L: Lattice>(
        &self,
        direction: Direction,
        transfer: impl Fn(MIRBlockID, &L) -> L,
        in_seed: Option<BlockMap<L>>,
        out_seed: Option<BlockMap<L>>,
    ) -> FixedPointIterRes<L> {
        let mut block_in = BlockMap::from_iter(self.get_block_bottoms::<L>());
        if let Some(in_seed) = in_seed {
            block_in.join_assign(&in_seed);
        }

        let mut block_out = BlockMap::from_iter(self.get_block_bottoms::<L>());
        if let Some(out_seed) = out_seed {
            block_out.join_assign(&out_seed);
        }

        let succs = self.compute_successors();
        let preds = self.compute_predecessors(&succs);

        let mut worklist: BTreeSet<_> = BTreeSet::from_iter(self.blocks.keys());

        while let Some(blk) = worklist.pop_last() {
            let new_in = match direction {
                Direction::Forward => preds
                    .get(&blk)
                    .into_iter()
                    .flatten()
                    .fold(L::bottom(), |l, p| l.join(&block_out[p])),
                Direction::Backward => transfer(blk, &block_out[&blk]),
            };
            let new_out = match direction {
                Direction::Forward => transfer(blk, &block_in[&blk]),
                Direction::Backward => succs
                    .get(&blk)
                    .into_iter()
                    .flatten()
                    .fold(L::bottom(), |l, p| l.join(&block_in[p])),
            };

            let changed = block_in[&blk] != new_in || block_out[&blk] != new_out;

            block_in.insert(blk, new_in);
            block_out.insert(blk, new_out);

            if changed {
                match direction {
                    Direction::Forward => worklist.extend(succs[&blk].iter().copied()),
                    Direction::Backward => worklist.extend(preds[&blk].iter().copied()),
                }
            }
        }

        FixedPointIterRes {
            block_in,
            block_out,
        }
    }
}
