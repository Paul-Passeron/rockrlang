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

use std::{
    collections::{HashMap, HashSet},
    iter::once,
};

use crate::{
    common::{
        arena::{Arena, Idx},
        location::Span,
        symbols::Symbol,
    }, hir::{self, Mutability}, mir::{
        basic_block::{MIRBasicBlock, MIRTerminator},
        cache::MIRCache,
        operand::{
            MIRCallee, MIRConstant, MIRConstructorArgs, MIROperand, MIRPlace,
            MIRProjection, MIRRValue, MIRRValueKind,
        },
    }, ril::TypeRef, thir::{self, FunctionRef}, thir_to_mir::{FuncInst, MIRKey},
};

pub mod analysis;
pub mod basic_block;
pub mod builder;
pub mod cache;
pub mod display;
pub mod operand;
pub mod passes;

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
type ConstructorArgs = MIRConstructorArgs;

pub type MIRBlockID = Idx<BasicBlock>;
pub type MIRLocalID = Idx<Local>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SyntacticSource {
    pub span: Span,
    pub id: hir::LocalId,
}

pub struct MIR {
    pub func: FuncInst,
    pub blocks: Arena<BasicBlock>,
    pub locals: Arena<Local>,
    pub parameters: Vec<LocalID>,
    pub entry: BlockID,

    cache: MIRCache,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MIRLocal {
    pub ty: TypeRef,
    pub mutability: Mutability,

    // Metadata:
    pub span: Span,
    pub name: Option<Symbol>,
    pub thir_src: Option<thir::LocalId>,
    pub syn_src: Option<SyntacticSource>,
}

impl MIRLocal {
    pub fn new(ty: TypeRef, mutability: Mutability, span: Span) -> Self {
        Self {
            ty,
            mutability,
            span,
            name: None,
            thir_src: None,
            syn_src: None,
        }
    }

    pub fn with_thir_src(self, src: thir::LocalId) -> Self {
        let mut this = self;
        this.thir_src = Some(src);
        this
    }

    pub fn with_name(self, name: Symbol) -> Self {
        let mut this = self;
        this.name = Some(name);
        this
    }

    pub fn with_syn_src(self, src: SyntacticSource) -> Self {
        let mut this = self;
        this.syn_src = Some(src);
        this
    }
}

impl MIR {
    fn _compute_reachable(&self, s: &mut HashSet<BlockID>, blk: BlockID) {
        if !s.insert(blk) {
            return;
        }

        match &self.blocks[blk].terminator {
            MIRTerminator::Return { .. } | MIRTerminator::Diverge => (),
            MIRTerminator::Goto { next } | MIRTerminator::Call { next, .. } => {
                self._compute_reachable(s, *next)
            }
            MIRTerminator::Branch { then, else_, .. } => {
                self._compute_reachable(s, *then);
                self._compute_reachable(s, *else_);
            }
            MIRTerminator::Switch {
                branches, default, ..
            } => {
                branches
                    .iter()
                    .map(|b| b.1)
                    .chain(once(default))
                    .for_each(|next| self._compute_reachable(s, *next));
            }
        }
    }

    pub fn compute_reachable(&self, from: BlockID) -> HashSet<BlockID> {
        let mut res = HashSet::new();
        self._compute_reachable(&mut res, from);
        res
    }

    fn compute_successors(&self) -> HashMap<BlockID, HashSet<BlockID>> {
        self.blocks
            .iter()
            .map(|(blk, infos)| {
                let succs = match &infos.terminator {
                    MIRTerminator::Return { .. } | MIRTerminator::Diverge => {
                        HashSet::new()
                    }
                    MIRTerminator::Goto { next } | MIRTerminator::Call { next, .. } => {
                        HashSet::from([*next])
                    }
                    MIRTerminator::Branch { then, else_, .. } => {
                        HashSet::from([*then, *else_])
                    }
                    MIRTerminator::Switch {
                        branches, default, ..
                    } => branches.iter().map(|b| *b.1).chain([*default]).collect(),
                };
                (blk, succs)
            })
            .collect()
    }

    fn compute_predecessors(&self) -> HashMap<BlockID, HashSet<BlockID>> {
        let mut res: HashMap<BlockID, HashSet<BlockID>> = HashMap::new();
        self.successors().iter().for_each(|(pred, succs)| {
            succs.iter().for_each(|succ| {
                res.entry(*succ).or_default().insert(*pred);
            });
        });
        self.blocks.iter().for_each(|(blk, _)| {
            res.entry(blk).or_default();
        });
        res
    }
}

impl PartialEq for MIR {
    fn eq(&self, other: &Self) -> bool {
        self.blocks == other.blocks
            && self.locals == other.locals
            && self.parameters == other.parameters
            && self.entry == other.entry
    }
}

impl Eq for MIR {}
