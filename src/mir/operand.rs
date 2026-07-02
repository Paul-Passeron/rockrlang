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
    common::{location::Span, symbols::Symbol},
    hir::Mutability,
    mir::{
        ConstructorArgs, LocalID, MIRLocalID, Operand, Projection, RValueKind,
    },
    parse_tree::expr::BinaryOperator,
    ril::{TypeRef, bool_id, char_id, ptr_of},
    thir::{EnumRef, FunctionRef, StructRef},
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
    Integer { value: i128, ty: TypeRef },
    Bool(bool),
    CString { contents: String, null_terminated: bool },
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct MIRPlace {
    pub local: LocalID,
    pub projections: Vec<Projection>,
    pub ty: TypeRef,
    pub span: Span,
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum MIRProjection {
    Deref,
    Field { name: Symbol, resulting_ty: TypeRef },
    TupleField { index: u32, resulting_ty: TypeRef },
    Index { index: Operand },
    Downcast { variant: usize },
}

#[derive(Clone, PartialEq, Eq)]
pub struct MIRRValue {
    pub kind: RValueKind,
    pub ty: TypeRef,
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
    SizeOf(TypeRef),
    Constructor {
        enum_ref: EnumRef,
        idx: usize,
        args: ConstructorArgs,
        span: Span,
    },
    StructLit {
        struct_ref: StructRef,
        fields: BTreeMap<Symbol, Operand>,
        span: Span,
    },
    Tuple(Vec<Operand>, Span),
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum MIRConstructorArgs {
    None,
    Tuple(Vec<Operand>),
    Struct(BTreeMap<Symbol, Operand>),
}

impl Constant {
    pub fn ty(&self, db: &dyn Db) -> TypeRef {
        match self {
            MIRConstant::Integer { ty, .. } => *ty,
            MIRConstant::Bool(_) => bool_id(db).into(),
            MIRConstant::CString { .. } => {
                ptr_of(db, char_id(db).into(), false).into()
            }
        }
    }
}

impl Operand {
    pub fn ty(&self, db: &dyn Db) -> TypeRef {
        match self {
            MIROperand::Constant(mirconstant, _) => mirconstant.ty(db),
            MIROperand::Move(mirplace) | MIROperand::Copy(mirplace) => {
                mirplace.ty
            }
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
    pub fn int(value: i128, ty: TypeRef) -> Self {
        Self::Integer { value, ty }
    }
}

impl MIRRValue {
    pub fn uses(&self) -> HashSet<MIRLocalID> {
        match &self.kind {
            MIRRValueKind::Ref(p, _)
            | MIRRValueKind::AddressOf(p, _)
            | MIRRValueKind::Discriminant(p) => p.uses(),
            MIRRValueKind::BinOp(_, l, r) => {
                l.uses().union(&r.uses()).copied().collect()
            }
            MIRRValueKind::Use(op)
            | MIRRValueKind::UnaryOp(_, op)
            | MIRRValueKind::Metadata(op) => op.uses(),
            MIRRValueKind::SizeOf(_) => HashSet::new(),
            MIRRValueKind::Constructor { args, .. } => args.uses(),
            MIRRValueKind::StructLit { fields, .. } => {
                fields.values().flat_map(|op| op.uses()).collect()
            }
            MIRRValueKind::Tuple(ops, _) => {
                ops.iter().flat_map(|op| op.uses()).collect()
            }
        }
    }
}

impl MIRConstructorArgs {
    pub fn uses(&self) -> HashSet<MIRLocalID> {
        match self {
            MIRConstructorArgs::None => HashSet::new(),
            MIRConstructorArgs::Tuple(ops) => {
                ops.iter().flat_map(|op| op.uses()).collect()
            }
            MIRConstructorArgs::Struct(fields) => {
                fields.values().flat_map(|op| op.uses()).collect()
            }
        }
    }
}

impl MIROperand {
    pub fn uses(&self) -> HashSet<MIRLocalID> {
        match self {
            MIROperand::Constant(_, _) => HashSet::new(),
            MIROperand::Move(p) | MIROperand::Copy(p) => p.uses(),
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

impl MIROperand {
    pub fn span(&self) -> Span {
        match self {
            MIROperand::Constant(_, span) => *span,
            MIROperand::Move(p) | MIROperand::Copy(p) => p.span,
        }
    }
}
