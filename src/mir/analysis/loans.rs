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
    fmt,
};

use itertools::Itertools;

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
        display::{StringWriter, fmt_place},
        operand::{MIRPlace, MIRRValueKind},
    },
};

pub struct MIRLoanAnalysis;

pub type LoanID = Idx<Loan>;

pub struct MIRLoanOut {
    pub loans: Arena<Loan>,
    pub indices: HashMap<MIRStmtIndex, LoanID>,
    pub loans_live_in: BlockMap<HashSet<LoanID>>,
    pub loans_live_out: BlockMap<HashSet<LoanID>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MIRStmtIndex(pub MIRBlockID, pub usize);

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
        let indices = loans
            .iter()
            .map(|(id, loan)| (loan.created_at, id))
            .collect();
        MIRLoanOut {
            loans,
            indices,
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
                    Stmt::Assign { dest, rvalue } => {
                        if let MIRRValueKind::Ref(place, mutability)
                        | MIRRValueKind::AddressOf(place, mutability) = &rvalue.kind
                        {
                            let loan = Loan {
                                place: place.clone(),
                                mutability: *mutability,
                                created_at: MIRStmtIndex(blk, idx),
                                holder: dest.local,
                            };
                            loans.insert(loan);
                        }
                    }
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
                        .flat_map(|local| Some(by_holder.get(local)?.iter().copied()))
                        .flatten()
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

struct MIRLoanOutDisplay<'a, 'b> {
    db: &'b dyn Db,
    out: &'a MIRLoanOut,
}

impl MIRLoanOut {
    pub fn display(&self, db: &dyn Db) -> impl fmt::Display {
        MIRLoanOutDisplay { db, out: self }
    }
}

impl fmt::Display for MIRLoanOutDisplay<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "loans:")?;
        for (id, loan) in &self.out.loans {
            let mut w = StringWriter(String::new());
            fmt_place(&mut w, self.db, &loan.place)?;

            writeln!(
                f,
                "    L{}: {} {} = {}_0..{} (holder: _{})",
                id.raw(),
                if loan.mutability.is_mut() { "&mut " } else { "&" },
                w.0,
                loan.created_at.0.raw(),
                loan.created_at.1,
                loan.holder.raw(),
            )?;
        }

        writeln!(f, "loans-live-in:")?;
        for (blk, ids) in &self.out.loans_live_in {
            writeln!(
                f,
                "    bb{}: {{{}}}",
                blk.raw(),
                ids.iter().map(|id| format!("L{}", id.raw())).join(", ")
            )?;
        }

        writeln!(f, "loans-live-out:")?;
        for (blk, ids) in &self.out.loans_live_out {
            writeln!(
                f,
                "    bb{}: {{{}}}",
                blk.raw(),
                ids.iter().map(|id| format!("L{}", id.raw())).join(", ")
            )?;
        }

        Ok(())
    }
}
