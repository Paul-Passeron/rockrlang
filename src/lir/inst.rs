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
    layout::{LIRTy, ScalarKind},
    lir::{ArithBinop, CmpBinop, LIRFunctionId, Logic, Refs},
};

pub enum Instruction<R: Refs> {
    Call { dest: Option<R::Def>, id: LIRFunctionId, args: Vec<R::Val> },
    Void(VoidInstKind<R>),
    Value { def: R::Def, kind: ValueInstKind<R> },
}

pub enum ConstValue {
    Int { ty: LIRTy, value: u128 },
    NullPtr { pointee: LIRTy },
    Zeroed { ty: LIRTy },
    Strlit { contents: String, null_terminated: bool },
    FunctionAddr(LIRFunctionId), /* May not be used right now but will be
                                  * later on */
}

pub enum VoidInstKind<R: Refs> {
    Store { ptr: R::Val, value: R::Val },
    MemCopy { src: R::Val, dst: R::Val, ty: LIRTy },
    SetDiscriminant { ptr: R::Val, ty: LIRTy, idx: u32 },
}

pub enum ValueInstKind<R: Refs> {
    // Const
    Const(ConstValue),

    // Memory
    Load { ptr: R::Val, ty: LIRTy },

    // Addressing
    FieldPtr { ptr: R::Val, ty: LIRTy, src_idx: u32 },
    UnionPayloadPtr { ptr: R::Val, ty: LIRTy, variant: u32 },
    GetDiscriminant { ptr: R::Val, ty: LIRTy },
    IndexPtr { ptr: R::Val, elem_ty: LIRTy, index: R::Val },

    // Aggregates
    MakeAggregate { ty: LIRTy, fields_in_src_order: Vec<R::Val> },
    ExtractField { value: R::Val, ty: LIRTy, src_idx: u32 },
    InsertField { value: R::Val, ty: LIRTy, src_idx: u32, field: R::Val },

    // Computation
    Arith { op: ArithBinop, lhs: R::Val, rhs: R::Val },
    Cmp { op: CmpBinop, lhs: R::Val, rhs: R::Val },
    Logic { op: Logic, lhs: R::Val, rhs: R::Val },
    Not { value: R::Val },
    Cast { kind: CastKind, value: R::Val, to: ScalarKind },
}

pub enum CastKind {
    IntTruncate,
    IntExtend { signed: bool },
    IntToPtr,
    PtrToInt,
    // Some float stuff, once we have them
    Bitcast, // Same-size scalar reinterpret
}

pub struct BlockTarget<R: Refs> {
    pub params: Vec<R::Val>,
    pub block: R::Block,
}

pub enum Terminator<R: Refs> {
    Goto(BlockTarget<R>),
    Br {
        cond: R::Val,
        if_true: BlockTarget<R>,
        if_false: BlockTarget<R>,
    },
    Switch {
        on: R::Val,
        branches: Vec<(u128, BlockTarget<R>)>,
        default: BlockTarget<R>,
    },

    Return(Option<R::Val>),
    Diverge,
}
