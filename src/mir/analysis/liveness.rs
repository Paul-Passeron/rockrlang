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
    mir::{MIR, MIRBlockID, MIRLocalID, analysis::MIRAnalysis},
};

pub struct MIRLivenessAnalysis;

pub struct MIRLivenessResult {
    pub live_in: HashMap<MIRBlockID, HashSet<MIRLocalID>>,
    pub live_out: HashMap<MIRBlockID, HashSet<MIRLocalID>>,
}

impl MIRAnalysis<'_, '_> for MIRLivenessAnalysis {
    type Out = MIRLivenessResult;

    fn run(&self, _db: &dyn Db, mir: & MIR) -> Self::Out {
        LivCtx::new(mir).run()
    }
}

struct LivCtx<'a> {
    mir: &'a MIR,
    live_in: HashMap<MIRBlockID, HashSet<MIRLocalID>>,
    live_out: HashMap<MIRBlockID, HashSet<MIRLocalID>>,

    successors: HashMap<MIRBlockID, HashSet<MIRBlockID>>,
}

impl<'a> LivCtx<'a> {
    pub fn new(mir: &'a MIR) -> Self {
        Self {
            mir,
            live_in: Self::get_start_live(mir),
            live_out: Self::get_start_live(mir),
            successors: mir.compute_successors(),
        }
    }

    pub fn step_for(&mut self, blk: MIRBlockID) -> bool {
        let infos = &self.mir.blocks[blk];
        let mut new_live_in: HashSet<MIRLocalID> = infos
            .uses()
            .into_iter()
            .chain(self.live_out[&blk].difference(&infos.defs()).copied())
            .collect();
        let mut new_live_out: HashSet<_> = self.successors[&blk]
            .iter()
            .flat_map(|s| &self.live_in[s])
            .copied()
            .collect();
        std::mem::swap(self.live_in.get_mut(&blk).unwrap(), &mut new_live_in);
        std::mem::swap(self.live_out.get_mut(&blk).unwrap(), &mut new_live_out);
        new_live_in == self.live_in[&blk] && new_live_out == self.live_out[&blk]
    }

    pub fn run(mut self) -> MIRLivenessResult {
        loop {
            if self.mir.blocks.iter().all(|blk| !self.step_for(blk.0)) {
                break;
            }
        }
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

impl fmt::Display for MIRLivenessResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "live-in:")?;
        for (bb, live) in self.live_in.iter() {
            writeln!(
                f,
                "    bb{}: {{{}}}",
                bb.raw(),
                live.iter()
                    .map(|local| format!("_{}", local.raw()))
                    .join(", ")
            )?;
        }
        writeln!(f, "live-out:")?;
        for (bb, live) in self.live_out.iter() {
            writeln!(
                f,
                "    bb{}: {{{}}}",
                bb.raw(),
                live.iter()
                    .map(|local| format!("_{}", local.raw()))
                    .join(", ")
            )?;
        }
        Ok(())
    }
}
