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

use std::collections::HashSet;

use salsa::Accumulator;

use crate::{
    Db,
    compiler::diagnostic::Diag,
    mir::{
        MIR,
        analysis::init_tracking::{
            InitState, IterOperand, MoveKey, MoveMap, init_key, uninit_key,
        },
        basic_block::{MIRTerminator, Stmt},
        operand::{MIROperand, MIRPlace, MIRRValue, MIRRValueKind},
    },
};

pub fn check_use_after_move(db: &dyn Db, mir: &MIR) {
    let init = mir.init_tracking(db);
    for (blk, infos) in &mir.blocks {
        let mut state = init.init_in[&blk].clone();
        for stmt in &infos.stmts {
            match stmt {
                Stmt::Assign { dest, rvalue } => {
                    if let Some(place) = rvalue.inner_place() {
                        place.check(db, &state);
                    }
                    rvalue.for_each_operand(|op| op.check(db, &mut state));
                    dest.for_each_operand(|op| op.check(db, &mut state));
                    init_key(&dest.as_move_key(), &mut state);
                }
            }
        }
        infos.terminator.check(db, &mut state);
    }
}

impl MIRTerminator {
    fn check(&self, db: &dyn Db, state: &mut MoveMap) {
        match self {
            Self::Goto { .. } | Self::Diverge => (),
            Self::Call { arguments, dest, .. } => {
                arguments.iter().for_each(|op| op.check(db, state));
                init_key(&MoveKey { base: *dest, projections: vec![] }, state);
            }
            Self::Return { value, .. } => {
                value.iter().for_each(|op| op.check(db, state));
            }
            Self::Branch { cond: op, .. }
            | Self::Switch { discriminant: op, .. } => op.check(db, state),
        }
    }
}

impl MIROperand {
    fn check(&self, db: &dyn Db, m: &mut MoveMap) {
        match self {
            Self::Move(p) => {
                p.check(db, m);
                uninit_key(&p.as_move_key(), m);
            }
            Self::Copy(p) => {
                p.check(db, m);
            }
            _ => (),
        }
    }
}

pub fn place_init_state(map: &MoveMap, place: &MIRPlace) -> InitState {
    let as_key = place.as_move_key();
    let kinds: HashSet<InitState> =
        HashSet::from_iter(map.iter().filter_map(|(k, state)| {
            (k == &as_key || as_key.is_strict_prefix(k)).then_some(*state)
        }));
    if kinds.is_empty() {
        InitState::Init
    } else if kinds.len() == 1 {
        kinds.into_iter().next().unwrap()
    } else {
        InitState::Maybe
    }
}

impl MIRPlace {
    fn check(&self, db: &dyn Db, m: &MoveMap) {
        let state = place_init_state(m, self);
        match state {
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
