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
    collections::{BTreeSet, HashMap, HashSet},
    iter::once,
};

use itertools::Itertools;

use crate::{
    Db,
    mir::{
        MIR, MIRBlockID, MIRLocalID, Operand,
        basic_block::{MIRTerminator, Stmt},
        builder::MIRBuilder,
        operand::MIROperand,
        passes::MIRPass,
    },
};

pub struct DeadCodeElimination;

impl MIRPass for DeadCodeElimination {
    fn run(&self, db: &dyn Db, mir: &MIR) -> MIR {
        DCECtx::new(db, mir).run()
    }
}

type OldBlockID = MIRBlockID;
type OldLocalID = MIRLocalID;
type Paths = BTreeSet<Vec<OldBlockID>>;

pub struct DCECtx<'a> {
    db: &'a dyn Db,
    mir: &'a MIR,

    b: MIRBuilder<'a>,

    local_map: HashMap<OldLocalID, MIRLocalID>,
    reachable: BTreeSet<OldBlockID>,
    predecessors: HashMap<OldBlockID, HashSet<OldBlockID>>,
    successors: HashMap<OldBlockID, HashSet<OldBlockID>>,
    paths: Vec<Vec<OldBlockID>>,
    path_bodies: Vec<MIRBlockID>,
    path_heads: HashMap<OldBlockID, usize>,
}

impl<'a> DCECtx<'a> {
    pub fn new(db: &'a dyn Db, mir: &'a MIR) -> Self {
        let entry_name = mir.blocks[mir.entry].name.clone();
        let b = MIRBuilder::new(db, entry_name);
        Self {
            db,
            mir,
            b,
            local_map: HashMap::new(),
            reachable: BTreeSet::new(),
            predecessors: HashMap::new(),
            successors: HashMap::new(),
            paths: Vec::new(),
            path_bodies: Vec::new(),
            path_heads: HashMap::new(),
        }
    }

    fn _compute_reachable(&self, s: &mut BTreeSet<OldBlockID>, blk: OldBlockID) {
        if !s.insert(blk) {
            return;
        }

        match &self.mir.blocks[blk].terminator {
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

    fn compute_reachable(&mut self) {
        let mut res = BTreeSet::new();
        self._compute_reachable(&mut res, self.mir.entry);
        self.reachable = res;
    }

    fn add_and_get_local_to_mapping(&mut self, original: MIRLocalID) -> MIRLocalID {
        if let Some(res) = self.local_map.get(&original) {
            return *res;
        }
        let description = self.mir.locals[original].clone();
        let new_id = self.b.new_local(description);
        self.local_map.insert(original, new_id);
        new_id
    }

    fn add_parameters(&mut self) {
        let new_params = self
            .mir
            .parameters
            .iter()
            .map(|param| self.add_and_get_local_to_mapping(*param))
            .collect_vec();

        self.b.set_parameters(new_params);
    }

    fn can_compress_paths(
        &self,
        p1: &[OldBlockID],
        p2: &[OldBlockID],
    ) -> Option</* reversed */ bool> {
        assert!(!p1.is_empty());
        assert!(!p2.is_empty());

        if p1.iter().chain(p2).unique().count() != p1.len() + p2.len() {
            // duplicate blocks in path, can't merge
            return None;
        }

        let (p1, p2, reversed) = {
            let p1_fst = p1.first().unwrap();
            let p2_last = p2.last().unwrap();
            if self.successors[p2_last].contains(p1_fst) {
                (p2, p1, true)
            } else {
                (p1, p2, false)
            }
        };

        let p1_last = p1.last().unwrap();
        let p2_fst = p2.first().unwrap();

        if !matches!(
            &self.mir.blocks[*p1_last].terminator,
            MIRTerminator::Goto { .. },
        ) {
            return None;
        }

        if self.successors[p1_last].len() != 1 || self.predecessors[p2_fst].len() != 1 {
            return None;
        }

        let p1_next = *self.successors[p1_last].iter().next().unwrap();
        let p2_before = *self.predecessors[p2_fst].iter().next().unwrap();

        if p1_next != p2_before {
            return None;
        }

        Some(reversed)
    }

    fn compute_paths(&mut self) {
        let mut paths = self.reachable.iter().map(|blk| vec![*blk]).collect_vec();
        loop {
            if paths.len() == 1 {
                break;
            }
            let mut found = None;
            'outer: for i in 0..paths.len() {
                for j in 0..i {
                    if let Some(reversed) = self.can_compress_paths(&paths[i], &paths[j])
                    {
                        found = Some((i, j, reversed));
                        break 'outer;
                    }
                }
            }
            if let Some((i, j, reversed)) = found {
                if reversed {
                    let p2 = paths.remove(i);
                    paths[j].extend(p2);
                } else {
                    let p2 = paths.remove(j);
                    paths[i - 1].extend(p2);
                }
            } else {
                break;
            }
        }
        self.paths = paths;
        self.compute_path_heads();
    }

    fn compute_path_heads(&mut self) {
        self.path_heads.extend(
            self.paths
                .iter()
                .enumerate()
                .map(|(i, p)| (*p.first().unwrap(), i)),
        );
    }

    fn compute_successors(&mut self) {
        let succs =
            self.reachable
                .iter()
                .map(|blk| {
                    let succs = match &self.mir.blocks[*blk].terminator {
                        MIRTerminator::Return { .. } | MIRTerminator::Diverge => {
                            HashSet::new()
                        }
                        MIRTerminator::Goto { next }
                        | MIRTerminator::Call { next, .. } => HashSet::from([*next]),
                        MIRTerminator::Branch { then, else_, .. } => {
                            HashSet::from([*then, *else_])
                        }
                        MIRTerminator::Switch {
                            branches, default, ..
                        } => branches.iter().map(|b| *b.1).chain([*default]).collect(),
                    };
                    (*blk, succs)
                })
                .collect_vec();
        self.successors.extend(succs);
    }

    fn compute_predecessors(&mut self) {
        self.successors.iter().for_each(|(pred, succs)| {
            succs.into_iter().for_each(|succ| {
                self.predecessors.entry(*succ).or_default().insert(*pred);
            });
        });
    }

    fn copy_stmt(&mut self, stmt: &Stmt) -> Stmt {
        todo!()
    }

    fn copy_operand(&mut self, operand: &MIROperand) -> MIROperand {
        todo!()
    }

    fn get_bb(&self, old: OldBlockID) -> MIRBlockID {
        self.path_bodies[self.path_heads[&old]]
    }

    fn copy_terminator(&mut self, terminator: &MIRTerminator) -> MIRTerminator {
        match terminator {
            MIRTerminator::Diverge => MIRTerminator::Diverge,
            MIRTerminator::Call {
                callee,
                arguments,
                dest,
                next,
                span,
            } => MIRTerminator::Call {
                callee: callee.clone(),
                arguments: arguments
                    .iter()
                    .map(|operand| self.copy_operand(operand))
                    .collect(),
                dest: self.add_and_get_local_to_mapping(*dest),
                next: self.get_bb(*next),
                span: *span,
            },
            MIRTerminator::Return { value, span } => MIRTerminator::Return {
                value: value.as_ref().map(|operand| self.copy_operand(operand)),
                span: *span,
            },
            MIRTerminator::Goto { next } => MIRTerminator::Goto {
                next: self.get_bb(*next),
            },
            MIRTerminator::Branch {
                cond,
                then,
                else_,
                span,
            } => todo!(),
            MIRTerminator::Switch {
                discriminant,
                branches,
                default,
                span,
            } => todo!(),
        }
    }

    fn compute_path_bodies(&mut self) {
        for p in &self.paths {
            assert!(!p.is_empty());
            let name = self.salvage_path_name(p);
            let bb = self.b.new_block(name);
            self.path_bodies.push(bb);
        }

        let paths = self.paths.clone();
        let bbs = self.path_bodies.clone();
        for (p, bb) in paths.iter().zip_eq(bbs) {
            self.b.switch_to_block(bb).unwrap();
            let stmts = p
                .iter()
                .flat_map(|blk| self.mir.blocks[*blk].stmts.clone())
                .collect_vec();
            stmts.iter().for_each(|stmt| {
                let new_stmt = self.copy_stmt(&stmt);
                self.b.emit(new_stmt);
            });
        }
    }

    fn salvage_path_name(&self, path: &[OldBlockID]) -> Option<String> {
        let names = path
            .iter()
            .map(|blk| self.mir.blocks[*blk].name.as_ref())
            .flatten()
            .collect_vec();
        if names.is_empty() {
            return None;
        }
        if names.len() == 1 {
            return Some(names[0].into());
        }
        Some(format!("merged-{}", names.iter().join("-")))
    }

    pub fn run(mut self) -> MIR {
        self.compute_reachable();
        self.compute_successors();
        self.compute_predecessors();
        self.compute_paths();
        self.compute_path_bodies();

        self.add_parameters();
        todo!()
    }
}
