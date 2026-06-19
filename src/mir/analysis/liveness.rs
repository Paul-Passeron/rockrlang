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
    iter::once,
};

use itertools::Itertools;

use crate::{
    Db,
    mir::{
        MIR, MIRBlockID, MIRLocalID,
        analysis::MIRAnalysis,
        basic_block::{MIRBasicBlock, MIRTerminator, Stmt},
        operand::{
            MIRConstructorArgs, MIROperand, MIRPlace, MIRProjection, MIRRValue,
            MIRRValueKind,
        },
    },
};

pub struct MIRLivenessAnalysis;

pub struct MIRLivenessResult {
    pub live_in: HashMap<MIRBlockID, HashSet<MIRLocalID>>,
    pub live_out: HashMap<MIRBlockID, HashSet<MIRLocalID>>,
}

impl MIRAnalysis for MIRLivenessAnalysis {
    type Out = MIRLivenessResult;

    fn run<'db, 'mir>(&self, _db: &'db dyn Db, mir: &'mir MIR) -> Self::Out {
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
        new_live_in != self.live_in[&blk] || new_live_out != self.live_out[&blk]
    }

    pub fn run(mut self) -> MIRLivenessResult {
        loop {
            if !self
                .mir
                .blocks
                .iter()
                .fold(false, |changed, blk| changed || self.step_for(blk.0))
            {
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

impl MIRBasicBlock {
    pub fn defs(&self) -> HashSet<MIRLocalID> {
        let mut res = self.terminator.defs();
        res.extend(self.stmts.iter().map(|stmt| match stmt {
            Stmt::Assign { dest, .. } => dest.local,
        }));
        res
    }

    pub fn uses(&self) -> HashSet<MIRLocalID> {
        self.stmts
            .iter()
            .flat_map(|stmt| match stmt {
                Stmt::Assign { rvalue, .. } => rvalue.uses(),
            })
            .chain(self.terminator.uses())
            .collect()
    }
}

impl MIRRValue {
    pub fn uses(&self) -> HashSet<MIRLocalID> {
        match &self.kind {
            MIRRValueKind::Ref(p, _)
            | MIRRValueKind::AddressOf(p, _)
            | MIRRValueKind::Discriminant(p) => p.uses(),
            MIRRValueKind::BinOp(_, l, r) => l.uses().union(&r.uses()).copied().collect(),
            MIRRValueKind::Use(op)
            | MIRRValueKind::UnaryOp(_, op)
            | MIRRValueKind::Metadata(op) => op.uses(),
            MIRRValueKind::SizeOf(_) => HashSet::new(),
        }
    }
}

impl MIRTerminator {
    pub fn defs(&self) -> HashSet<MIRLocalID> {
        match self {
            MIRTerminator::Call { dest, .. } => HashSet::from([*dest]),
            _ => HashSet::new(),
        }
    }

    pub fn uses(&self) -> HashSet<MIRLocalID> {
        match self {
            MIRTerminator::Goto { .. } | MIRTerminator::Diverge => HashSet::new(),
            MIRTerminator::Call { arguments, .. } => {
                arguments.iter().flat_map(|op| op.uses()).collect()
            }
            MIRTerminator::Return { value: op, .. } => {
                op.iter().flat_map(|op| op.uses()).collect()
            }
            MIRTerminator::Switch {
                discriminant: op, ..
            }
            | MIRTerminator::Branch { cond: op, .. } => op.uses(),
        }
    }
}

impl MIROperand {
    pub fn uses(&self) -> HashSet<MIRLocalID> {
        match self {
            MIROperand::Constant(_) => HashSet::new(),
            MIROperand::Move(p) | MIROperand::Copy(p) => p.uses(),
            MIROperand::Constructor { args, .. } => match args {
                MIRConstructorArgs::None => HashSet::new(),
                MIRConstructorArgs::Tuple(ops) => {
                    ops.iter().flat_map(|op| op.uses()).collect()
                }
                MIRConstructorArgs::Struct(fields) => {
                    fields.iter().flat_map(|field| field.1.uses()).collect()
                }
            },
            MIROperand::StructLit { fields, .. } => {
                fields.iter().flat_map(|field| field.1.uses()).collect()
            }
            MIROperand::Tuple(ops, _) => ops.iter().flat_map(|op| op.uses()).collect(),
        }
    }
}

impl MIRPlace {
    pub fn uses(&self) -> HashSet<MIRLocalID> {
        once(self.local)
            .chain(self.projections.iter().flat_map(|proj| proj.uses()))
            .collect()
    }
}

impl MIRProjection {
    pub fn uses(&self) -> HashSet<MIRLocalID> {
        match self {
            MIRProjection::TupleField { .. }
            | MIRProjection::Field { .. }
            | MIRProjection::Downcast { .. }
            | MIRProjection::Deref => HashSet::new(),
            MIRProjection::Index { index } => index.uses(),
        }
    }
}
