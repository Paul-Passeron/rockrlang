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

use std::collections::{BTreeMap, HashSet};

use crate::{
    common::{bitset::BitSet, location::Span},
    mir::{Callee, MIRLocalID, Mir, Place, RValue, Terminator},
};

use super::{BlockID, LocalID, Operand, operand::MIROperand};

/// Come with me if you want to live
#[derive(PartialEq, Eq)]
pub enum MIRTerminator {
    /// This terminator cannot be reached. Calling a function returning never is
    /// a way of invoking it.
    Diverge,

    Call {
        callee: Callee,
        arguments: Vec<Operand>,
        dest: LocalID,
        next: BlockID,
        span: Span,
    },

    Return {
        value: Option<Operand>,
        span: Span,
    },

    Goto {
        next: BlockID,
    },

    Branch {
        cond: Operand,
        then: BlockID,
        else_: BlockID,
        span: Span,
    },

    Switch {
        discriminant: Operand,
        branches: BTreeMap<u128, BlockID>,
        default: BlockID,
        span: Span,
    },
}

#[derive(PartialEq, Eq)]
pub struct MIRBasicBlock {
    pub stmts: Vec<Stmt>,
    pub terminator: Terminator,

    // Metadata:
    pub name: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Stmt {
    Assign { dest: Place, rvalue: RValue },
}

impl MIRBasicBlock {
    pub fn empty(name: Option<String>) -> Self {
        Self::empty_with_terminator(name, Terminator::Diverge)
    }

    pub fn empty_with_terminator(name: Option<String>, terminator: Terminator) -> Self {
        Self { stmts: vec![], terminator, name }
    }
}

impl MIRBasicBlock {
    pub fn bitset_defs(&self, mir: &Mir) -> BitSet<MIRLocalID> {
        let mut res = self.terminator.bitset_defs(mir);
        self.stmts.iter().for_each(|stmt| match stmt {
            Stmt::Assign { dest, .. } => {
                res.insert(&dest.local);
            }
        });
        res
    }

    pub fn defs(&self) -> HashSet<MIRLocalID> {
        let mut res = self.terminator.defs();
        res.extend(self.stmts.iter().map(|stmt| match stmt {
            Stmt::Assign { dest, .. } => dest.local,
        }));
        res
    }

    pub fn bitset_uses(&self, mir: &Mir) -> BitSet<MIRLocalID> {
        let domain = mir.locals.len();
        let mut res = BitSet::new(domain);
        let mut defined_so_far = BitSet::new(domain);
        for stmt in &self.stmts {
            match stmt {
                Stmt::Assign { dest, rvalue } => {
                    let mut uses = rvalue.bitset_uses(mir);
                    uses.substract(&defined_so_far);
                    res.union(&uses);
                    defined_so_far.insert(&dest.local);
                }
            }
        }
        let mut term_uses = self.terminator.bitset_uses(mir);
        term_uses.substract(&defined_so_far);
        res.union(&term_uses);
        res
    }

    pub fn uses(&self) -> HashSet<MIRLocalID> {
        let mut uses = HashSet::new();
        let mut defined_so_far: HashSet<MIRLocalID> = HashSet::new();

        for stmt in &self.stmts {
            match stmt {
                Stmt::Assign { dest, rvalue } => {
                    for local in rvalue.uses() {
                        if !defined_so_far.contains(&local) {
                            uses.insert(local);
                        }
                    }
                    defined_so_far.insert(dest.local);
                }
            }
        }

        for local in self.terminator.uses() {
            if !defined_so_far.contains(&local) {
                uses.insert(local);
            }
        }

        uses
    }
}

impl MIRTerminator {
    pub fn bitset_defs(&self, mir: &Mir) -> BitSet<MIRLocalID> {
        let domain = mir.locals.len();
        let mut res = BitSet::new(domain);
        match self {
            Self::Call { dest, .. } => {
                res.insert(dest);
            }
            _ => (),
        };
        res
    }

    pub fn defs(&self) -> HashSet<MIRLocalID> {
        match self {
            Self::Call { dest, .. } => HashSet::from([*dest]),
            _ => HashSet::new(),
        }
    }

    pub fn bitset_uses(&self, mir: &Mir) -> BitSet<MIRLocalID> {
        let domain = mir.locals.len();
        match self {
            Self::Goto { .. } | Self::Diverge => BitSet::new(domain),
            Self::Call { arguments, .. } => {
                let mut res = BitSet::new(domain);
                arguments.iter().for_each(|op| {
                    res.union(&op.bitset_uses(mir));
                });
                res
            }
            Self::Return { value: op, .. } => {
                let mut res = BitSet::new(domain);
                op.iter().for_each(|op| {
                    res.union(&op.bitset_uses(mir));
                });
                res
            }
            Self::Switch { discriminant: op, .. } | Self::Branch { cond: op, .. } => {
                op.bitset_uses(mir)
            }
        }
    }

    pub fn uses(&self) -> HashSet<MIRLocalID> {
        match self {
            Self::Goto { .. } | Self::Diverge => HashSet::new(),
            Self::Call { arguments, .. } => {
                arguments.iter().flat_map(MIROperand::uses).collect()
            }
            Self::Return { value: op, .. } => {
                op.iter().flat_map(MIROperand::uses).collect()
            }
            Self::Switch { discriminant: op, .. } | Self::Branch { cond: op, .. } => {
                op.uses()
            }
        }
    }
}
