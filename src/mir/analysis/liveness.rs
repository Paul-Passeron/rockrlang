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

use std::{collections::HashMap, fmt};

use itertools::Itertools;

use crate::{
    Db,
    common::bitset::{BitSet, BitSetIdx},
    mir::{
        MIRBlockID, MIRLocalID, Mir,
        analysis::{
            MIRAnalysis,
            lattice::{Direction, FixedPointBlockRes, Lattice, LatticeChange},
        },
    },
};

pub struct MIRLivenessAnalysis;

#[derive(PartialEq, Eq)]
pub struct MIRLivenessResult {
    pub live_in: HashMap<MIRBlockID, BitSet<MIRLocalID>>,
    pub live_out: HashMap<MIRBlockID, BitSet<MIRLocalID>>,
}

impl BitSetIdx for MIRLocalID {
    fn as_idx(&self) -> usize {
        self.into_raw()
    }

    fn from_idx(idx: usize) -> Self {
        Self::from_raw(idx)
    }
}

impl<Idx: BitSetIdx> Lattice for BitSet<Idx> {
    fn bottom() -> Self {
        unimplemented!()
    }

    fn join(&self, other: &Self) -> Self {
        let mut res = self.clone();
        res.join_assign(other);
        res
    }

    fn join_assign(&mut self, other: &Self) -> LatticeChange {
        if self.union(other) { LatticeChange::Changed } else { LatticeChange::Unchanged }
    }
}

impl From<FixedPointBlockRes<BitSet<MIRLocalID>>> for MIRLivenessResult {
    fn from(
        FixedPointBlockRes { block_in, block_out }: FixedPointBlockRes<
            BitSet<MIRLocalID>,
        >,
    ) -> Self {
        Self { live_in: block_in, live_out: block_out }
    }
}

impl MIRAnalysis<'_, '_> for MIRLivenessAnalysis {
    type Out = MIRLivenessResult;

    fn run(&self, _db: &dyn Db, mir: &Mir) -> Self::Out {
        let domain = mir.locals.len();
        mir.fixed_point_iter_bottom::<BitSet<_>>(
            Direction::Backward,
            |blk, old_out| {
                let infos = &mir.blocks[blk];
                let mut old = old_out.clone();
                old.substract(&infos.bitset_defs(mir));
                let mut res = infos.bitset_uses(mir);
                res.union(&old);
                res
            },
            None,
            move || BitSet::new(domain),
        )
        .into()
    }
}

impl fmt::Display for MIRLivenessResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "live-in:")?;
        for (bb, live) in &self.live_in {
            writeln!(
                f,
                "    bb{}: {{{}}}",
                bb.raw(),
                live.iter().map(|local| format!("_{}", local.raw())).join(", ")
            )?;
        }
        writeln!(f, "live-out:")?;
        for (bb, live) in &self.live_out {
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
