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

use crate::{
    Db,
    common::symbols::Symbol,
    mir::{
        BlockID, LocalID, Mir,
        analysis::{
            MIRAnalysis,
            lattice::{BlockMap, FixedPointBlockRes, Lattice},
        },
        basic_block::{MIRTerminator, Stmt},
        operand::{
            MIRConstructorArgs, MIROperand, MIRPlace, MIRProjection, MIRRValue,
            MIRRValueKind,
        },
    },
    resolved::TypeRef,
};

use super::lattice::Direction;

pub struct MIRInitAnalysis;

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
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
    pub base: LocalID,
    pub projections: Vec<TrackedProjection>,
}

impl MIRProjection {
    fn as_tracked_projection(&self) -> Option<TrackedProjection> {
        match self {
            Self::Deref => Some(TrackedProjection::Deref),
            Self::Field { name, .. } => Some(TrackedProjection::StructField(*name)),
            Self::TupleField { index, .. } => Some(TrackedProjection::TupleField(*index)),
            Self::Index { .. } => None,
            Self::Downcast { variant } => Some(TrackedProjection::Downcast(*variant)),
        }
    }
}

impl MIROperand {
    fn for_all_operands(&self, mut f: impl FnMut(&Self)) {
        self.for_all_operands_dyn(&mut f);
    }

    fn for_all_operands_dyn(&self, f: &mut dyn FnMut(&Self)) {
        f(self);
        match self {
            Self::Constant(_, _) => (),
            Self::Move(mirplace) | Self::Copy(mirplace) => {
                mirplace.for_all_operands_dyn(f);
            }
        }
    }
}

impl MIRPlace {
    pub fn as_move_key(&self) -> MoveKey {
        MoveKey {
            base: self.local,
            projections: self
                .projections
                .iter()
                .map_while(MIRProjection::as_tracked_projection)
                .collect(),
        }
    }

    fn for_all_operands(&self, mut f: impl FnMut(&MIROperand)) {
        self.for_all_operands_dyn(&mut f);
    }

    fn for_all_operands_dyn(&self, f: &mut dyn FnMut(&MIROperand)) {
        self.projections.iter().for_each(|proj| {
            if let MIRProjection::Index { index } = proj {
                index.for_all_operands_dyn(f);
            }
        });
    }
}

impl MIRRValue {
    fn for_all_operands(&self, mut f: impl FnMut(&MIROperand)) {
        self.for_each_operand(|op| op.for_all_operands_dyn(&mut f));
    }
}

fn collect_leaves(
    db: &dyn Db,
    base: LocalID,
    prefix: &mut Vec<TrackedProjection>,
    ty: TypeRef,
    out: &mut HashSet<MoveKey>,
) {
    if let Some(sr) = ty.as_struct_ref(db) {
        let fields = sr.def.field_names(db);
        if fields.is_empty() {
            out.insert(MoveKey { base, projections: prefix.clone() });
            return;
        }
        for name in fields.iter() {
            let fty = sr.typeof_field(db, *name).unwrap_or(TypeRef::Unknown);
            prefix.push(TrackedProjection::StructField(*name));
            collect_leaves(db, base, prefix, fty, out);
            prefix.pop();
        }
    } else if let Some(elems) = ty.as_tuple_ref(db) {
        if elems.is_empty() {
            out.insert(MoveKey { base, projections: prefix.clone() });
            return;
        }
        for (i, ety) in elems.iter().enumerate() {
            prefix.push(TrackedProjection::TupleField(i as u32));
            collect_leaves(db, base, prefix, *ety, out);
            prefix.pop();
        }
    } else {
        out.insert(MoveKey { base, projections: prefix.clone() });
    }
}

fn compute_move_key_set(db: &dyn Db, mir: &Mir) -> HashSet<MoveKey> {
    let mut set = HashSet::new();
    for local in mir.locals.keys() {
        let ty = mir.locals[local].ty;
        collect_leaves(db, local, &mut Vec::new(), ty, &mut set);
    }
    set
}

pub type MoveMap = HashMap<MoveKey, InitState>;
type GranularFPRes = FixedPointBlockRes<MoveMap>;

#[derive(PartialEq, Eq)]
pub struct MIRInitOut {
    pub init_in: BlockMap<MoveMap>,
    pub init_out: BlockMap<MoveMap>,
}

impl From<GranularFPRes> for MIRInitOut {
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

pub fn init_key(key: &MoveKey, map: &mut MoveMap) {
    map.iter_mut()
        .filter_map(|(k, state)| (k == key || key.is_strict_prefix(k)).then_some(state))
        .for_each(|state| *state = InitState::Init);
}

pub fn uninit_key(key: &MoveKey, map: &mut MoveMap) {
    map.iter_mut()
        .filter_map(|(k, state)| (k == key || key.is_strict_prefix(k)).then_some(state))
        .for_each(|state| *state = InitState::Uninit);
}

impl MIRInitAnalysis {
    fn get_seed(db: &dyn Db, mir: &Mir) -> BlockMap<MoveMap> {
        let move_keys = compute_move_key_set(db, mir);
        let mut entry = MoveMap::from_iter(
            move_keys.into_iter().map(|key| (key, InitState::bottom())),
        );
        mir.parameters.iter().copied().for_each(|idx| {
            init_key(&MoveKey { base: idx, projections: vec![] }, &mut entry);
        });
        BlockMap::from([(mir.entry, entry)])
    }

    fn granular_transfer(mir: &Mir, blk: BlockID, map: &MoveMap) -> MoveMap {
        let mut res = map.clone();
        let data = &mir.blocks[blk];
        for Stmt::Assign { dest, rvalue } in &data.stmts {
            dest.for_all_operands(|op| {
                if let MIROperand::Move(p) = op {
                    {
                        let map: &mut MoveMap = &mut res;
                        uninit_key(&p.as_move_key(), map);
                    };
                }
            });
            rvalue.for_all_operands(|op| {
                if let MIROperand::Move(p) = op {
                    {
                        let map: &mut MoveMap = &mut res;
                        uninit_key(&p.as_move_key(), map);
                    };
                }
            });
            {
                let map: &mut MoveMap = &mut res;
                init_key(&dest.as_move_key(), map);
            };
        }
        match &data.terminator {
            MIRTerminator::Return { value: None, .. }
            | MIRTerminator::Goto { .. }
            | MIRTerminator::Diverge => (),
            MIRTerminator::Call { arguments, dest, .. } => {
                for op in arguments {
                    op.for_all_operands(|value| {
                        if let MIROperand::Move(p) = value {
                            {
                                let map: &mut MoveMap = &mut res;
                                uninit_key(&p.as_move_key(), map);
                            };
                        }
                    });
                }
                {
                    let place: &MIRPlace = &MIRPlace {
                        local: *dest,
                        projections: vec![],
                        ty: TypeRef::Unknown,
                        span: mir.locals[*dest].span,
                    };
                    let map: &mut MoveMap = &mut res;
                    init_key(&place.as_move_key(), map);
                };
            }
            MIRTerminator::Branch { cond: value, .. }
            | MIRTerminator::Switch { discriminant: value, .. }
            | MIRTerminator::Return { value: Some(value), .. } => {
                value.for_all_operands(|value| {
                    if let MIROperand::Move(p) = value {
                        {
                            let map: &mut MoveMap = &mut res;
                            uninit_key(&p.as_move_key(), map);
                        };
                    }
                });
            }
        }
        res
    }
}

impl MIRAnalysis<'_, '_> for MIRInitAnalysis {
    type Out = MIRInitOut;

    fn run(&self, db: &'_ dyn Db, mir: &'_ Mir) -> Self::Out {
        mir.fixed_point_iter(
            Direction::Forward,
            |blk, old_in| Self::granular_transfer(mir, blk, old_in),
            Some(&Self::get_seed(db, mir)),
            None,
        )
        .into()
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
            Self::None => vec![],
            Self::Tuple(ops) => ops.iter().collect(),
            Self::Struct(fields) => fields.values().collect(),
        }
        .into_iter()
    }
}

impl IterOperand for MIRPlace {
    fn iter_each_operand(&self) -> impl Iterator<Item = &MIROperand> {
        self.projections.iter().filter_map(|proj| match proj {
            MIRProjection::Field { .. }
            | MIRProjection::TupleField { .. }
            | MIRProjection::Downcast { .. }
            | MIRProjection::Deref => None,
            MIRProjection::Index { index } => Some(index),
        })
    }
}

impl fmt::Display for InitState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Init => "init",
            Self::Maybe => "?",
            Self::Uninit => "uninit",
        })
    }
}
