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

use std::{collections::BTreeMap, iter::repeat_n};

use itertools::Itertools;

use crate::{
    Db,
    check::thir::sanity_check::{RefWrappedTy, WrapKind},
    layout::{Discriminant, IntWidth, LIRTy, layout_of},
    mir::{
        MIRBlockID,
        basic_block::{MIRTerminator, Stmt},
        operand::{MIRPlace, MIRProjection, MIRRValue, MIRRValueKind},
    },
    resolved::{TypeDefId, TypeId, TypeRef, char_id, int_id, ref_of, usize_id},
    thir::{EnumRef, StructRef, ThirMatchBranch},
    thir_to_mir::{
        ThirToMIR,
        decision_tree::{Constructor, DecisionTree, Matrix, Row},
    },
};

pub(super) struct MatchLowerer<'a, 'b> {
    db: &'b dyn Db,
    ctx: &'a mut ThirToMIR<'b>,
    scrut: MIRPlace,
    merge_bb: MIRBlockID,
}

impl<'a, 'b> MatchLowerer<'a, 'b> {
    pub(super) fn new(
        ctx: &'a mut ThirToMIR<'b>,
        scrut: MIRPlace,
        merge_bb: MIRBlockID,
    ) -> Self {
        Self { db: ctx.db, ctx, scrut, merge_bb }
    }

    fn build_match_matrix(&self, branches: &'a [ThirMatchBranch]) -> Matrix<'a> {
        let rows = branches
            .iter()
            .enumerate()
            .map(|(idx, branch)| -> Row<'a> {
                assert!(branch.guard.is_none()); // TODO
                Row {
                    pats: vec![Some(&branch.pattern)],
                    branch_idx: idx,
                    bindings: vec![],
                }
            })
            .collect();

        Matrix { cols: vec![self.scrut.clone()], rows }
    }

    fn lower_dt_aux(
        &mut self,
        dt: &DecisionTree,
        bbs: &[MIRBlockID],
        diverge: MIRBlockID,
    ) {
        match dt {
            DecisionTree::Leaf { branch_idx, bindings } => {
                for (local, place) in bindings {
                    let ty = self.ctx.builder.locals[*local].ty;
                    let rvalue = self.ctx.wrap_ref_to_fit(ty, place);
                    self.ctx.builder.emit(Stmt::Assign {
                        dest: self.ctx.place_of_local(*local, place.span),
                        rvalue,
                    });
                }
                self.ctx.goto(bbs[*branch_idx]);
            }
            DecisionTree::Switch { place, cases, default } => {
                let wrapped = RefWrappedTy::from_type_ref(self.db, place.ty);
                let ty = wrapped.inner;
                let depth = wrapped.depth();
                if ty.as_enum_ref(self.db).is_some() {
                    let mut scrut_place = place.clone();
                    scrut_place.projections.extend(repeat_n(MIRProjection::Deref, depth));
                    scrut_place.ty = ty;
                    let span = self.ctx.builder.locals[place.local].span;

                    let layout = layout_of(self.db, ty);
                    let lir_ty = LIRTy { layout, origin: Some(ty) };
                    let vlayout = lir_ty
                        .union_layout(self.db)
                        .expect("If this is not a variant layout, this is a bug");
                    let Discriminant::Tagged { kind: discr_width, .. } =
                        vlayout.discriminant
                    else {
                        todo!()
                    };

                    let discr_ty: TypeRef =
                        int_ty_with_witdh(self.db, discr_width).into();

                    let discr = MIRRValue {
                        kind: MIRRValueKind::Discriminant(scrut_place),
                        ty: discr_ty,
                        span,
                    };
                    let discr_place = self.ctx.synthetic_place(discr_ty, span);
                    self.ctx
                        .builder
                        .emit(Stmt::Assign { dest: discr_place.clone(), rvalue: discr });

                    let current = self.ctx.builder.current_block();

                    let branches: BTreeMap<u128, MIRBlockID> = cases
                        .iter()
                        .map(|(ctor, next_dec_tree)| {
                            let Constructor::Variant(variant_idx) = ctor else {
                                unreachable!()
                            };
                            let next_dec_tree_bb = self.ctx.builder.new_block(None);
                            self.ctx.switch_to(next_dec_tree_bb);
                            self.lower_dt_aux(next_dec_tree, bbs, diverge);
                            (*variant_idx as u128, next_dec_tree_bb)
                        })
                        .collect();

                    let default = if let Some(default) = default {
                        let default_bb = self.ctx.builder.new_block(None);
                        self.ctx.switch_to(default_bb);
                        self.lower_dt_aux(default, bbs, diverge);
                        default_bb
                    } else {
                        diverge
                    };

                    self.ctx.switch_to(current);
                    self.ctx.build_terminator(MIRTerminator::Switch {
                        discriminant: discr_place.into_move(),
                        branches,
                        default,
                        span,
                    });
                } else if ty
                    .as_type_id()
                    .is_some_and(|ty| ty.def(self.db).is_int_like(self.db).is_some())
                {
                    let mut scrut_place = place.clone();
                    scrut_place.projections.extend(repeat_n(MIRProjection::Deref, depth));
                    let current = self.ctx.builder.current_block();

                    let branches: BTreeMap<u128, MIRBlockID> = cases
                        .iter()
                        .map(|(ctor, next_dec_tree)| {
                            let Constructor::IntLit(value) = ctor else { unreachable!() };
                            let next_dec_tree_bb = self.ctx.builder.new_block(None);
                            self.ctx.switch_to(next_dec_tree_bb);
                            self.lower_dt_aux(next_dec_tree, bbs, diverge);
                            (*value as u128, next_dec_tree_bb)
                        })
                        .collect();

                    let default = if let Some(default) = default {
                        let default_bb = self.ctx.builder.new_block(None);
                        self.ctx.switch_to(default_bb);
                        self.lower_dt_aux(default, bbs, diverge);
                        default_bb
                    } else {
                        diverge
                    };

                    self.ctx.switch_to(current);
                    let span = scrut_place.span;
                    self.ctx.build_terminator(MIRTerminator::Switch {
                        discriminant: scrut_place.into_move(),
                        branches,
                        default,
                        span,
                    });
                } else {
                    todo!("Ty is {}", ty.to_string(self.db))
                }
            }
            DecisionTree::Fail => {
                if !self.ctx.current_block_is_terminated() {
                    self.ctx.build_terminator(MIRTerminator::Diverge);
                }
            }
        }
    }

    fn lower_decision_tree(&mut self, dt: &DecisionTree, branches: &[ThirMatchBranch]) {
        let branch_bodies = (0..branches.len())
            .map(|idx| self.ctx.builder.new_block(Some(format!("match-branch-{idx}"))))
            .collect_vec();
        let diverge = self.ctx.builder.new_block(Some("match-diverge".into()));
        self.lower_dt_aux(dt, &branch_bodies, diverge);
        for (bb, branch) in branch_bodies.into_iter().zip(branches) {
            self.ctx
                .builder
                .switch_to_block(bb)
                .expect("This block should not be terminated");
            self.ctx.build_stmts(&branch.body);
            if !self.ctx.current_block_is_terminated() {
                self.ctx.goto(self.merge_bb);
            }
        }
        self.ctx.switch_to(diverge);
        self.ctx.build_terminator(MIRTerminator::Diverge);
        self.ctx.switch_to(self.merge_bb);
    }

    pub(super) fn lower(&mut self, branches: &'a [ThirMatchBranch]) {
        let matrix = self.build_match_matrix(branches);
        let decision_tree = matrix.compile(self.ctx);
        self.lower_decision_tree(&decision_tree, branches);
    }
}

impl EnumRef {
    pub fn into_type_ref(self, db: &dyn Db) -> TypeRef {
        TypeId::new(db, TypeDefId::Enum(self.def), self.args).into()
    }
}

impl StructRef {
    pub fn into_type_ref(self, db: &dyn Db) -> TypeRef {
        TypeId::new(db, TypeDefId::Struct(self.def), self.args).into()
    }
}

impl ThirToMIR<'_> {
    pub fn wrap_ref_to_fit(&mut self, target: TypeRef, place: &MIRPlace) -> MIRRValue {
        let span = self.builder.locals[place.local].span;
        if place.ty == target || target.as_ref(self.db).is_none() {
            return MIRRValue {
                kind: MIRRValueKind::Use(self.move_or_copy(place.clone())),
                ty: place.ty,
                span,
            };
        }

        let RefWrappedTy { mut refs, .. } =
            RefWrappedTy::peel_until(self.db, target, place.ty)
                .unwrap_or_else(|| RefWrappedTy::from_type_ref(self.db, place.ty));
        if refs.is_empty() {
            // TODO: weird ???
            return MIRRValue {
                kind: MIRRValueKind::Use(self.move_or_copy(place.clone())),
                ty: place.ty,
                span,
            };
        }
        let WrapKind::Ref(inital) = refs.remove(0);
        let mut res = MIRRValue {
            kind: MIRRValueKind::Ref(place.clone(), inital),
            ty: ref_of(self.db, place.ty, inital.is_mut()).into(),
            span,
        };
        for w in &refs {
            let WrapKind::Ref(mutability) = w;
            let place = self.synthetic_place(res.ty, span);
            let new_ty = ref_of(self.db, place.ty, mutability.is_mut());
            self.assign(place.clone(), res);
            res = MIRRValue {
                kind: MIRRValueKind::Ref(place, *mutability),
                ty: new_ty.into(),
                span,
            };
        }
        res
    }
}

pub fn int_ty_with_witdh(db: &dyn Db, width: IntWidth) -> TypeId {
    match width {
        IntWidth::I8 => char_id(db),
        IntWidth::I16 => todo!(),
        IntWidth::I32 => int_id(db),
        IntWidth::I64 => usize_id(db),
        IntWidth::I128 => todo!(),
    }
}
