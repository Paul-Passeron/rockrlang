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

use crate::{ConstraintId, ConstraintSolver, SolverHost};
use std::collections::BTreeMap;

#[derive(Clone)]
pub(crate) enum Undo<L> {
    Listeners(L, Option<Vec<ConstraintId>>),
    ReadySet(ConstraintId),
    ReadyPush(ConstraintId),
    Emitted(ConstraintId),
}

/// A token for an open snapshot. Must be consumed by exactly one of
/// [`ConstraintSolver::rollback_to`] or [`ConstraintSolver::commit`], in
/// LIFO order with respect to other snapshots.
#[must_use = "a snapshot must be rolled back or committed"]
#[derive(Debug)]
pub struct Snapshot {
    undo_len: usize,
    depth: usize,
}

fn restore<K: Ord, V>(map: &mut BTreeMap<K, V>, key: K, old: Option<V>) {
    match old {
        Some(value) => {
            map.insert(key, value);
        }
        None => {
            map.remove(&key);
        }
    }
}

impl<H: SolverHost> ConstraintSolver<H> {
    pub fn start_snapshot(&mut self) -> Snapshot {
        self.snapshot_depth += 1;
        Snapshot { undo_len: self.undo_log.len(), depth: self.snapshot_depth }
    }

    /// Undo every change made to the solver since `snapshot` was taken.
    #[allow(clippy::needless_pass_by_value, reason = "consumes the snapshot token")]
    pub fn rollback_to(&mut self, snapshot: Snapshot) {
        self.check_innermost(&snapshot);
        self.rollback_undo_log(snapshot.undo_len);
        self.end_snapshot();
    }

    /// Keep the changes made since `snapshot` was taken. If this snapshot is
    /// nested, an enclosing snapshot can still roll them back.
    #[allow(clippy::needless_pass_by_value, reason = "consumes the snapshot token")]
    pub fn commit(&mut self, snapshot: Snapshot) {
        self.check_innermost(&snapshot);
        self.end_snapshot();
    }

    pub fn in_snapshot(&self) -> bool {
        self.snapshot_depth > 0
    }

    pub(crate) fn push_undo(&mut self, undo: Undo<H::Listener>) {
        if self.in_snapshot() {
            self.undo_log.push(undo);
        }
    }

    fn check_innermost(&self, snapshot: &Snapshot) {
        assert_eq!(
            snapshot.depth, self.snapshot_depth,
            "snapshots must be closed in LIFO order"
        );
        debug_assert!(snapshot.undo_len <= self.undo_log.len());
    }

    fn end_snapshot(&mut self) {
        self.snapshot_depth -= 1;
        if self.snapshot_depth == 0 {
            self.undo_log.clear();
        }
    }

    fn rollback_undo_log(&mut self, mark: usize) {
        while self.undo_log.len() > mark {
            match self.undo_log.pop().expect("len > mark") {
                Undo::Listeners(listener, old) => {
                    restore(&mut self.listeners, listener, old);
                }
                Undo::ReadySet(id) => {
                    self.ready_set.remove(&id);
                }
                Undo::ReadyPush(id) => {
                    let popped = self.ready.pop_back();
                    debug_assert_eq!(
                        popped,
                        Some(id),
                        "`ready` must be append-only inside a snapshot"
                    );
                }
                Undo::Emitted(id) => {
                    self.constraints.remove(&id);
                    self.pending_defaults.remove(&id);
                }
            }
        }
    }
}
