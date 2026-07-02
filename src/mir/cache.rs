use std::{collections::HashSet, sync::OnceLock};

use crate::{
    Db,
    mir::{
        MIR, MIRBlockID,
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
    pub fn empty() -> Self {
        Self {
            liveness: OnceLock::new(),
            init_tracking: OnceLock::new(),
            successors: OnceLock::new(),
            predecessors: OnceLock::new(),
            loans: OnceLock::new(),
        }
    }
}

impl MIR {
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
