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
    common::location::Span,
    mir::{Callee, MIRLocalID, Place, RValue, Terminator},
};

use super::{BlockID, LocalID, Operand};

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
        Self {
            stmts: vec![],
            terminator,
            name,
        }
    }
}

impl MIRBasicBlock {
    pub fn defs(&self) -> HashSet<MIRLocalID> {
        let mut res = self.terminator.defs();
        res.extend(self.stmts.iter().map(|stmt| match stmt {
            Stmt::Assign { dest, .. } => dest.local,
        }));
        res
    }

    pub fn uses(&self) -> HashSet<MIRLocalID> {
        self.stmts
            .iter()
            .flat_map(|stmt| match stmt {
                Stmt::Assign { rvalue, .. } => rvalue.uses(),
            })
            .chain(self.terminator.uses())
            .collect()
    }
}

impl MIRTerminator {
    pub fn defs(&self) -> HashSet<MIRLocalID> {
        match self {
            MIRTerminator::Call { dest, .. } => HashSet::from([*dest]),
            _ => HashSet::new(),
        }
    }

    pub fn uses(&self) -> HashSet<MIRLocalID> {
        match self {
            MIRTerminator::Goto { .. } | MIRTerminator::Diverge => HashSet::new(),
            MIRTerminator::Call { arguments, .. } => {
                arguments.iter().flat_map(|op| op.uses()).collect()
            }
            MIRTerminator::Return { value: op, .. } => {
                op.iter().flat_map(|op| op.uses()).collect()
            }
            MIRTerminator::Switch {
                discriminant: op, ..
            }
            | MIRTerminator::Branch { cond: op, .. } => op.uses(),
        }
    }
}
