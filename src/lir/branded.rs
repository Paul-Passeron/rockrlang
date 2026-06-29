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
    common::{
        arena::{Arena, Idx},
        symbols::Symbol,
    },
    lir::{
        LIRDef, LIRFunctionId,
        finalized::{BlockData, FunctionBody, InstKind, Instruction, Terminator},
    },
};

pub type Invariant<'ir> = fn(&'ir ()) -> &'ir ();

// Cheap handle on an SSA value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueId<'ir> {
    idx: Idx<LIRDef>,
    _brand: PhantomData<Invariant<'ir>>,
}

// Cheap handle on a basic block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrandedBlockId<'ir> {
    idx: Idx<BrandedBlockData<'ir>>,
    _brand: PhantomData<Invariant<'ir>>,
}

// move-only: only one ValueDef exists per def and is held by its owner (Either
// an instruction or a block parameter)
pub struct ValueDef<'ir> {
    idx: Idx<LIRDef>,
    _brand: PhantomData<Invariant<'ir>>,
}

impl<'ir> ValueDef<'ir> {
    pub fn id(&self) -> ValueId<'ir> {
        ValueId {
            idx: self.idx,
            _brand: PhantomData,
        }
    }
}

pub struct BrandedInstruction<'ir> {
    pub result: ValueDef<'ir>,
    pub kind: BrandedInstKind<'ir>,
}

pub enum BrandedInstKind<'ir> {
    Store {
        ptr: ValueId<'ir>,
        value: ValueId<'ir>,
    },
}

pub enum BrandedTerminator<'ir> {
    Br {
        cond: ValueId<'ir>,
        block_if_true: BrandedBlockId<'ir>,
        block_if_false: BrandedBlockId<'ir>,
    },
    Goto {
        target: BrandedBlockId<'ir>,
    },
    Switch {
        on: ValueId<'ir>,
        branches: Vec<(u128, BrandedBlockId<'ir>)>,
        default: BrandedBlockId<'ir>,
    },
    Diverge,
    Call {
        id: LIRFunctionId,
        args: Vec<ValueId<'ir>>,
        dest: ValueDef<'ir>,
        next: BrandedBlockId<'ir>,
    },
    Return {
        value: Option<ValueId<'ir>>,
    },
}

pub struct BrandedBlockData<'ir> {
    pub name: Option<Symbol>,
    pub params: Vec<ValueDef<'ir>>,
    pub insts: Vec<BrandedInstruction<'ir>>,
    pub terminator: Option<BrandedTerminator<'ir>>,
}

pub struct InProgressBody<'ir> {
    defs: Arena<LIRDef>,
    blocks: Arena<BrandedBlockData<'ir>>,
    entry: Option<BrandedBlockId<'ir>>,
    _brand: PhantomData<Invariant<'ir>>,
}

impl<'ir> InProgressBody<'ir> {
    pub fn new(_guard: generativity::Guard<'ir>) -> Self {
        Self {
            defs: Arena::new(),
            blocks: Arena::new(),
            entry: None,
            _brand: PhantomData,
        }
    }

    pub fn finalize(self) -> FunctionBody {
        let arena = Arena::new();
        self.blocks.into_values().for_each(|block| {
            arena.insert(block.finalize());
        });
        FunctionBody {
            defs: self.defs,
            blocks: arena,
            entry: Idx::from_raw(self.entry.unwrap().idx.into_raw()),
        }
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

impl<'ir> BrandedInstruction<'ir> {
    pub fn finalize(self) -> Instruction {
        Instruction {
            result: self.result.idx,
            kind: self.kind.finalize(),
        }
    }
}

impl<'ir> BrandedInstKind<'ir> {
    pub fn finalize(self) -> InstKind {
        match self {
            BrandedInstKind::Store { ptr, value } => InstKind::Store {
                ptr: ptr.idx,
                value: value.idx,
            },
        }
    }
}

impl<'ir> BrandedTerminator<'ir> {
    pub fn finalize(self) -> Terminator {
        match self {
            BrandedTerminator::Br {
                cond,
                block_if_true,
                block_if_false,
            } => Terminator::Br {
                cond: cond.idx,
                block_if_true: block_if_true.finalize(),
                block_if_false: block_if_false.finalize(),
            },
            BrandedTerminator::Goto { target } => Terminator::Goto {
                target: target.finalize(),
            },
            BrandedTerminator::Switch {
                on,
                branches,
                default,
            } => Terminator::Switch {
                on: on.idx,
                branches: branches
                    .into_iter()
                    .map(|(n, b)| (n, b.finalize()))
                    .collect_vec(),
                default: default.finalize(),
            },
            BrandedTerminator::Diverge => Terminator::Diverge,
            BrandedTerminator::Call {
                id,
                args,
                dest,
                next,
            } => Terminator::Call {
                id,
                args: args.into_iter().map(|v| v.idx).collect_vec(),
                dest: dest.idx,
                next: next.finalize(),
            },
            BrandedTerminator::Return { value } => Terminator::Return {
                value: value.map(|v| v.idx),
            },
        }
    }
}

impl<'ir> BrandedBlockId<'ir> {
    pub fn finalize(self) -> Idx<BlockData> {
        Idx::from_raw(self.idx.raw())
    }
}
