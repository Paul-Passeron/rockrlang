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
    common::symbols::Symbol,
    mir::{
        BlockID, LocalID, MIR,
        analysis::{
            MIRAnalysis,
            lattice::{BlockMap, FixedPointBlockRes, Lattice, LocalMap},
            loans::MIRStmtIndex,
        },
        basic_block::{MIRBasicBlock, MIRTerminator, Stmt},
        operand::{
            MIRConstructorArgs, MIROperand, MIRPlace, MIRProjection, MIRRValue,
            MIRRValueKind,
        },
    },
    parse_tree::expr::BinaryOperator::Eq,
    resolved::TypeRef,
};

use super::lattice::Direction;

pub struct MIRInitAnalysis;
pub struct MIRGranularInitAnalysis;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackedProjection {
    StructField(Symbol),
    TupleField(u32),
    Deref,
    Downcast(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MoveKey {
    base: LocalID,
    projections: Vec<TrackedProjection>,
}

impl MIRProjection {
    fn as_tracked_projection(&self) -> Option<TrackedProjection> {
        match self {
            MIRProjection::Deref => Some(TrackedProjection::Deref),
            MIRProjection::Field { name, .. } => {
                Some(TrackedProjection::StructField(*name))
            }
            MIRProjection::TupleField { index, .. } => {
                Some(TrackedProjection::TupleField(*index))
            }
            MIRProjection::Index { .. } => None,
            MIRProjection::Downcast { variant } => {
                Some(TrackedProjection::Downcast(*variant))
            }
        }
    }
}

impl MIRPlace {
    fn as_move_key(&self) -> MoveKey {
        MoveKey {
            base: self.local,
            projections: self
                .projections
                .iter()
                .map_while(MIRProjection::as_tracked_projection)
                .collect(),
        }
    }
}

impl MIROperand {
    fn for_all_operands(&self, mut f: impl FnMut(&MIROperand)) {
        fn aux(base: &MIROperand, f: &mut impl FnMut(&MIROperand)) {
            f(base);
            match base {
                MIROperand::Constant(_, _) => (),
                MIROperand::Move(mirplace) | MIROperand::Copy(mirplace) => {
                    mirplace.for_all_operands(f)
                }
            }
        }
        aux(self, &mut f);
    }
}

impl MIRPlace {
    fn for_all_operands(&self, mut f: impl FnMut(&MIROperand)) {
        self.projections.iter().for_each(|proj| match proj {
            MIRProjection::Index { index } => index.for_all_operands(&mut f),
            _ => (),
        });
    }
}

impl MIRRValue {
    fn for_all_operands(&self, mut f: impl FnMut(&MIROperand)) {
        self.for_each_operand(|op| op.for_all_operands(&mut f));
    }
}

#[allow(unused)]
fn compute_move_key_set(db: &dyn Db, mir: &MIR) -> HashSet<MoveKey> {
    let mut set = HashSet::new();
    mir.locals.keys().for_each(|idx| {
        set.insert(MoveKey { base: idx, projections: vec![] });
    });
    mir.blocks.iter().flat_map(|(_, block_data)| block_data.stmts.iter()).for_each(
        |stmt| match stmt {
            Stmt::Assign { dest, rvalue } => {
                set.insert(dest.as_move_key());
                dest.for_all_operands(|op| match op {
                    MIROperand::Copy(dest) | MIROperand::Move(dest) => {
                        set.insert(dest.as_move_key());
                    }
                    _ => (),
                });
                rvalue.for_all_operands(|op| match op {
                    MIROperand::Copy(dest) | MIROperand::Move(dest) => {
                        set.insert(dest.as_move_key());
                    }
                    _ => (),
                });
            }
        },
    );
    set
}

type MoveMap = HashMap<MoveKey, InitState>;
type GranularFPRes = FixedPointBlockRes<MoveMap>;

#[derive(PartialEq, Eq)]
pub struct MIRGranularInitOut {
    pub init_in: BlockMap<MoveMap>,
    pub init_out: BlockMap<MoveMap>,
}

impl From<GranularFPRes> for MIRGranularInitOut {
    fn from(value: GranularFPRes) -> Self {
        Self { init_in: value.block_in, init_out: value.block_out }
    }
}

impl MoveKey {
    pub fn is_strict_prefix(&self, other: &Self) -> bool {
        if self.base != other.base {
            return false;
        }
        if self.projections.len() >= other.projections.len() {
            return false;
        }
        self.projections.iter().zip(&other.projections).all(|(pa, pb)| pa == pb)
    }
}

fn init_key(key: &MoveKey, map: &mut MoveMap) {
    map.iter_mut()
        .filter_map(|(k, state)| (k == key || key.is_strict_prefix(k)).then_some(state))
        .for_each(|state| *state = InitState::Init);
}

impl MIRGranularInitAnalysis {
    fn get_seed(&self, db: &dyn Db, mir: &MIR) -> BlockMap<MoveMap> {
        let move_keys = compute_move_key_set(db, mir);
        let move_map = MoveMap::from_iter(
            move_keys.into_iter().map(|key| (key, InitState::bottom())),
        );
        BlockMap::from_iter(mir.blocks.keys().map(|blk| {
            let mut map = move_map.clone();
            if blk == mir.entry {
                mir.parameters.iter().copied().for_each(|idx| {
                    init_key(&MoveKey { base: idx, projections: vec![] }, &mut map);
                });
            }
            (blk, map)
        }))
    }

    fn do_init(place: &MIRPlace, map: &mut MoveMap) {
        let d = place.as_move_key();
        init_key(&d, map);
        map.iter_mut().filter(|(k, _)| k.is_strict_prefix(&d)).for_each(|(_, state)| {
            if *state == InitState::Uninit {
                *state = InitState::Maybe
            }
        });
    }

    fn do_move(place: &MIRPlace, map: &mut MoveMap) {
        let d = place.as_move_key();
        map.iter_mut()
            .filter(|(k, _)| *k == &d || d.is_strict_prefix(k))
            .for_each(|(_, state)| *state = InitState::Uninit);
        map.iter_mut().filter(|(k, _)| k.is_strict_prefix(&d)).for_each(|(_, state)| {
            if *state == InitState::Init {
                *state = InitState::Maybe
            }
        });
    }

    fn granular_transfer(&self, mir: &MIR, blk: BlockID, map: &MoveMap) -> MoveMap {
        let mut res = map.clone();
        let data = &mir.blocks[blk];
        for Stmt::Assign { dest, rvalue } in &data.stmts {
            dest.for_all_operands(|op| {
                if let MIROperand::Move(p) = op {
                    Self::do_move(p, &mut res);
                }
            });
            rvalue.for_all_operands(|op| {
                if let MIROperand::Move(p) = op {
                    Self::do_move(p, &mut res);
                }
            });
            Self::do_init(dest, &mut res);
        }
        match &data.terminator {
            MIRTerminator::Return { value: None, .. }
            | MIRTerminator::Goto { .. }
            | MIRTerminator::Diverge => (),
            MIRTerminator::Call { arguments, dest, .. } => {
                arguments.iter().for_each(|op| {
                    op.for_all_operands(|value| {
                        if let MIROperand::Move(p) = value {
                            Self::do_move(p, &mut res);
                        }
                    })
                });
                Self::do_init(
                    &MIRPlace {
                        local: *dest,
                        projections: vec![],
                        ty: TypeRef::Unknown,
                        span: mir.locals[*dest].span,
                    },
                    &mut res,
                );
            }
            MIRTerminator::Branch { cond: value, .. }
            | MIRTerminator::Switch { discriminant: value, .. }
            | MIRTerminator::Return { value: Some(value), .. } => {
                value.for_all_operands(|value| {
                    if let MIROperand::Move(p) = value {
                        Self::do_move(p, &mut res);
                    }
                });
            }
        }
        res
    }
}

impl MIRAnalysis<'_, '_> for MIRGranularInitAnalysis {
    type Out = MIRGranularInitOut;

    fn run(&self, db: &'_ dyn Db, mir: &'_ MIR) -> Self::Out {
        mir.fixed_point_iter(
            Direction::Forward,
            |blk, old_in| self.granular_transfer(mir, blk, old_in),
            Some(self.get_seed(db, mir)),
            None,
        )
        .into()
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
