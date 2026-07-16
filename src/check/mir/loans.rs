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

use std::collections::HashSet;

use salsa::Accumulator;

use crate::{
    Db,
    compiler::diagnostic::Diag,
    mir::{
        MIR, MIRBlockID, MIRLocalID,
        analysis::{
            init_tracking::IterOperand,
            loans::{LoanID, MIRLoanOut, MIRStmtIndex},
        },
        basic_block::{MIRTerminator, Stmt},
        operand::{MIROperand, MIRPlace, MIRProjection, MIRRValue, MIRRValueKind},
    },
};

pub(super) fn check_loans(db: &dyn Db, mir: &MIR) {
    let loans = mir.loans(db);
    for idx in mir.blocks.keys() {
        check_block(db, mir, idx, loans);
    }
}

fn check_block(db: &dyn Db, mir: &MIR, blk: MIRBlockID, loans: &MIRLoanOut) {
    let mut state = loans.loans_live_in[&blk].clone();
    let block = &mir.blocks[blk];
    let live_in_per_stmt = live_in_per_stmt(db, mir, blk);
    block.stmts.iter().enumerate().for_each(|(i, stmt)| {
        check_stmt(db, blk, stmt, i, loans, &live_in_per_stmt[i], &mut state)
    });
    check_terminator(db, &block.terminator, loans, &state);
}

fn live_in_per_stmt(db: &dyn Db, mir: &MIR, blk: MIRBlockID) -> Vec<HashSet<MIRLocalID>> {
    let seed = mir.liveness(db).live_out[&blk].clone();
    let mut res = vec![seed];
    for stmt in mir.blocks[blk].stmts.iter().rev() {
        let mut current = res.last().unwrap().clone();
        let Stmt::Assign { dest, rvalue } = stmt;
        current.remove(&dest.local);
        current.extend(rvalue.uses());
        res.push(current);
    }
    res.reverse();
    res
}

fn check_stmt(
    db: &dyn Db,
    blk: MIRBlockID,
    stmt: &Stmt,
    stmt_idx: usize,
    loans: &MIRLoanOut,
    live_in_this_stmt: &HashSet<MIRLocalID>,
    state: &mut HashSet<LoanID>,
) {
    let Stmt::Assign { dest, rvalue } = stmt;

    state.retain(|loan_id| {
        let holder = loans.loans[*loan_id].holder;
        holder != dest.local && live_in_this_stmt.contains(&holder)
    });

    check_rvalue_conflicts(db, rvalue, state, loans);

    if let Some(_) = rvalue.inner_place()
        && let Some(loan_id) = loans.loan_at(blk, stmt_idx)
    {
        state.insert(loan_id);
    }
}

impl MIROperand {
    pub fn as_access(&self) -> Option<(AccessKind, &MIRPlace)> {
        match self {
            MIROperand::Constant(_, _) => None,
            MIROperand::Move(place) => Some((AccessKind::Move, place)),
            MIROperand::Copy(place) => Some((AccessKind::Copy, place)),
        }
    }
}

fn check_operand(
    db: &dyn Db,
    op: &MIROperand,
    state: &HashSet<LoanID>,
    loans: &MIRLoanOut,
) {
    if let Some((access, place)) = op.as_access() {
        check_access(db, place, access, state, loans)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessKind {
    Copy,
    Move,
}

fn check_access(
    db: &dyn Db,
    place: &MIRPlace,
    access_kind: AccessKind,
    state: &HashSet<LoanID>,
    loans: &MIRLoanOut,
) {
    place.for_each_operand(|op| check_operand(db, op, state, loans));
    for loan_id in state {
        let loan = &loans.loans[*loan_id];
        if !places_conflict(&loan.place, place) {
            continue;
        }
        let violates = match access_kind {
            AccessKind::Move => true,
            AccessKind::Copy => loan.mutability.is_mut(),
        };
        if violates {
            Diag::generic_error("Conflict here (place)!".to_string(), place.span)
                .accumulate(db);
        }
    }
}

fn check_terminator(
    db: &dyn Db,
    terminator: &MIRTerminator,
    loans: &MIRLoanOut,
    state: &HashSet<LoanID>,
) {
    match terminator {
        MIRTerminator::Diverge | MIRTerminator::Goto { .. } => (),
        MIRTerminator::Call { arguments: ops, .. } => {
            ops.iter().for_each(|op| check_operand(db, op, state, loans))
        }
        MIRTerminator::Return { value: Some(op), .. }
        | MIRTerminator::Branch { cond: op, .. }
        | MIRTerminator::Switch { discriminant: op, .. } => {
            check_operand(db, op, state, loans)
        }
        MIRTerminator::Return { .. } => (),
    }
}

fn check_rvalue_conflicts(
    db: &dyn Db,
    rvalue: &MIRRValue,
    state: &HashSet<LoanID>,
    loans: &MIRLoanOut,
) {
    match &rvalue.kind {
        MIRRValueKind::Ref(p, mutability) | MIRRValueKind::AddressOf(p, mutability) => {
            p.for_each_operand(|op| check_operand(db, op, state, loans));

            for loan_id in state {
                let existing = &loans.loans[*loan_id];
                if places_conflict(p, &existing.place) {
                    let incompatible =
                        mutability.is_mut() || existing.mutability.is_mut();
                    if incompatible {
                        Diag::generic_error("Conflict here !".to_string(), rvalue.span)
                            .accumulate(db);
                    }
                }
            }
        }
        _ => rvalue.for_each_operand(|op| check_operand(db, op, state, loans)),
    }
}

fn places_conflict(place: &MIRPlace, other: &MIRPlace) -> bool {
    if place.local != other.local {
        return false;
    }

    for (pa, pb) in place.projections.iter().zip(&other.projections) {
        match (pa, pb) {
            (
                MIRProjection::Field { name: n1, .. },
                MIRProjection::Field { name: n2, .. },
            ) => {
                if n1 != n2 {
                    return false;
                }
            }
            (
                MIRProjection::TupleField { index: i1, .. },
                MIRProjection::TupleField { index: i2, .. },
            ) => {
                if i1 != i2 {
                    return false;
                }
            }
            (
                MIRProjection::Downcast { variant: v1 },
                MIRProjection::Downcast { variant: v2 },
            ) => {
                if v1 != v2 {
                    return false;
                }
            }
            (MIRProjection::Deref, MIRProjection::Deref) => {}
            (MIRProjection::Index { .. }, MIRProjection::Index { .. }) => {}
            _ => {}
        }
    }
    true
}

impl MIRLoanOut {
    pub fn loan_at(&self, block: MIRBlockID, idx: usize) -> Option<LoanID> {
        self.indices.get(&MIRStmtIndex(block, idx)).copied()
    }
}
