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

// See https://dl.acm.org/doi/pdf/10.1145/3547647 for possible improvements to the
// representation

// Some ideas:
// - Differentiate between shared / mutable borrow deref ?

use std::{
    collections::{BTreeMap, HashSet},
    iter::once,
};

use crate::{
    Db,
    common::{bitset::BitSet, location::Span, symbols::Symbol},
    hir::Mutability,
    mir::{
        ConstructorArgs, LocalID, MIRLocalID, Mir, Operand, Projection, RValueKind,
        concrete_ty::{ConcreteEnumRef, ConcreteStructRef, ConcreteTy},
    },
    parse_tree::expr::BinaryOperator,
    resolved::{bool_id, char_id, ptr_of},
    thir::FunctionRef,
};

use super::{Constant, Place};

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum MIROperand {
    Constant(Constant, Span),
    Move(Place),
    Copy(Place),
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum MIRConstant {
    Integer { value: u128, ty: ConcreteTy },
    Bool(bool),
    CString { contents: String, null_terminated: bool },
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct MIRPlace {
    pub local: LocalID,
    pub projections: Vec<Projection>,
    pub ty: ConcreteTy,
    pub span: Span,
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum MIRProjection {
    Deref,
    Field { name: Symbol, resulting_ty: ConcreteTy },
    TupleField { index: u32, resulting_ty: ConcreteTy },
    Index { index: Operand },
    Downcast { variant: usize },
}

#[derive(Clone, PartialEq, Eq)]
pub struct MIRRValue {
    pub kind: RValueKind,
    pub ty: ConcreteTy,
    pub span: Span,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum UnaryOperator {
    Neg,  // `-` in -x
    LNot, // `!` in !x
}

#[derive(Clone, PartialEq, Eq)]
pub enum MIRCallee {
    Direct(FunctionRef),
}

#[derive(Clone, PartialEq, Eq)]
pub enum MIRRValueKind {
    Use(Operand),
    Ref(Place, Mutability),
    AddressOf(Place, Mutability),
    BinOp(BinaryOperator, Operand, Operand),
    UnaryOp(UnaryOperator, Operand),
    Discriminant(Place),
    Metadata(Operand),
    SizeOf(ConcreteTy),
    Constructor {
        enum_ref: ConcreteEnumRef,
        idx: usize,
        args: ConstructorArgs,
        span: Span,
    },
    StructLit {
        struct_ref: ConcreteStructRef,
        fields: BTreeMap<Symbol, Operand>,
        span: Span,
    },
    Tuple(Vec<Operand>, Span),
    Cast(Operand, ConcreteTy),
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum MIRConstructorArgs {
    None,
    Tuple(Vec<Operand>),
    Struct(BTreeMap<Symbol, Operand>),
}

impl Constant {
    pub fn ty(&self, db: &dyn Db) -> ConcreteTy {
        match self {
            Self::Integer { ty, .. } => *ty,
            Self::Bool(_) => bool_id(db),
            Self::CString { .. } => ptr_of(db, char_id(db), false),
        }
    }
}

impl Operand {
    pub fn ty(&self, db: &dyn Db) -> ConcreteTy {
        match self {
            Self::Constant(mirconstant, _) => mirconstant.ty(db),
            Self::Move(mirplace) | Self::Copy(mirplace) => mirplace.ty,
        }
    }
}

impl Place {
    pub fn into_move(self) -> Operand {
        Operand::Move(self)
    }

    pub fn into_copy(self) -> Operand {
        Operand::Copy(self)
    }
}

impl From<FunctionRef> for MIRCallee {
    fn from(value: FunctionRef) -> Self {
        Self::Direct(value)
    }
}

impl Constant {
    pub fn int(value: u128, ty: ConcreteTy) -> Self {
        Self::Integer { value, ty }
    }
}

impl MIRRValue {
    pub fn uses(&self) -> HashSet<MIRLocalID> {
        match &self.kind {
            MIRRValueKind::Ref(p, _)
            | MIRRValueKind::AddressOf(p, _)
            | MIRRValueKind::Discriminant(p) => p.uses(),
            MIRRValueKind::BinOp(_, l, r) => l.uses().union(&r.uses()).copied().collect(),
            MIRRValueKind::Cast(op, _)
            | MIRRValueKind::Use(op)
            | MIRRValueKind::UnaryOp(_, op)
            | MIRRValueKind::Metadata(op) => op.uses(),
            MIRRValueKind::SizeOf(_) => HashSet::new(),
            MIRRValueKind::Constructor { args, .. } => args.uses(),
            MIRRValueKind::StructLit { fields, .. } => {
                fields.values().flat_map(MIROperand::uses).collect()
            }
            MIRRValueKind::Tuple(ops, _) => {
                ops.iter().flat_map(MIROperand::uses).collect()
            }
        }
    }

    pub fn bitset_uses(&self, mir: &Mir) -> BitSet<MIRLocalID> {
        let domain = mir.locals.len();
        match &self.kind {
            MIRRValueKind::Ref(p, _)
            | MIRRValueKind::AddressOf(p, _)
            | MIRRValueKind::Discriminant(p) => p.bitset_uses(mir),
            MIRRValueKind::BinOp(_, l, r) => {
                let mut res = l.bitset_uses(mir);
                res.union(&r.bitset_uses(mir));
                res
            }
            MIRRValueKind::Cast(op, _)
            | MIRRValueKind::Use(op)
            | MIRRValueKind::UnaryOp(_, op)
            | MIRRValueKind::Metadata(op) => op.bitset_uses(mir),
            MIRRValueKind::SizeOf(_) => BitSet::new(domain),
            MIRRValueKind::Constructor { args, .. } => args.bitset_uses(mir),
            MIRRValueKind::StructLit { fields, .. } => {
                let mut res = BitSet::new(domain);
                for field in fields.values() {
                    res.union(&field.bitset_uses(mir));
                }
                res
            }
            MIRRValueKind::Tuple(ops, _) => {
                let mut res = BitSet::new(domain);
                for field in ops {
                    res.union(&field.bitset_uses(mir));
                }
                res
            }
        }
    }
}

impl MIRConstructorArgs {
    pub fn bitset_uses(&self, mir: &Mir) -> BitSet<MIRLocalID> {
        let domain = mir.locals.len();
        let mut res = BitSet::new(domain);
        match self {
            Self::None => (),
            Self::Tuple(ops) => {
                for op in ops {
                    res.union(&op.bitset_uses(mir));
                }
            }
            Self::Struct(fields) => {
                for op in fields.values() {
                    res.union(&op.bitset_uses(mir));
                }
            }
        }
        res
    }

    pub fn uses(&self) -> HashSet<MIRLocalID> {
        match self {
            Self::None => HashSet::new(),
            Self::Tuple(ops) => ops.iter().flat_map(MIROperand::uses).collect(),
            Self::Struct(fields) => fields.values().flat_map(MIROperand::uses).collect(),
        }
    }
}

impl MIROperand {
    pub fn bitset_uses(&self, mir: &Mir) -> BitSet<MIRLocalID> {
        let domain = mir.locals.len();
        match self {
            Self::Constant(_, _) => BitSet::new(domain),
            Self::Move(p) | Self::Copy(p) => p.bitset_uses(mir),
        }
    }

    pub fn uses(&self) -> HashSet<MIRLocalID> {
        match self {
            Self::Constant(_, _) => HashSet::new(),
            Self::Move(p) | Self::Copy(p) => p.uses(),
        }
    }
}

impl MIRPlace {
    pub fn bitset_uses(&self, mir: &Mir) -> BitSet<MIRLocalID> {
        let mut res = BitSet::new(mir.locals.len());
        res.insert(&self.local);
        self.projections.iter().for_each(|proj| {
            res.union(&proj.bitset_uses(mir));
        });
        res
    }

    pub fn uses(&self) -> HashSet<MIRLocalID> {
        once(self.local)
            .chain(self.projections.iter().flat_map(MIRProjection::uses))
            .collect()
    }
}

impl MIRProjection {
    pub fn bitset_uses(&self, mir: &Mir) -> BitSet<MIRLocalID> {
        let domain = mir.locals.len();
        match self {
            Self::TupleField { .. }
            | Self::Field { .. }
            | Self::Downcast { .. }
            | Self::Deref => BitSet::new(domain),
            Self::Index { index } => index.bitset_uses(mir),
        }
    }

    pub fn uses(&self) -> HashSet<MIRLocalID> {
        match self {
            Self::TupleField { .. }
            | Self::Field { .. }
            | Self::Downcast { .. }
            | Self::Deref => HashSet::new(),
            Self::Index { index } => index.uses(),
        }
    }
}

impl MIROperand {
    pub fn span(&self) -> Span {
        match self {
            Self::Constant(_, span) => *span,
            Self::Move(p) | Self::Copy(p) => p.span,
        }
    }
}
