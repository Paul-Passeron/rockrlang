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
    },
    lir::LIRDef,
};

pub struct BlockData {
    pub name: Option<Symbol>,
    pub params: Vec<LIRDef>,
    pub insts: Vec<Instruction>,
    pub terminator: Terminator,
}

pub struct Instruction {
    pub result: Idx<LIRDef>,
    pub kind: InstKind,
}

pub enum InstKind {
    Store {
        ptr: Idx<LIRDef>,
        value: Idx<LIRDef>,
    },
}

pub enum Terminator {
    Br {
        target: Idx<BlockData>,
        args: Vec<Idx<LIRDef>>,
    },
}

pub struct FunctionBody {
    pub defs: Arena<LIRDef>,
    pub blocks: Arena<BlockData>,
    pub entry: Idx<BlockData>,
}
