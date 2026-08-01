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

use salsa::Accumulator;

use crate::{
    Db,
    compiler::diagnostic::Diag,
    mir::{
        Mir,
        analysis::init_tracking::{
            IdInitMap, InitState, IterOperand, MIRInitOut, MoveKey,
        },
        basic_block::{MIRTerminator, Stmt},
        operand::{MIROperand, MIRPlace, MIRRValue, MIRRValueKind},
    },
};

pub fn check_use_after_move(db: &dyn Db, mir: &Mir) {
    let init = mir.init_tracking(db);
    for (blk, infos) in &mir.blocks {
        let mut state = init.entry_state(blk);
        for stmt in &infos.stmts {
            match stmt {
                Stmt::Assign { dest, rvalue } => {
                    if let Some(place) = rvalue.inner_place() {
                        place.check(db, init, &state);
                    }
                    rvalue.for_each_operand(|op| op.check(db, init, &mut state));
                    dest.for_each_operand(|op| op.check(db, init, &mut state));
                    init.mark_init(&mut state, &dest.as_move_key());
                }
            }
        }
        infos.terminator.check(db, init, &mut state);
    }
}

impl MIRTerminator {
    fn check(&self, db: &dyn Db, init: &MIRInitOut, state: &mut IdInitMap) {
        match self {
            Self::Goto { .. } | Self::Diverge => (),
            Self::Call { arguments, dest, .. } => {
                for op in arguments {
                    op.check(db, init, state);
                }
                init.mark_init(state, &MoveKey { base: *dest, projections: vec![] });
            }
            Self::Return { value, .. } => {
                if let Some(op) = value {
                    op.check(db, init, state);
                }
            }
            Self::Branch { cond: op, .. } | Self::Switch { discriminant: op, .. } => {
                op.check(db, init, state);
            }
        }
    }
}

impl MIROperand {
    fn check(&self, db: &dyn Db, init: &MIRInitOut, m: &mut IdInitMap) {
        match self {
            Self::Move(p) => {
                p.check(db, init, m);
                init.mark_uninit(m, &p.as_move_key());
            }
            Self::Copy(p) => {
                p.check(db, init, m);
            }
            Self::Constant(_, _) => (),
        }
    }
}

impl MIRPlace {
    fn check(&self, db: &dyn Db, init: &MIRInitOut, m: &IdInitMap) {
        match init.query(m, &self.as_move_key()) {
            InitState::Init => (),
            InitState::Maybe => {
                Diag::generic_error("Use after move (maybe)".to_owned(), self.span)
                    .accumulate(db);
            }
            InitState::Uninit => {
                Diag::generic_error("Use after move".to_owned(), self.span)
                    .accumulate(db);
            }
        }
    }
}

impl MIRRValue {
    pub fn inner_place(&self) -> Option<&MIRPlace> {
        match &self.kind {
            MIRRValueKind::Ref(p, _)
            | MIRRValueKind::AddressOf(p, _)
            | MIRRValueKind::Discriminant(p) => Some(p),
            _ => None,
        }
    }
}
