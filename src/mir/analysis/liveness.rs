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
            lattice::{Direction, FixedPointBlockRes},
        },
    },
};

pub struct MIRLivenessAnalysis;

#[derive(PartialEq, Eq)]
pub struct MIRLivenessResult {
    pub live_in: HashMap<MIRBlockID, HashSet<MIRLocalID>>,
    pub live_out: HashMap<MIRBlockID, HashSet<MIRLocalID>>,
}

impl From<FixedPointBlockRes<HashSet<MIRLocalID>>> for MIRLivenessResult {
    fn from(
        FixedPointBlockRes { block_in, block_out }: FixedPointBlockRes<
            HashSet<MIRLocalID>,
        >,
    ) -> Self {
        Self { live_in: block_in, live_out: block_out }
    }
}

impl MIRAnalysis<'_, '_> for MIRLivenessAnalysis {
    type Out = MIRLivenessResult;

    fn run(&self, _db: &dyn Db, mir: &MIR) -> Self::Out {
        mir.fixed_point_iter::<HashSet<_>>(
            Direction::Backward,
            |blk, old_out| {
                let infos = &mir.blocks[blk];
                infos
                    .uses()
                    .into_iter()
                    .chain(old_out.difference(&infos.defs()).copied())
                    .collect()
            },
            None,
            None,
        )
        .into()
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
                live.iter().map(|local| format!("_{}", local.raw())).join(", ")
            )?;
        }
        writeln!(f, "live-out:")?;
        for (bb, live) in self.live_out.iter() {
            writeln!(
                f,
                "    bb{}: {{{}}}",
                bb.raw(),
                live.iter().map(|local| format!("_{}", local.raw())).join(", ")
            )?;
        }
        Ok(())
    }
}
