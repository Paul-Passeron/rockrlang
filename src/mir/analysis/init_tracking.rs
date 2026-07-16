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

use std::fmt;

use itertools::Itertools;

use crate::mir::{
    MIR,
    analysis::{
        MIRAnalysis,
        lattice::{BlockMap, FixedPointBlockRes, Lattice, LocalMap},
    },
    basic_block::{MIRBasicBlock, MIRTerminator, Stmt},
    operand::{
        MIRConstructorArgs, MIROperand, MIRPlace, MIRProjection, MIRRValue, MIRRValueKind,
    },
};

use super::lattice::Direction;

pub struct MIRInitAnalysis;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum InitState {
    Init,
    Maybe,
    Uninit,
}

impl Lattice for InitState {
    fn bottom() -> Self {
        Self::Uninit
    }

    fn join(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Uninit, Self::Uninit) => Self::Uninit,
            (Self::Init, Self::Init) => Self::Init,
            _ => Self::Maybe,
        }
    }
}

type FPRes = FixedPointBlockRes<LocalMap<InitState>>;

#[derive(PartialEq, Eq)]
pub struct MIRInitOut {
    pub init_in: BlockMap<LocalMap<InitState>>,
    pub init_out: BlockMap<LocalMap<InitState>>,
}

impl From<FPRes> for MIRInitOut {
    fn from(value: FPRes) -> Self {
        Self { init_in: value.block_in, init_out: value.block_out }
    }
}

impl MIRInitAnalysis {
    fn get_seed(&self, mir: &MIR) -> BlockMap<LocalMap<InitState>> {
        BlockMap::from([(
            mir.entry,
            LocalMap::from_iter(mir.parameters.iter().map(|loc| (*loc, InitState::Init))),
        )])
    }
}

impl MIRAnalysis<'_, '_> for MIRInitAnalysis {
    type Out = MIRInitOut;

    fn run(&self, _: &dyn crate::Db, mir: &MIR) -> Self::Out {
        mir.fixed_point_iter(
            Direction::Forward,
            |blk, old_in| {
                // We are computing new out
                mir.blocks[blk].init_states(old_in)
            },
            Some(self.get_seed(mir)),
            None,
        )
        .into()
    }
}

impl MIRBasicBlock {
    pub fn init_states(&self, state: &LocalMap<InitState>) -> LocalMap<InitState> {
        let mut res = state.clone();
        for stmt in &self.stmts {
            match stmt {
                Stmt::Assign { dest, rvalue } => {
                    rvalue.for_each_operand(|op| op.apply(&mut res));
                    res.insert(dest.local, InitState::Init);
                }
            }
        }
        self.terminator.init_states(&mut res);
        res
    }
}

impl MIRTerminator {
    pub fn init_states(&self, state: &mut LocalMap<InitState>) {
        match self {
            MIRTerminator::Call { arguments, dest, .. } => {
                arguments.iter().for_each(|op| op.apply(state));
                state.insert(*dest, InitState::Init);
            }
            MIRTerminator::Return { value, .. } => {
                value.iter().for_each(|op| op.apply(state))
            }
            MIRTerminator::Branch { cond: op, .. }
            | MIRTerminator::Switch { discriminant: op, .. } => op.apply(state),
            _ => (),
        }
    }
}

pub trait IterOperand {
    fn iter_each_operand(&self) -> impl Iterator<Item = &MIROperand>;

    fn for_each_operand(&self, f: impl FnMut(&MIROperand)) {
        self.iter_each_operand().for_each(f);
    }
}

impl IterOperand for MIRRValue {
    fn iter_each_operand(&self) -> impl Iterator<Item = &MIROperand> {
        match &self.kind {
            MIRRValueKind::BinOp(_, a, b) => vec![a, b],
            MIRRValueKind::Ref(p, _)
            | MIRRValueKind::AddressOf(p, _)
            | MIRRValueKind::Discriminant(p) => p.iter_each_operand().collect(),
            MIRRValueKind::Cast(op, _)
            | MIRRValueKind::Use(op)
            | MIRRValueKind::UnaryOp(_, op)
            | MIRRValueKind::Metadata(op) => vec![op],
            MIRRValueKind::SizeOf(_) => vec![],
            MIRRValueKind::Constructor { args, .. } => args.iter_each_operand().collect(),
            MIRRValueKind::StructLit { fields, .. } => fields.values().collect(),
            MIRRValueKind::Tuple(ops, _) => ops.iter().collect(),
        }
        .into_iter()
    }
}

impl IterOperand for MIRConstructorArgs {
    fn iter_each_operand(&self) -> impl Iterator<Item = &MIROperand> {
        match self {
            MIRConstructorArgs::None => vec![],
            MIRConstructorArgs::Tuple(ops) => ops.iter().collect(),
            MIRConstructorArgs::Struct(fields) => fields.values().collect(),
        }
        .into_iter()
    }
}

impl IterOperand for MIRPlace {
    fn iter_each_operand(&self) -> impl Iterator<Item = &MIROperand> {
        self.projections.iter().flat_map(|proj| match proj {
            MIRProjection::Field { .. }
            | MIRProjection::TupleField { .. }
            | MIRProjection::Downcast { .. }
            | MIRProjection::Deref => None,
            MIRProjection::Index { index } => Some(index),
        })
    }
}

impl MIROperand {
    pub fn apply(&self, state: &mut LocalMap<InitState>) {
        if let MIROperand::Move(p) = self {
            state.insert(p.local, InitState::Uninit);
        }
    }
}

impl fmt::Display for MIRInitOut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "init-in:")?;
        for (bb, init) in self.init_in.iter() {
            writeln!(
                f,
                "    bb{}: {{{}}}",
                bb.raw(),
                init.iter()
                    .map(|(local, init)| format!("_{}: {init}", local.raw(),))
                    .join(", ")
            )?;
        }
        writeln!(f, "init-out:")?;
        for (bb, init) in self.init_out.iter() {
            writeln!(
                f,
                "    bb{}: {{{}}}",
                bb.raw(),
                init.iter()
                    .map(|(local, init)| format!("_{}: {init}", local.raw(),))
                    .join(", ")
            )?;
        }
        Ok(())
    }
}

impl fmt::Display for InitState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            InitState::Init => "init",
            InitState::Maybe => "?",
            InitState::Uninit => "uninit",
        })
    }
}
