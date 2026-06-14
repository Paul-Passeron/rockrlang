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
        location::Span,
        symbols::Symbol,
    },
    hir::{self, Mutability},
    mir::{
        basic_block::{MIRBasicBlock, MIRTerminator},
        operand::{
            MIRCallee, MIRConstant, MIROperand, MIRPlace, MIRProjection, MIRRValue,
            MIRRValueKind,
        },
    },
    ril::TypeRef,
    thir,
};

pub mod basic_block;
pub mod operand;

// MIR-local type aliases
type BasicBlock = MIRBasicBlock;
type BlockID = MIRBlockID;
type Local = MIRLocal;
type LocalID = MIRLocalID;
type Terminator = MIRTerminator;
type Operand = MIROperand;
type Place = MIRPlace;
type Constant = MIRConstant;
type Projection = MIRProjection;
type RValue = MIRRValue;
type RValueKind = MIRRValueKind;
type Callee = MIRCallee;

pub type MIRBlockID = Idx<BasicBlock>;
pub type MIRLocalID = Idx<Local>;

pub struct SyntacticSource {
    pub span: Span,
    pub id: hir::LocalId,
}

pub struct MIR {
    pub blocks: Arena<BasicBlock>,
    pub locals: Arena<Local>,
    pub entry: BlockID,
}

pub struct MIRLocal {
    pub ty: TypeRef,
    pub mutability: Mutability,

    // Metadata:
    pub span: Span,
    pub name: Option<Symbol>,
    pub thir_src: Option<thir::LocalId>,
    pub syn_src: Option<SyntacticSource>,
}
