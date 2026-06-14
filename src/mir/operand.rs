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

use crate::{
    common::{location::Span, symbols::Symbol},
    hir::Mutability,
    mir::{LocalID, Operand, Projection, RValueKind},
    parse_tree::expr::BinaryOperator,
    ril::TypeRef,
    thir::{EnumRef, FunctionRef},
};

use super::{Constant, Place};

pub enum MIROperand {
    Constant(Constant),
    Move(Place),
    Copy(Place),
}

pub enum MIRConstant {
    Integer { value: i128, ty: TypeRef },
}

pub struct MIRPlace {
    pub local: LocalID,
    pub projection: Vec<Projection>,
    pub ty: TypeRef,
}

pub enum MIRProjection {
    Deref,
    Field { name: Symbol, resulting_ty: TypeRef },
    TupleField { index: u32, resulting_ty: TypeRef },
    Index { index: Operand },
}

pub struct MIRRValue {
    pub kind: RValueKind,
    pub ty: TypeRef,
    pub span: Span,
}

pub enum UnaryOperator {
    Neg,  // `-` in -x
    LNot, // `!` in !x
}

pub enum MIRCallee {
    Direct(FunctionRef),
}

pub enum MIRRValueKind {
    Use(Operand),
    Ref(Place, Mutability),
    BinOp(BinaryOperator, Operand, Operand),
    UnaryOp(UnaryOperator, Operand),
    Constructor {
        enum_def: EnumRef,
        idx: usize,
        args: Vec<Operand>,
    },
    Discriminant(Place),
}

impl Constant {
    pub fn ty(&self) -> TypeRef {
        match self {
            MIRConstant::Integer { ty, .. } => *ty,
        }
    }
}

impl Operand {
    pub fn ty(&self) -> TypeRef {
        match self {
            MIROperand::Constant(mirconstant) => mirconstant.ty(),
            MIROperand::Move(mirplace) | MIROperand::Copy(mirplace) => mirplace.ty,
        }
    }
}
