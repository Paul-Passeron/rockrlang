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

use std::collections::BTreeMap;

use crate::mir::{Callee, Place, RValue, Terminator};

use super::{BlockID, LocalID, Operand};

/// Come with me if you want to live
pub enum MIRTerminator {
    /// This terminator cannot be reached. Calling a function returning never is
    /// a way of invoking it.
    Diverge,

    Call {
        callee: Callee,
        arguments: Vec<Operand>,
        dest: LocalID,
        next: BlockID,
    },

    Return {
        value: Option<Operand>,
    },

    Goto {
        next: BlockID,
    },

    Branch {
        cond: Operand,
        then: BlockID,
        else_: BlockID,
    },

    Switch {
        discriminant: Operand,
        branches: BTreeMap<u128, BlockID>,
    },
}

pub struct MIRBasicBlock {
    pub stmts: Vec<Stmt>,
    pub terminator: Terminator,
}

pub enum Stmt {
    Assign { dest: Place, rvalue: RValue },
}
