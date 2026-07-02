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
        MIR,
        analysis::{
            init_tracking::{InitState, IterOperand},
            lattice::LocalMap,
        },
        basic_block::{MIRTerminator, Stmt},
        operand::{MIROperand, MIRPlace, MIRRValue, MIRRValueKind},
    },
};

// TODO: Track partial moves instead of moving the whole local when one of its
// projection gets moved

pub fn check_use_after_move(db: &dyn Db, mir: &MIR) {
    let init = mir.init_tracking(db);
    for (blk, infos) in &mir.blocks {
        let mut state = init.init_in[&blk].clone();
        for stmt in &infos.stmts {
            match stmt {
                Stmt::Assign { dest, rvalue } => {
                    if let Some(place) = rvalue.inner_place() {
                        place.check(db, &mut state);
                    }
                    rvalue.for_each_operand(|op| {
                        op.check(db, &mut state);
                    });
                    state.insert(dest.local, InitState::Init);
                }
            }
        }
        infos.terminator.check(db, &mut state);
    }
}

impl MIRTerminator {
    fn check(&self, db: &dyn Db, state: &mut LocalMap<InitState>) {
        match self {
            MIRTerminator::Goto { .. } | MIRTerminator::Diverge => (),
            MIRTerminator::Call { arguments, .. } => {
                arguments.iter().for_each(|op| op.check(db, state));
            }
            MIRTerminator::Return { value, .. } => {
                value.iter().for_each(|op| op.check(db, state))
            }
            MIRTerminator::Branch { cond: op, .. }
            | MIRTerminator::Switch { discriminant: op, .. } => {
                op.check(db, state)
            }
        }
    }
}

impl MIROperand {
    fn check(&self, db: &dyn Db, m: &mut LocalMap<InitState>) {
        if let MIROperand::Move(p) | MIROperand::Copy(p) = self {
            p.check(db, m)
        }
        self.apply(m);
    }
}

impl MIRPlace {
    fn check(&self, db: &dyn Db, m: &mut LocalMap<InitState>) {
        let state = m.get(&self.local).copied().unwrap_or(InitState::Uninit);
        match state {
            InitState::Init => (),
            InitState::Maybe => Diag::generic_error(
                "Use after move (maybe)".to_string(),
                self.span,
            )
            .accumulate(db),
            InitState::Uninit => {
                Diag::generic_error("Use after move".to_string(), self.span)
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
