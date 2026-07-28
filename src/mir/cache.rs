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

use std::{collections::HashSet, sync::OnceLock};

use crate::{
    Db,
    mir::{
        MIRBlockID, Mir,
        analysis::{
            MIRAnalysis,
            init_tracking::{MIRInitAnalysis, MIRInitOut},
            lattice::BlockMap,
            liveness::{MIRLivenessAnalysis, MIRLivenessResult},
            loans::{MIRLoanAnalysis, MIRLoanOut},
        },
    },
};

pub(super) struct MIRCache {
    liveness: OnceLock<MIRLivenessResult>,
    init_tracking: OnceLock<MIRInitOut>,
    successors: OnceLock<BlockMap<HashSet<MIRBlockID>>>,
    predecessors: OnceLock<BlockMap<HashSet<MIRBlockID>>>,
    loans: OnceLock<MIRLoanOut>,
}

impl MIRCache {
    pub(super) fn empty() -> Self {
        Self {
            liveness: OnceLock::new(),
            init_tracking: OnceLock::new(),
            successors: OnceLock::new(),
            predecessors: OnceLock::new(),
            loans: OnceLock::new(),
        }
    }
}

impl Mir {
    pub fn liveness(&self, db: &dyn Db) -> &MIRLivenessResult {
        self.cache.liveness.get_or_init(|| MIRLivenessAnalysis.run(db, self))
    }

    pub fn init_tracking(&self, db: &dyn Db) -> &MIRInitOut {
        self.cache.init_tracking.get_or_init(|| MIRInitAnalysis.run(db, self))
    }

    pub fn successors(&self) -> &BlockMap<HashSet<MIRBlockID>> {
        self.cache.successors.get_or_init(|| self.compute_successors())
    }

    pub fn predecessors(&self) -> &BlockMap<HashSet<MIRBlockID>> {
        self.cache.predecessors.get_or_init(|| self.compute_predecessors())
    }

    pub fn loans(&self, db: &dyn Db) -> &MIRLoanOut {
        self.cache.loans.get_or_init(|| MIRLoanAnalysis.run(db, self))
    }
}
