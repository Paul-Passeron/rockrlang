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

use std::{collections::VecDeque, sync::Arc};

use crate::{ConstraintSolveResult, SolverHost};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// Every constraint was solved.
    Holds,
    /// Some constraint failed.
    Fails,
    /// No failure, but some constraints are stuck.
    Ambiguous,
}

pub(crate) fn run_probe<H: SolverHost>(
    host: &mut H,
    constraints: impl IntoIterator<Item = (H::Kind, H::Metadata)>,
) -> ProbeOutcome {
    debug_assert!(
        host.solver().in_snapshot(),
        "`run_probe` must run inside a snapshot so its effects can be rolled back"
    );
    // Nested probes stack.
    let outer = host.solver_mut().probe_queue.replace(Vec::new());
    let mut pending: VecDeque<_> = constraints
        .into_iter()
        .map(|(kind, metadata)| Arc::new(host.solver_mut().fresh(kind, metadata)))
        .collect();

    let outcome = 'outer: loop {
        let mut progressed = false;
        for _ in 0..pending.len() {
            let constraint = pending.pop_front().expect("len checked");
            match host.solve(&constraint) {
                ConstraintSolveResult::Solved => progressed = true,
                ConstraintSolveResult::Pending => pending.push_back(constraint),
                ConstraintSolveResult::Error(_) => break 'outer ProbeOutcome::Fails,
            }

            let emitted =
                std::mem::take(host.solver_mut().probe_queue.as_mut().expect("in probe"));
            if !emitted.is_empty() {
                progressed = true;
                pending.extend(emitted);
            }
        }
        if pending.is_empty() {
            break ProbeOutcome::Holds;
        }
        if !progressed {
            break ProbeOutcome::Ambiguous;
        }
    };

    host.solver_mut().probe_queue = outer;
    outcome
}
