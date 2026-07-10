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

use std::collections::{BTreeSet, HashMap, HashSet};

use itertools::Itertools;

use crate::{
    Db,
    mir::{
        MIR, MIRBlockID, MIRLocalID,
        basic_block::{MIRTerminator, Stmt},
        builder::MIRBuilder,
        operand::{
            MIRConstructorArgs, MIROperand, MIRPlace, MIRProjection, MIRRValue,
            MIRRValueKind,
        },
        passes::MIRPass,
    },
    thir_to_mir::{_mir, FuncInst, MIRKey},
};

pub struct DeadCodeElimination;

impl MIRPass for DeadCodeElimination {
    fn run(&self, db: &dyn Db, mir: &MIR) -> MIR {
        DCECtx::new(db, mir).run()
    }
}

type OldBlockID = MIRBlockID;
type OldLocalID = MIRLocalID;

pub struct DCECtx<'a> {
    mir: &'a MIR,
    b: MIRBuilder<'a>,

    local_map: HashMap<OldLocalID, MIRLocalID>,
    reachable: BTreeSet<OldBlockID>,
    predecessors: HashMap<OldBlockID, HashSet<OldBlockID>>,
    successors: HashMap<OldBlockID, HashSet<OldBlockID>>,
}

struct PathCompressionRes {
    i: usize,
    j: usize,
    reversed: bool,
}

impl<'a> DCECtx<'a> {
    pub fn new(db: &'a dyn Db, mir: &'a MIR) -> Self {
        let entry_name = mir.blocks[mir.entry].name.clone();
        let b = MIRBuilder::new(db, entry_name);
        Self {
            mir,
            b,
            local_map: HashMap::new(),
            reachable: BTreeSet::new(),
            predecessors: HashMap::new(),
            successors: HashMap::new(),
        }
    }

    fn compute_reachable(&mut self) {
        let reachable = self.mir.compute_reachable(self.mir.entry);
        self.reachable = reachable.into_iter().collect();
    }

    fn add_and_get_local_to_mapping(
        &mut self,
        original: MIRLocalID,
    ) -> MIRLocalID {
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
        let (p1, p2, reversed) = {
            let p1_fst = p1.first().unwrap();
            let p2_last = p2.last().unwrap();
            if self.successors[p2_last].contains(p1_fst) {
                (p2, p1, true)
            } else {
                (p1, p2, false)
            }
        };

        let p1_last = *p1.last().unwrap();
        let p2_fst = *p2.first().unwrap();

        if !matches!(
            &self.mir.blocks[p1_last].terminator,
            MIRTerminator::Goto { .. },
        ) {
            return None;
        }

        if self.successors[&p1_last].len() != 1 {
            return None;
        }

        if self.predecessors[&p2_fst].len() != 1 {
            return None;
        }

        let p1_next = *self.successors[&p1_last].iter().next().unwrap();

        if p1_next != p2_fst {
            return None;
        }

        Some(reversed)
    }

    fn find_next_path_compression(
        &self,
        paths: &[Vec<OldBlockID>],
    ) -> Option<PathCompressionRes> {
        (0..paths.len())
            .flat_map(|i| (0..i).map(|j| (i, j)).collect_vec())
            .find_map(|(i, j)| {
                let reversed = self.can_compress_paths(&paths[i], &paths[j])?;
                Some(PathCompressionRes { i, j, reversed })
            })
    }

    fn compress_paths(
        &self,
        paths: &mut Vec<Vec<OldBlockID>>,
        i: usize,
        j: usize,
        reversed: bool,
    ) {
        if reversed {
            let p2 = paths.remove(i);
            paths[j].extend(p2);
        } else {
            let p2 = paths.remove(j);
            paths[i - 1].extend(p2);
        }
    }

    fn compute_paths(&mut self) -> Vec<Vec<OldBlockID>> {
        let mut paths =
            self.reachable.iter().map(|blk| vec![*blk]).collect_vec();
        while let Some(PathCompressionRes { i, j, reversed }) =
            self.find_next_path_compression(&paths)
        {
            self.compress_paths(&mut paths, i, j, reversed);
        }
        paths
    }

    fn compute_path_heads(
        &mut self,
        paths: &[Vec<OldBlockID>],
    ) -> HashMap<OldBlockID, usize> {
        paths
            .iter()
            .enumerate()
            .map(|(i, p)| (*p.first().unwrap(), i))
            .collect()
    }

    fn compute_successors(&mut self) {
        // TODO: Maybe don't do that :)
        let rs: HashSet<_> = self.reachable.iter().copied().collect();
        let succs = self.mir.successors().iter().filter_map(|(blk, succs)| {
            if rs.contains(blk) {
                Some((*blk, succs.intersection(&rs).copied().collect()))
            } else {
                None
            }
        });
        self.successors.extend(succs);
    }

    fn compute_predecessors(&mut self) {
        self.successors.iter().for_each(|(pred, succs)| {
            succs.iter().for_each(|succ| {
                self.predecessors.entry(*succ).or_default().insert(*pred);
            });
        });
        self.reachable.iter().for_each(|blk| {
            self.predecessors.entry(*blk).or_default();
        });
    }

    fn copy_rvalue(&mut self, rvalue: &MIRRValue) -> MIRRValue {
        let kind = match &rvalue.kind {
            MIRRValueKind::Use(op) => MIRRValueKind::Use(self.copy_operand(op)),
            MIRRValueKind::Ref(p, m) => {
                MIRRValueKind::Ref(self.copy_place(p), *m)
            }
            MIRRValueKind::AddressOf(p, m) => {
                MIRRValueKind::AddressOf(self.copy_place(p), *m)
            }
            MIRRValueKind::BinOp(bop, l, r) => MIRRValueKind::BinOp(
                *bop,
                self.copy_operand(l),
                self.copy_operand(r),
            ),
            MIRRValueKind::UnaryOp(uop, op) => {
                MIRRValueKind::UnaryOp(*uop, self.copy_operand(op))
            }
            MIRRValueKind::Discriminant(p) => {
                MIRRValueKind::Discriminant(self.copy_place(p))
            }
            MIRRValueKind::Metadata(op) => {
                MIRRValueKind::Metadata(self.copy_operand(op))
            }
            MIRRValueKind::SizeOf(t) => MIRRValueKind::SizeOf(*t),
            MIRRValueKind::Constructor { enum_ref, idx, args, span } => {
                MIRRValueKind::Constructor {
                    enum_ref: enum_ref.clone(),
                    idx: *idx,
                    args: self.copy_cons_args(args),
                    span: *span,
                }
            }
            MIRRValueKind::StructLit { struct_ref, fields, span } => {
                MIRRValueKind::StructLit {
                    struct_ref: struct_ref.clone(),
                    fields: fields
                        .iter()
                        .map(|f| (*f.0, self.copy_operand(f.1)))
                        .collect(),
                    span: *span,
                }
            }
            MIRRValueKind::Tuple(ops, span) => MIRRValueKind::Tuple(
                ops.iter().map(|op| self.copy_operand(op)).collect(),
                *span,
            ),
        };
        MIRRValue { kind, ty: rvalue.ty, span: rvalue.span }
    }

    fn copy_stmt(&mut self, stmt: &Stmt) -> Stmt {
        match stmt {
            Stmt::Assign { dest, rvalue } => Stmt::Assign {
                dest: self.copy_place(dest),
                rvalue: self.copy_rvalue(rvalue),
            },
        }
    }

    fn copy_projection(&mut self, proj: &MIRProjection) -> MIRProjection {
        match proj {
            MIRProjection::Deref => MIRProjection::Deref,
            MIRProjection::Field { name, resulting_ty } => {
                MIRProjection::Field {
                    name: *name,
                    resulting_ty: *resulting_ty,
                }
            }
            MIRProjection::TupleField { index, resulting_ty } => {
                MIRProjection::TupleField {
                    index: *index,
                    resulting_ty: *resulting_ty,
                }
            }
            MIRProjection::Index { index } => {
                MIRProjection::Index { index: self.copy_operand(index) }
            }
            MIRProjection::Downcast { variant } => {
                MIRProjection::Downcast { variant: *variant }
            }
        }
    }

    fn copy_place(&mut self, place: &MIRPlace) -> MIRPlace {
        MIRPlace {
            local: self.add_and_get_local_to_mapping(place.local),
            projections: place
                .projections
                .iter()
                .map(|proj| self.copy_projection(proj))
                .collect(),
            ty: place.ty,
            span: place.span,
        }
    }

    fn copy_cons_args(
        &mut self,
        args: &MIRConstructorArgs,
    ) -> MIRConstructorArgs {
        match args {
            MIRConstructorArgs::None => MIRConstructorArgs::None,
            MIRConstructorArgs::Tuple(ops) => MIRConstructorArgs::Tuple(
                ops.iter().map(|op| self.copy_operand(op)).collect(),
            ),
            MIRConstructorArgs::Struct(fields) => MIRConstructorArgs::Struct(
                fields
                    .iter()
                    .map(|(key, val)| (*key, self.copy_operand(val)))
                    .collect(),
            ),
        }
    }

    fn copy_operand(&mut self, operand: &MIROperand) -> MIROperand {
        match operand {
            MIROperand::Constant(cst, span) => {
                MIROperand::Constant(cst.clone(), *span)
            }
            MIROperand::Move(p) => MIROperand::Move(self.copy_place(p)),
            MIROperand::Copy(p) => MIROperand::Copy(self.copy_place(p)),
        }
    }

    fn get_bb(
        &self,
        old: OldBlockID,
        bbs: &[MIRBlockID],
        path_heads: &HashMap<OldBlockID, usize>,
    ) -> MIRBlockID {
        bbs[path_heads[&old]]
    }

    fn copy_terminator(
        &mut self,
        terminator: &MIRTerminator,
        bbs: &[MIRBlockID],
        path_heads: &HashMap<OldBlockID, usize>,
    ) -> MIRTerminator {
        match terminator {
            MIRTerminator::Diverge => MIRTerminator::Diverge,
            MIRTerminator::Call { callee, arguments, dest, next, span } => {
                MIRTerminator::Call {
                    callee: callee.clone(),
                    arguments: arguments
                        .iter()
                        .map(|operand| self.copy_operand(operand))
                        .collect(),
                    dest: self.add_and_get_local_to_mapping(*dest),
                    next: self.get_bb(*next, bbs, path_heads),
                    span: *span,
                }
            }
            MIRTerminator::Return { value, span } => MIRTerminator::Return {
                value: value.as_ref().map(|operand| self.copy_operand(operand)),
                span: *span,
            },
            MIRTerminator::Goto { next } => MIRTerminator::Goto {
                next: self.get_bb(*next, bbs, path_heads),
            },
            MIRTerminator::Branch { cond, then, else_, span } => {
                MIRTerminator::Branch {
                    cond: self.copy_operand(cond),
                    then: self.get_bb(*then, bbs, path_heads),
                    else_: self.get_bb(*else_, bbs, path_heads),
                    span: *span,
                }
            }
            MIRTerminator::Switch { discriminant, branches, default, span } => {
                MIRTerminator::Switch {
                    discriminant: self.copy_operand(discriminant),
                    branches: branches
                        .iter()
                        .map(|br| (*br.0, self.get_bb(*br.1, bbs, path_heads)))
                        .collect(),
                    default: self.get_bb(*default, bbs, path_heads),
                    span: *span,
                }
            }
        }
    }

    fn compute_path_bodies(&mut self, paths: Vec<Vec<OldBlockID>>) {
        let path_heads = self.compute_path_heads(&paths);
        let bbs = paths
            .iter()
            .map(|p| {
                assert!(!p.is_empty());
                let name = self.salvage_path_name(p);
                if *p.first().unwrap() == self.mir.entry {
                    self.b.blocks[self.b.entry].name = name;
                    self.b.entry
                } else {
                    self.b.new_block(name)
                }
            })
            .collect_vec();

        for (p, bb) in paths.iter().zip_eq(&bbs) {
            self.b.switch_to_block(*bb).unwrap();
            let stmts = p
                .iter()
                .flat_map(|blk| self.mir.blocks[*blk].stmts.clone())
                .collect_vec();
            stmts.iter().for_each(|stmt| {
                let new_stmt = self.copy_stmt(stmt);
                self.b.emit(new_stmt);
            });
            let terminator = &self.mir.blocks[*p.last().unwrap()].terminator;
            let new_terminator =
                self.copy_terminator(terminator, &bbs, &path_heads);
            self.b.terminate(new_terminator).unwrap();
        }
    }

    fn salvage_path_name(&self, path: &[OldBlockID]) -> Option<String> {
        let names = path
            .iter()
            .filter_map(|blk| self.mir.blocks[*blk].name.as_ref())
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
        self.add_parameters();

        self.compute_reachable();
        self.compute_successors();
        self.compute_predecessors();

        let paths = self.compute_paths();
        self.compute_path_bodies(paths);
        self.b.finalize(self.mir.func).unwrap()
    }
}

#[salsa::tracked(returns(ref))]
fn _dce<'db>(db: &'db dyn Db, key: MIRKey<'db>) -> MIR {
    let mir = _mir(db, key);
    DeadCodeElimination.run(db, mir)
}

pub fn dce(db: &dyn Db, f: FuncInst) -> &MIR {
    _dce(db, f.interned())
}
