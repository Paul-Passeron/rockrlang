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
    common::{
        bitset::{BitSet, BitSetIdx},
        symbols::Symbol,
    },
    mir::{
        BlockID, LocalID, Mir,
        analysis::{
            MIRAnalysis,
            lattice::{BlockMap, Direction, FixedPointBlockRes, Lattice},
        },
        basic_block::{MIRTerminator, Stmt},
        operand::{
            MIRConstructorArgs, MIROperand, MIRPlace, MIRProjection, MIRRValue,
            MIRRValueKind,
        },
    },
};

use super::lattice::LatticeChange;

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

fn record_place(db: &dyn Db, mir: &Mir, place: &MIRPlace, out: &mut HashSet<MoveKey>) {
    if !mir.locals[place.local].ty.is_copy(db) {
        out.insert(place.as_move_key());
    }
    for proj in &place.projections {
        if let MIRProjection::Index { index } = proj {
            record_operand(db, mir, index, out);
        }
    }
}

fn record_operand(db: &dyn Db, mir: &Mir, op: &MIROperand, out: &mut HashSet<MoveKey>) {
    match op {
        MIROperand::Move(p) | MIROperand::Copy(p) => record_place(db, mir, p, out),
        MIROperand::Constant(_, _) => (),
    }
}

fn compute_move_key_set(db: &dyn Db, mir: &Mir) -> HashSet<MoveKey> {
    let mut set = HashSet::new();
    for local in mir.locals.keys() {
        if !mir.locals[local].ty.is_copy(db) {
            set.insert(MoveKey { base: local, projections: Vec::new() });
        }
    }

    for (_, data) in &mir.blocks {
        for Stmt::Assign { dest, rvalue } in &data.stmts {
            record_place(db, mir, dest, &mut set);
            if let Some(p) = rvalue.inner_place() {
                record_place(db, mir, p, &mut set);
            }
            rvalue.for_each_operand(|op| record_operand(db, mir, op, &mut set));
        }
        match &data.terminator {
            MIRTerminator::Return { value: None, .. }
            | MIRTerminator::Goto { .. }
            | MIRTerminator::Diverge => (),
            MIRTerminator::Call { arguments, .. } => {
                for op in arguments {
                    record_operand(db, mir, op, &mut set);
                }
            }
            MIRTerminator::Branch { cond: value, .. }
            | MIRTerminator::Switch { discriminant: value, .. }
            | MIRTerminator::Return { value: Some(value), .. } => {
                record_operand(db, mir, value, &mut set);
            }
        }
    }

    set
}

impl BitSetIdx for MoveKeyId {
    fn as_idx(&self) -> usize {
        self.0
    }

    fn from_idx(idx: usize) -> Self {
        Self(idx)
    }
}

pub type MoveMap = HashMap<MoveKey, InitState>;

#[derive(PartialEq, Eq)]
pub struct MIRInitOut {
    pub init_in: BlockMap<MoveMap>,
    pub init_out: BlockMap<MoveMap>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MoveKeyId(usize);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct IdInitMap {
    uninit: BitSet<MoveKeyId>,
    init: BitSet<MoveKeyId>,
}

impl Lattice for IdInitMap {
    fn bottom() -> Self {
        unimplemented!("")
    }

    fn join(&self, other: &Self) -> Self {
        let mut cloned = self.clone();
        cloned.join_assign(other);
        cloned
    }

    fn join_assign(&mut self, other: &Self) -> LatticeChange {
        let mut changed = false;
        changed |= self.init.union(&other.init);
        changed |= self.uninit.union(&other.uninit);
        if changed { LatticeChange::Changed } else { LatticeChange::Unchanged }
    }
}

struct KeyInterner {
    keys: Vec<MoveKey>,
}

impl KeyInterner {
    fn build(db: &dyn Db, mir: &Mir) -> Self {
        Self { keys: compute_move_key_set(db, mir).into_iter().collect() }
    }

    fn affected(&self, target: &MoveKey) -> Vec<MoveKeyId> {
        self.keys
            .iter()
            .enumerate()
            .filter_map(|(id, k)| {
                (k == target || target.is_strict_prefix(k)).then_some(id)
            })
            .map(MoveKeyId)
            .collect()
    }
}

enum InitOp {
    Init(Vec<MoveKeyId>),
    Uninit(Vec<MoveKeyId>),
}

fn compute_block_ops(interner: &KeyInterner, mir: &Mir, blk: BlockID) -> Vec<InitOp> {
    let mut ops = Vec::new();
    let data = &mir.blocks[blk];
    for Stmt::Assign { dest, rvalue } in &data.stmts {
        dest.for_all_operands(|op| {
            if let MIROperand::Move(p) = op {
                ops.push(InitOp::Uninit(interner.affected(&p.as_move_key())));
            }
        });
        rvalue.for_all_operands(|op| {
            if let MIROperand::Move(p) = op {
                ops.push(InitOp::Uninit(interner.affected(&p.as_move_key())));
            }
        });
        ops.push(InitOp::Init(interner.affected(&dest.as_move_key())));
    }
    match &data.terminator {
        MIRTerminator::Return { value: None, .. }
        | MIRTerminator::Goto { .. }
        | MIRTerminator::Diverge => (),
        MIRTerminator::Call { arguments, dest, .. } => {
            for op in arguments {
                op.for_all_operands(|value| {
                    if let MIROperand::Move(p) = value {
                        ops.push(InitOp::Uninit(interner.affected(&p.as_move_key())));
                    }
                });
            }
            ops.push(InitOp::Init(
                interner.affected(&MoveKey { base: *dest, projections: vec![] }),
            ));
        }
        MIRTerminator::Branch { cond: value, .. }
        | MIRTerminator::Switch { discriminant: value, .. }
        | MIRTerminator::Return { value: Some(value), .. } => {
            value.for_all_operands(|value| {
                if let MIROperand::Move(p) = value {
                    ops.push(InitOp::Uninit(interner.affected(&p.as_move_key())));
                }
            });
        }
    }
    ops
}

fn apply_ops(input: &IdInitMap, ops: &[InitOp]) -> IdInitMap {
    let mut res = input.clone();
    for op in ops {
        match op {
            InitOp::Init(ids) => {
                for id in ids {
                    res.init.insert(id);
                    res.uninit.remove(id);
                }
            }
            InitOp::Uninit(ids) => {
                for id in ids {
                    res.init.remove(id);
                    res.uninit.insert(id);
                }
            }
        };
    }
    res
}

impl MIRAnalysis<'_, '_> for MIRInitAnalysis {
    type Out = MIRInitOut;

    fn run(&self, db: &dyn Db, mir: &Mir) -> Self::Out {
        let interner = KeyInterner::build(db, mir);
        let n = interner.keys.len();

        let block_ops: BlockMap<Vec<InitOp>> = mir
            .blocks
            .keys()
            .map(|blk| (blk, compute_block_ops(&interner, mir, blk)))
            .collect();

        // Entry seed: every key Uninit, then parameters marked Init.
        let mut entry = IdInitMap { init: BitSet::new(n), uninit: BitSet::new(n) };
        (0..n).for_each(|i| {
            entry.uninit.insert(&MoveKeyId(i));
        });
        for &param in &mir.parameters {
            for id in interner.affected(&MoveKey { base: param, projections: vec![] }) {
                entry.init.insert(&id);
                entry.uninit.remove(&id);
            }
        }

        let FixedPointBlockRes { block_in, block_out } = mir.fixed_point_iter_bottom(
            Direction::Forward,
            |blk, old| apply_ops(old, &block_ops[&blk]),
            Some(&entry),
            || IdInitMap { init: BitSet::new(n), uninit: BitSet::new(n) },
        );

        let to_move_map = |m: &IdInitMap| -> MoveMap {
            (0..n)
                .map(|id| {
                    let key = MoveKeyId(id);
                    let state = match (m.init.contains(&key), m.uninit.contains(&key)) {
                        (true, false) => InitState::Init,
                        (false, true) => InitState::Uninit,
                        _ => InitState::Maybe,
                    };
                    (interner.keys[id].clone(), state)
                })
                .collect()
        };
        MIRInitOut {
            init_in: block_in.iter().map(|(b, m)| (*b, to_move_map(m))).collect(),
            init_out: block_out.iter().map(|(b, m)| (*b, to_move_map(m))).collect(),
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
