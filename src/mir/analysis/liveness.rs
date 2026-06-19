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
    mir::{
        MIR, MIRBlockID, MIRLocalID,
        analysis::{
            MIRAnalysis,
            lattice::{Direction, FixedPointIterRes},
        },
    },
};

pub struct MIRLivenessAnalysis;

pub struct MIRLivenessResult {
    pub live_in: HashMap<MIRBlockID, HashSet<MIRLocalID>>,
    pub live_out: HashMap<MIRBlockID, HashSet<MIRLocalID>>,
}

impl MIRAnalysis<'_, '_> for MIRLivenessAnalysis {
    type Out = MIRLivenessResult;

    fn run(&self, _db: &dyn Db, mir: &MIR) -> Self::Out {
        let FixedPointIterRes {
            block_in,
            block_out,
        } = mir.fixed_point_iter(
            Direction::Backward,
            |blk, old_out| Self::transfer(mir, blk, old_out),
            None,
            None,
        );

        MIRLivenessResult {
            live_in: block_in,
            live_out: block_out,
        }
    }
}

impl MIRLivenessAnalysis {
    fn transfer(
        mir: &MIR,
        blk: MIRBlockID,
        old_out: &HashSet<MIRLocalID>,
    ) -> HashSet<MIRLocalID> {
        let infos = &mir.blocks[blk];
        let uses = infos.uses();
        let defs = infos.defs();
        uses.into_iter()
            .chain(old_out.difference(&defs).copied())
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
