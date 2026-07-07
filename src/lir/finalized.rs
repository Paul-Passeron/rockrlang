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
    common::{
        arena::{Arena, Idx},
        symbols::Symbol,
    }, layout::LIRTy, lir::{Finalized, LIRDef},
};

type Terminator = super::inst::Terminator<Finalized>;
type Instruction = super::inst::Instruction<Finalized>;

pub struct BlockData {
    pub name: Option<Symbol>,
    pub params: Vec<Idx<LIRDef>>,
    pub insts: Vec<Instruction>,
    pub terminator: Terminator,
}

pub struct StackSlot {
    pub value: Idx<LIRDef>,
    pub pointee_ty: LIRTy, // pointee
}

pub struct FunctionBody {
    pub defs: Arena<LIRDef>,
    pub stack_slots: Vec<StackSlot>,
    pub blocks: Arena<BlockData>,
    pub entry: Idx<BlockData>,
}
