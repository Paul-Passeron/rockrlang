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

use std::marker::PhantomData;

use itertools::Itertools;

use crate::{
    Db,
    common::{
        arena::{Arena, Idx},
        symbols::Symbol,
    },
    layout::LIRTy,
    lir::{
        Branded, Finalized, FunctionSig, LIRDef, LIRFunctionId, ValueDef, ValueId,
        VerifyError,
        finalized::{BlockData, FunctionBody, StackSlot},
        inst::{BlockTarget, ValueInstKind, VoidInstKind},
    },
};

type Instruction<'ir> = super::inst::Instruction<Branded<'ir>>;
type Terminator<'ir> = super::inst::Terminator<Branded<'ir>>;
type FInstruction = super::inst::Instruction<Finalized>;
type FTerminator = super::inst::Terminator<Finalized>;
pub type Invariant<'ir> = fn(&'ir ()) -> &'ir ();

// Cheap handle on a basic block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrandedBlockId<'ir> {
    pub idx: Idx<BrandedBlockData<'ir>>,
    pub(super) _brand: PhantomData<Invariant<'ir>>,
}

impl<'ir> InProgressBody<'ir> {
    pub fn new_block(&self, name: Option<Symbol>) -> BrandedBlockId<'ir> {
        BrandedBlockId {
            idx: self.blocks.insert(BrandedBlockData {
                name,
                params: Vec::new(),
                insts: Vec::new(),
                terminator: None,
            }),
            _brand: PhantomData,
        }
    }
}

impl<'ir> BrandedBlockId<'ir> {
    pub fn from_idx(idx: Idx<BrandedBlockData<'ir>>) -> Self {
        Self { idx, _brand: PhantomData }
    }
}

pub struct BrandedBlockData<'ir> {
    pub name: Option<Symbol>,
    pub params: Vec<ValueDef<'ir>>,
    pub insts: Vec<Instruction<'ir>>,
    pub terminator: Option<Terminator<'ir>>,
}

pub struct BrandedStackSlot<'ir> {
    pub value: ValueId<'ir>,
    pub ty: LIRTy,
}

pub struct InProgressBody<'ir> {
    pub id: LIRFunctionId,
    pub defs: Arena<LIRDef>,
    pub slots: Vec<BrandedStackSlot<'ir>>,
    pub blocks: Arena<BrandedBlockData<'ir>>,
    pub entry: BrandedBlockId<'ir>,
    _brand: PhantomData<Invariant<'ir>>,
}

impl<'ir> InProgressBody<'ir> {
    pub fn new(
        _guard: generativity::Guard<'ir>,
        db: &dyn Db,
        id: LIRFunctionId,
        sig: &FunctionSig,
    ) -> Self {
        let defs = Arena::new();
        let blocks = Arena::new();
        let params = sig
            .signature
            .params
            .iter()
            .map(|ty| {
                let idx = defs.insert(LIRDef { ty: *ty });
                ValueDef { idx, _brand: PhantomData }
            })
            .collect_vec();
        let entry = blocks.insert(BrandedBlockData {
            name: Some(Symbol::new(db, "entry")),
            params,
            insts: Vec::new(),
            terminator: None,
        });
        Self {
            id,
            defs,
            blocks,
            slots: Vec::new(),
            entry: BrandedBlockId { idx: entry, _brand: PhantomData },
            _brand: PhantomData,
        }
    }

    fn verify(&self) -> Result<(), VerifyError> {
        // TODO: verify
        Ok(())
    }

    pub fn finalize(self, db: &dyn Db) -> Result<FunctionBody, VerifyError> {
        self.verify()?;
        let arena = Arena::new();
        self.blocks.into_values().for_each(|block| {
            arena.insert(block.finalize());
        });
        Ok(FunctionBody {
            defs: self.defs,
            blocks: arena,
            stack_slots: self
                .slots
                .into_iter()
                .map(|slot| {
                    debug_assert!(!slot.ty.is_zst(db));
                    StackSlot { value: slot.value.idx, pointee_ty: slot.ty }
                })
                .collect(),
            entry: Idx::from_raw(self.entry.idx.into_raw()),
        })
    }
}

impl BrandedBlockData<'_> {
    pub fn finalize(self) -> BlockData {
        BlockData {
            name: self.name,
            params: self.params.into_iter().map(|param| param.idx).collect(),
            insts: self
                .insts
                .into_iter()
                .map(super::inst::Instruction::finalize)
                .collect(),
            terminator: self.terminator.unwrap().finalize(),
        }
    }
}

impl Instruction<'_> {
    pub fn finalize(self) -> FInstruction {
        match self {
            Instruction::Void(kind) => FInstruction::Void(kind.finalize()),
            Instruction::Value { def, kind } => {
                FInstruction::Value { def: def.idx, kind: kind.finalize() }
            }
            Instruction::Call { dest, id, args } => FInstruction::Call {
                dest: dest.map(|d| d.idx),
                id,
                args: args.into_iter().map(|arg| arg.idx).collect_vec(),
            },
        }
    }
}

impl ValueInstKind<Branded<'_>> {
    pub fn finalize(self) -> ValueInstKind<Finalized> {
        match self {
            Self::Const(cst) => ValueInstKind::Const(cst),
            Self::Load { ptr, ty } => ValueInstKind::Load { ptr: ptr.idx, ty },
            Self::FieldPtr { ptr, ty, src_idx } => {
                ValueInstKind::FieldPtr { ptr: ptr.idx, ty, src_idx }
            }
            Self::UnionPayloadPtr { ptr, ty, variant } => {
                ValueInstKind::UnionPayloadPtr { ptr: ptr.idx, ty, variant }
            }
            Self::GetDiscriminant { ptr, ty } => {
                ValueInstKind::GetDiscriminant { ptr: ptr.idx, ty }
            }
            Self::IndexPtr { ptr, elem_ty, index } => {
                ValueInstKind::IndexPtr { ptr: ptr.idx, elem_ty, index: index.idx }
            }
            Self::MakeAggregate { ty, fields_in_src_order } => {
                ValueInstKind::MakeAggregate {
                    ty,
                    fields_in_src_order: fields_in_src_order
                        .into_iter()
                        .map(|f| f.idx)
                        .collect(),
                }
            }
            Self::ExtractField { value, ty, src_idx } => {
                ValueInstKind::ExtractField { value: value.idx, ty, src_idx }
            }
            Self::InsertField { value, ty, src_idx, field } => {
                ValueInstKind::InsertField {
                    value: value.idx,
                    ty,
                    src_idx,
                    field: field.idx,
                }
            }
            Self::Arith { op, lhs, rhs } => {
                ValueInstKind::Arith { op, lhs: lhs.idx, rhs: rhs.idx }
            }
            Self::Cmp { op, lhs, rhs } => {
                ValueInstKind::Cmp { op, lhs: lhs.idx, rhs: rhs.idx }
            }
            Self::Logic { op, lhs, rhs } => {
                ValueInstKind::Logic { op, lhs: lhs.idx, rhs: rhs.idx }
            }
            Self::Not { value } => ValueInstKind::Not { value: value.idx },
            Self::Cast { kind, value, to } => {
                ValueInstKind::Cast { kind, value: value.idx, to }
            }
        }
    }
}

impl VoidInstKind<Branded<'_>> {
    pub fn finalize(self) -> VoidInstKind<Finalized> {
        match self {
            VoidInstKind::Store { ptr, value } => {
                VoidInstKind::Store { ptr: ptr.idx, value: value.idx }
            }
            VoidInstKind::MemCopy { src, dst, ty } => {
                VoidInstKind::MemCopy { src: src.idx, dst: dst.idx, ty }
            }
            VoidInstKind::SetDiscriminant { ptr, ty, idx } => {
                VoidInstKind::SetDiscriminant { ptr: ptr.idx, ty, idx }
            }
        }
    }
}

impl Terminator<'_> {
    pub fn finalize(self) -> FTerminator {
        match self {
            Terminator::Goto(block_target) => FTerminator::Goto(block_target.finalize()),
            Terminator::Br { cond, if_true, if_false } => FTerminator::Br {
                cond: cond.idx,
                if_true: if_true.finalize(),
                if_false: if_false.finalize(),
            },
            Terminator::Switch { on, branches, default } => FTerminator::Switch {
                on: on.idx,
                branches: branches
                    .into_iter()
                    .map(|(idx, target)| (idx, target.finalize()))
                    .collect(),
                default: default.finalize(),
            },
            Terminator::Return(val) => FTerminator::Return(val.map(|v| v.idx)),
            Terminator::Diverge => FTerminator::Diverge,
        }
    }
}

impl BlockTarget<Branded<'_>> {
    pub fn finalize(self) -> BlockTarget<Finalized> {
        BlockTarget {
            params: self.params.into_iter().map(|p| p.idx).collect(),
            block: self.block.finalize(),
        }
    }
}

impl BrandedBlockId<'_> {
    pub fn finalize(self) -> Idx<BlockData> {
        Idx::from_raw(self.idx.raw())
    }
}
