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

use std::collections::{HashMap, HashSet};

use crate::{
    Db,
    mir::{MIR, MIRBlockID, MIRLocalID, analysis::MIRAnalysis},
};

pub struct MIRLivenessAnalysis;

pub struct MIRLivenessResult {
    pub live_in: HashMap<MIRBlockID, HashSet<MIRLocalID>>,
    pub live_out: HashMap<MIRBlockID, HashSet<MIRLocalID>>,
}

impl MIRAnalysis for MIRLivenessAnalysis {
    type Out = MIRLivenessResult;

    fn run<'db, 'mir>(&self, db: &'db dyn Db, mir: &'mir MIR) -> Self::Out {
        LivCtx::new(db, mir).run()
    }
}

struct LivCtx<'a> {
    db: &'a dyn Db,
    mir: &'a MIR,
    live_in: HashMap<MIRBlockID, HashSet<MIRLocalID>>,
    live_out: HashMap<MIRBlockID, HashSet<MIRLocalID>>,
}

impl<'a> LivCtx<'a> {
    pub fn new(db: &'a dyn Db, mir: &'a MIR) -> Self {
        Self {
            db,
            mir,
            live_in: Self::get_start_live(mir),
            live_out: Self::get_start_live(mir),
        }
    }

    pub fn run(mut self) -> MIRLivenessResult {
        self.finalize()
    }

    fn finalize(self) -> MIRLivenessResult {
        MIRLivenessResult {
            live_in: self.live_in,
            live_out: self.live_out,
        }
    }

    fn get_start_live(mir: &MIR) -> HashMap<MIRBlockID, HashSet<MIRLocalID>> {
        mir.blocks
            .iter()
            .map(|(id, _)| (id, HashSet::new()))
            .collect()
    }
}
