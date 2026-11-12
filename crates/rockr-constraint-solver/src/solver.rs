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
    collections::{BTreeMap, BTreeSet, HashSet, VecDeque},
    sync::Arc,
};

use crate::{Constraint, ConstraintId, SolverHost, snapshot::Undo};

/// Bookkeeping state of the solver. Embedded in the host, which exposes it
/// through [`SolverHost::solver`] / [`SolverHost::solver_mut`].
pub struct ConstraintSolver<H: SolverHost> {
    next_id: usize,

    pub(crate) constraints: BTreeMap<ConstraintId, Arc<Constraint<H>>>,
    pub(crate) solved: BTreeSet<ConstraintId>,
    pub(crate) pending_defaults: BTreeSet<ConstraintId>,
    pub(crate) errored: BTreeSet<ConstraintId>,
    pub(crate) listeners: BTreeMap<H::Listener, Vec<ConstraintId>>,

    pub(crate) ready: VecDeque<ConstraintId>,
    pub(crate) ready_set: HashSet<ConstraintId>,

    pub(crate) undo_log: Vec<Undo<H::Listener>>,
    pub(crate) snapshot_depth: usize,

    /// `Some` while a probe is running: emitted constraints are collected
    /// here instead of being registered globally.
    pub(crate) probe_queue: Option<Vec<Arc<Constraint<H>>>>,
}

impl<H: SolverHost> ConstraintSolver<H> {
    pub fn new() -> Self {
        Self {
            next_id: 0,

            constraints: BTreeMap::new(),
            solved: BTreeSet::new(),
            pending_defaults: BTreeSet::new(),
            errored: BTreeSet::new(),
            listeners: BTreeMap::new(),

            ready: VecDeque::new(),
            ready_set: HashSet::new(),

            undo_log: Vec::new(),
            snapshot_depth: 0,

            probe_queue: None,
        }
    }

    /// Register a new constraint. It will be tried on the next
    /// [`crate::SolverHostExt::solve_constraints`], or immediately by the
    /// enclosing probe if one is running.
    pub fn emit(&mut self, kind: H::Kind, metadata: H::Metadata) -> ConstraintId {
        let constraint = Arc::new(self.fresh(kind, metadata));
        let id = constraint.id;
        if let Some(queue) = &mut self.probe_queue {
            queue.push(constraint);
            return id;
        }
        if H::has_default(&constraint.kind) {
            self.pending_defaults.insert(id);
        }
        self.constraints.insert(id, constraint);
        self.push_undo(Undo::Emitted(id));
        self.enqueue(id);
        id
    }

    /// Something `listener` stands for changed: requeue every constraint
    /// waiting on it.
    pub fn wake(&mut self, listener: H::Listener) {
        self.record_listeners(listener);
        let woken = self.listeners.remove(&listener).unwrap_or_default();
        for id in woken {
            self.enqueue(id);
        }
    }

    /// `from` now stands for the same thing as `into` (e.g. two inference
    /// variables were unified): constraints waiting on `from` now wait on
    /// `into`.
    pub fn merge_listeners(&mut self, from: H::Listener, into: H::Listener) {
        if from == into {
            return;
        }
        self.record_listeners(from);
        self.record_listeners(into);
        if let Some(moved) = self.listeners.remove(&from) {
            self.listeners.entry(into).or_default().extend(moved);
        }
    }

    pub fn get(&self, id: ConstraintId) -> Option<&Arc<Constraint<H>>> {
        self.constraints.get(&id)
    }

    pub fn is_solved(&self, id: ConstraintId) -> bool {
        self.solved.contains(&id)
    }

    pub fn unsolved_constraints(&self) -> impl Iterator<Item = &Arc<Constraint<H>>> {
        self.constraints
            .iter()
            .filter(|(id, _)| !self.solved.contains(id))
            .map(|(_, constraint)| constraint)
    }

    pub fn in_probe(&self) -> bool {
        self.probe_queue.is_some()
    }

    pub(crate) fn fresh(&mut self, kind: H::Kind, metadata: H::Metadata) -> Constraint<H> {
        let id = ConstraintId(self.next_id);
        self.next_id += 1;
        Constraint { id, kind, metadata }
    }

    pub(crate) fn register_listeners(
        &mut self,
        id: ConstraintId,
        listeners: impl IntoIterator<Item = H::Listener>,
    ) {
        for listener in listeners {
            self.record_listeners(listener);
            self.listeners.entry(listener).or_default().push(id);
        }
    }

    /// Only valid outside snapshots: popping from `ready` isn't undoable.
    pub(crate) fn pop_ready(&mut self) -> Option<ConstraintId> {
        debug_assert!(!self.in_snapshot(), "`ready` must be append-only inside a snapshot");
        let id = self.ready.pop_front()?;
        self.ready_set.remove(&id);
        Some(id)
    }

    fn enqueue(&mut self, id: ConstraintId) {
        if self.ready_set.insert(id) {
            self.push_undo(Undo::ReadySet(id));
            self.ready.push_back(id);
            self.push_undo(Undo::ReadyPush(id));
        }
    }

    fn record_listeners(&mut self, listener: H::Listener) {
        if self.in_snapshot() {
            let old = self.listeners.get(&listener).cloned();
            self.undo_log.push(Undo::Listeners(listener, old));
        }
    }
}

impl<H: SolverHost> Default for ConstraintSolver<H> {
    fn default() -> Self {
        Self::new()
    }
}
