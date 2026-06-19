use std::{collections::HashMap, hash::Hash};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatticeChange {
    Changed,
    Unchanged,
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
        let changed = if joined != *self {
            LatticeChange::Changed
        } else {
            LatticeChange::Unchanged
        };
        *self = joined;
        changed
    }
}

// A map from key to lattice values is itself a lattice. This is gonna be useful
// when we have locals to lattice values and want to relate that to blocks,
// etc...

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
