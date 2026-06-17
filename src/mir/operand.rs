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

use std::collections::HashMap;

use crate::{
    Db,
    common::{location::Span, symbols::Symbol},
    hir::Mutability,
    mir::{ConstructorArgs, LocalID, Operand, Projection, RValueKind},
    parse_tree::expr::BinaryOperator,
    ril::{TypeRef, bool_id, char_id, ptr_of},
    thir::{EnumRef, FunctionRef, StructRef},
};

use super::{Constant, Place};

#[derive(PartialEq, Eq, Clone)]
pub enum MIROperand {
    Constant(Constant),
    Move(Place),
    Copy(Place),
}

#[derive(PartialEq, Eq, Clone)]
pub enum MIRConstant {
    Integer {
        value: i128,
        ty: TypeRef,
    },
    Bool(bool),
    CString {
        contents: String,
        null_terminated: bool,
    },
}

#[derive(PartialEq, Eq, Clone)]
pub struct MIRPlace {
    pub local: LocalID,
    pub projections: Vec<Projection>,
    pub ty: TypeRef,
}

#[derive(PartialEq, Eq, Clone)]
pub enum MIRProjection {
    Deref,
    Field { name: Symbol, resulting_ty: TypeRef },
    TupleField { index: u32, resulting_ty: TypeRef },
    Index { index: Operand },
    Downcast {
        variant: usize
    },
}

#[derive(PartialEq, Eq)]
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

#[derive(PartialEq, Eq)]
pub enum MIRCallee {
    Direct(FunctionRef),
}

#[derive(PartialEq, Eq)]
pub enum MIRRValueKind {
    Use(Operand),
    Ref(Place, Mutability),
    AddressOf(Place, Mutability),
    BinOp(BinaryOperator, Operand, Operand),
    UnaryOp(UnaryOperator, Operand),
    Constructor {
        enum_ref: EnumRef,
        idx: usize,
        args: ConstructorArgs,
    },
    StructLit {
        struct_ref: StructRef,
        fields: HashMap<Symbol, Operand>,
    },
    Discriminant(Place),
    Metadata(Operand),
    SizeOf(TypeRef),
    Tuple(Vec<Operand>),
}

#[derive(PartialEq, Eq)]
pub enum MIRConstructorArgs {
    None,
    Tuple(Vec<Operand>),
    Struct(HashMap<Symbol, Operand>),
}

impl Constant {
    pub fn ty(&self, db: &dyn Db) -> TypeRef {
        match self {
            MIRConstant::Integer { ty, .. } => *ty,
            MIRConstant::Bool(_) => bool_id(db).into(),
            MIRConstant::CString { .. } => ptr_of(db, char_id(db).into(), false).into(),
        }
    }
}

impl Operand {
    pub fn ty(&self, db: &dyn Db) -> TypeRef {
        match self {
            MIROperand::Constant(mirconstant) => mirconstant.ty(db),
            MIROperand::Move(mirplace) | MIROperand::Copy(mirplace) => mirplace.ty,
        }
    }
}

impl From<Constant> for Operand {
    fn from(value: Constant) -> Self {
        Self::Constant(value)
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
