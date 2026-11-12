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

#![feature(try_trait_v2, try_trait_v2_residual)]

use std::fmt;

mod driver;
mod probe;
mod result;
mod snapshot;
mod solver;

pub use driver::SolverHostExt;
pub use probe::ProbeOutcome;
pub use result::{ConstraintSolveResult, ConstraintSolveResultResidual};
pub use snapshot::Snapshot;
pub use solver::ConstraintSolver;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConstraintId(usize);

impl ConstraintId {
    pub const fn index(self) -> usize {
        self.0
    }
}

/// A constraint as stored by the solver: the host's constraint kind plus
/// whatever metadata the host wants back when reporting errors (e.g. a span).
pub struct Constraint<H: SolverHost> {
    pub id: ConstraintId,
    pub kind: H::Kind,
    pub metadata: H::Metadata,
}

impl<H: SolverHost> Clone for Constraint<H>
where
    H::Kind: Clone,
    H::Metadata: Clone,
{
    fn clone(&self) -> Self {
        Self { id: self.id, kind: self.kind.clone(), metadata: self.metadata.clone() }
    }
}

impl<H: SolverHost> fmt::Debug for Constraint<H>
where
    H::Kind: fmt::Debug,
    H::Metadata: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Constraint")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("metadata", &self.metadata)
            .finish()
    }
}

/// The type that gives meaning to constraints and owns the solver.
pub trait SolverHost: Sized {
    /// What a constraint says.
    type Kind;
    /// What a pending constraint waits on (e.g. an inference variable).
    /// Changes are reported through [`ConstraintSolver::wake`] and
    /// [`ConstraintSolver::merge_listeners`].
    type Listener: Copy + Ord;
    /// Extra data stored alongside each constraint (e.g. a span).
    type Metadata;
    type Err;

    fn solver(&self) -> &ConstraintSolver<Self>;
    fn solver_mut(&mut self) -> &mut ConstraintSolver<Self>;

    /// Try to make progress on a single constraint.
    ///
    /// Returning [`ConstraintSolveResult::Pending`] parks the constraint on
    /// the listeners reported by [`SolverHost::listeners`] until one of them
    /// is woken.
    fn solve(
        &mut self,
        constraint: &Constraint<Self>,
    ) -> ConstraintSolveResult<Self::Err>;

    /// What a pending constraint should be woken up by. Called each time the
    /// constraint goes back to pending, so it may depend on the current state.
    fn listeners(
        &mut self,
        kind: &Self::Kind,
    ) -> impl IntoIterator<Item = Self::Listener>;

    /// Whether this constraint has a fallback that will be applied once
    /// nothing else can make progress.
    fn has_default(_kind: &Self::Kind) -> bool {
        false
    }

    /// Apply the fallback of a constraint for which [`SolverHost::has_default`]
    /// is true. Must not return [`ConstraintSolveResult::Pending`].
    fn apply_default(
        &mut self,
        _constraint: &Constraint<Self>,
    ) -> ConstraintSolveResult<Self::Err> {
        unreachable!("`apply_default` called on a constraint without default behaviour")
    }
}
