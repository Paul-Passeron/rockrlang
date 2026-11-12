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

use std::sync::Arc;

use crate::{Constraint, ConstraintSolveResult, ProbeOutcome, SolverHost, probe};

/// Driving loops, available on every [`SolverHost`].
pub trait SolverHostExt: SolverHost {
    /// Solve ready constraints until none are left, then apply default
    /// behaviours one at a time (in emission order), solving whatever
    /// they unblock, until none are left.
    ///
    /// Stops at the first error and returns the failing constraint. Errored
    /// constraints are never retried, so calling this again resumes past
    /// the error.
    ///
    /// Does nothing while a probe is running.
    fn solve_constraints(&mut self) -> Result<(), (Arc<Constraint<Self>>, Self::Err)>;

    /// Solve `constraints` in isolation. Constraints they emit are solved
    /// as part of the probe, not registered globally.
    ///
    /// Must be called inside a snapshot, which the caller rolls back.
    fn run_probe(
        &mut self,
        constraints: impl IntoIterator<Item = (Self::Kind, Self::Metadata)>,
    ) -> ProbeOutcome;
}

impl<H: SolverHost> SolverHostExt for H {
    fn solve_constraints(&mut self) -> Result<(), (Arc<Constraint<Self>>, Self::Err)> {
        if self.solver().in_probe() {
            return Ok(());
        }
        debug_assert!(
            !self.solver().in_snapshot(),
            "`solve_constraints` must not run inside a snapshot"
        );
        loop {
            while let Some(id) = self.solver_mut().pop_ready() {
                let solver = self.solver();
                if solver.errored.contains(&id) || solver.solved.contains(&id) {
                    continue;
                }
                let constraint = Arc::clone(&solver.constraints[&id]);
                match self.solve(&constraint) {
                    ConstraintSolveResult::Solved => {
                        self.solver_mut().solved.insert(id);
                    }
                    ConstraintSolveResult::Pending => {
                        let listeners: Vec<_> =
                            self.listeners(&constraint.kind).into_iter().collect();
                        self.solver_mut().register_listeners(id, listeners);
                    }
                    ConstraintSolveResult::Error(err) => {
                        self.solver_mut().errored.insert(id);
                        return Err((constraint, err));
                    }
                }
            }

            let Some(constraint) = next_pending_default(self) else {
                break;
            };
            let id = constraint.id;
            match self.apply_default(&constraint) {
                ConstraintSolveResult::Solved => {
                    self.solver_mut().solved.insert(id);
                }
                ConstraintSolveResult::Pending => {
                    panic!("`apply_default` must not return `Pending`")
                }
                ConstraintSolveResult::Error(err) => {
                    self.solver_mut().errored.insert(id);
                    return Err((constraint, err));
                }
            }
        }
        Ok(())
    }

    fn run_probe(
        &mut self,
        constraints: impl IntoIterator<Item = (Self::Kind, Self::Metadata)>,
    ) -> ProbeOutcome {
        probe::run_probe(self, constraints)
    }
}

/// Pops the oldest default-carrying constraint that is still unresolved.
fn next_pending_default<H: SolverHost>(host: &mut H) -> Option<Arc<Constraint<H>>> {
    let solver = host.solver_mut();
    while let Some(id) = solver.pending_defaults.pop_first() {
        if !solver.solved.contains(&id) && !solver.errored.contains(&id) {
            return Some(Arc::clone(&solver.constraints[&id]));
        }
    }
    None
}
