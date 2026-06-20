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
    collections::{HashMap, HashSet},
    ops::Index,
};

use crate::{
    Db,
    common::arena::{Arena, Idx},
    hir::Mutability,
    mir::{
        MIR, MIRBlockID, MIRLocalID,
        analysis::{
            MIRAnalysis,
            lattice::{BlockMap, LocalMap},
        },
        basic_block::Stmt,
        operand::{MIRPlace, MIRRValueKind},
    },
};

pub struct MIRLoanAnalysis;

pub type LoanID = Idx<Loan>;

pub struct MIRLoanOut {
    pub loans: Arena<Loan>,
    pub loans_live_in: BlockMap<HashSet<LoanID>>,
    pub loans_live_out: BlockMap<HashSet<LoanID>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MIRStmtIndex(MIRBlockID, usize);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Loan {
    pub place: MIRPlace,
    pub mutability: Mutability,
    pub created_at: MIRStmtIndex,
    pub holder: MIRLocalID,
}

impl MIRAnalysis<'_, '_> for MIRLoanAnalysis {
    type Out = MIRLoanOut;

    fn run(&self, db: &dyn Db, mir: &MIR) -> Self::Out {
        let loans = self.collect_loans(mir);
        let by_holder = loans.by_holder();
        let liveness = mir.liveness(db);
        let loans_live_in = self.project_liveness(&liveness.live_in, &by_holder);
        let loans_live_out = self.project_liveness(&liveness.live_out, &by_holder);
        MIRLoanOut {
            loans,
            loans_live_in,
            loans_live_out,
        }
    }
}

impl MIRLoanAnalysis {
    fn collect_loans(&self, mir: &MIR) -> Arena<Loan> {
        let loans = Arena::new();
        for (blk, infos) in &mir.blocks {
            for (idx, stmt) in infos.stmts.iter().enumerate() {
                match stmt {
                    Stmt::Assign { dest, rvalue } => match &rvalue.kind {
                        MIRRValueKind::Ref(place, mutability) => {
                            let loan = Loan {
                                place: place.clone(),
                                mutability: *mutability,
                                created_at: MIRStmtIndex(blk, idx),
                                holder: dest.local,
                            };
                            loans.insert(loan);
                        }
                        _ => (),
                    },
                }
            }
        }
        loans
    }

    // TODO: this over-approximates when a local is reassigned with a different loan
    fn project_liveness(
        &self,
        liveness: &BlockMap<HashSet<MIRLocalID>>,
        by_holder: &HashMap<MIRLocalID, Vec<LoanID>>,
    ) -> BlockMap<HashSet<LoanID>> {
        liveness
            .iter()
            .map(|(blk, ids)| {
                (
                    *blk,
                    ids.iter()
                        .flat_map(|local| by_holder[local].iter().copied())
                        .collect(),
                )
            })
            .collect()
    }
}

impl Arena<Loan> {
    pub fn by_holder(&self) -> LocalMap<Vec<LoanID>> {
        let mut res: LocalMap<Vec<LoanID>> = LocalMap::new();
        for (idx, loan) in self {
            res.entry(loan.holder).or_default().push(idx);
        }
        res
    }
}

impl Index<MIRStmtIndex> for MIR {
    type Output = Stmt;

    fn index(&self, index: MIRStmtIndex) -> &Self::Output {
        &self.blocks[index.0].stmts[index.1]
    }
}
