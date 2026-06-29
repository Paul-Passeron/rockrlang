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

use crate::{common::{
    arena::{Arena, Idx},
    symbols::Symbol,
}, lir::LIRDef};

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
        target: BrandedBlockId<'ir>,
        args: Vec<ValueId<'ir>>,
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
}
