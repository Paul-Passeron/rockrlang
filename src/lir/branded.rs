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
    lir::{
        Branded, Finalized, FunctionSig, LIRDef, ValueDef, VerifyError,
        finalized::{BlockData, FunctionBody},
        inst::ValueInstKind,
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
    pub(super) idx: Idx<BrandedBlockData<'ir>>,
    _brand: PhantomData<Invariant<'ir>>,
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

pub struct InProgressBody<'ir> {
    pub defs: Arena<LIRDef>,
    pub blocks: Arena<BrandedBlockData<'ir>>,
    pub entry: BrandedBlockId<'ir>,
    _brand: PhantomData<Invariant<'ir>>,
}

impl<'ir> InProgressBody<'ir> {
    pub fn new(
        _guard: generativity::Guard<'ir>,
        db: &dyn Db,
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
            defs,
            blocks,
            entry: BrandedBlockId { idx: entry, _brand: PhantomData },
            _brand: PhantomData,
        }
    }

    fn verify(&self) -> Result<(), VerifyError> {
        todo!()
    }

    pub fn finalize(self) -> Result<FunctionBody, VerifyError> {
        self.verify()?;
        let arena = Arena::new();
        self.blocks.into_values().for_each(|block| {
            arena.insert(block.finalize());
        });
        Ok(FunctionBody {
            defs: self.defs,
            blocks: arena,
            entry: Idx::from_raw(self.entry.idx.into_raw()),
        })
    }
}

impl<'ir> BrandedBlockData<'ir> {
    pub fn finalize(self) -> BlockData {
        BlockData {
            name: self.name,
            params: self.params.into_iter().map(|param| param.idx).collect(),
            insts: self.insts.into_iter().map(|inst| inst.finalize()).collect(),
            terminator: self.terminator.unwrap().finalize(),
        }
    }
}

impl<'ir> Instruction<'ir> {
    pub fn finalize(self) -> FInstruction {
        match self {
            Instruction::Void(_branded_void_instruction) => todo!(),
            Instruction::Value { def: _, kind: _ } => todo!(),
        }
    }
}

impl<'ir> ValueInstKind<Branded<'ir>> {
    pub fn finalize(self) -> ValueInstKind<Finalized> {
        match self {
            ValueInstKind::Const(_const_value) => todo!(),
            ValueInstKind::Alloca { ty: _ } => todo!(),
            ValueInstKind::Load { ptr: _, ty: _ } => todo!(),
            ValueInstKind::FieldPtr { ptr: _, ty: _, idx: _ } => todo!(),
            ValueInstKind::UnionPayloadPtr { ptr: _, ty: _, variant: _ } => todo!(),
            ValueInstKind::GetDiscriminant { ptr: _, ty: _ } => todo!(),
            ValueInstKind::MakeAggregate { ty: _, fields: _ } => todo!(),
            ValueInstKind::ExtractField { value: _, ty: _, idx: _ } => todo!(),
            ValueInstKind::InsertField { value: _, ty: _, idx: _, field: _ } => todo!(),
            ValueInstKind::Arith { op: _, lhs: _, rhs: _ } => todo!(),
            ValueInstKind::Cmp { op: _, lhs: _, rhs: _ } => todo!(),
            ValueInstKind::Logic { op: _, lhs: _, rhs: _ } => todo!(),
            ValueInstKind::Not { value: _ } => todo!(),
            ValueInstKind::Cast { kind: _, value: _, to: _ } => todo!(),
        }
    }
}

impl<'ir> Terminator<'ir> {
    pub fn finalize(self) -> FTerminator {
        match self {
            Terminator::Goto(_block_target) => todo!(),
            Terminator::Br { cond: _, if_true: _, if_false: _ } => todo!(),
            Terminator::Switch { on: _, branches: _, default: _ } => todo!(),
            Terminator::Call { id: _, args: _, dest: _, next: _ } => todo!(),
            Terminator::Return(_) => todo!(),
            Terminator::Diverge => todo!(),
        }
    }
}

impl<'ir> BrandedBlockId<'ir> {
    pub fn finalize(self) -> Idx<BlockData> {
        Idx::from_raw(self.idx.raw())
    }
}
