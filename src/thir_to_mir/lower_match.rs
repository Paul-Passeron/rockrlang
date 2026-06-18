use std::collections::BTreeMap;

use itertools::Itertools;

use crate::{
    Db,
    mir::{
        MIRBlockID,
        basic_block::{MIRTerminator, Stmt},
        operand::{MIRPlace, MIRProjection, MIRRValue, MIRRValueKind},
    },
    ril::{TypeDefId, TypeId, TypeRef, int_id},
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
    pub fn new(
        ctx: &'a mut ThirToMIR<'b>,
        scrut: MIRPlace,
        merge_bb: MIRBlockID,
    ) -> Self {
        Self {
            db: ctx.db,
            ctx,
            scrut,
            merge_bb,
        }
    }

    fn build_match_matrix(&mut self, branches: &'a [ThirMatchBranch]) -> Matrix<'a> {
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

        Matrix {
            cols: vec![self.scrut.clone()],
            rows,
        }
    }

    fn _lower_dt(&mut self, dt: &DecisionTree, bbs: &[MIRBlockID], diverge: MIRBlockID) {
        match dt {
            DecisionTree::Leaf {
                branch_idx,
                bindings,
            } => {
                for (local, place) in bindings {
                    let ty = self.ctx.builder.locals[*local].ty;
                    let rvalue = self.ctx.wrap_ref_to_fit(ty, place);
                    self.ctx.builder.emit(Stmt::Assign {
                        dest: self.ctx.place_of_local(*local),
                        rvalue,
                    });
                }
                self.ctx.goto(bbs[*branch_idx]);
            }
            DecisionTree::Switch {
                place,
                cases,
                default,
            } => {
                let (ty, depth) = place.ty.peel_aux(self.db);
                if ty.as_enum_ref(self.db).is_some() {
                    let mut scrut_place = place.clone();
                    scrut_place
                        .projections
                        .extend(std::iter::repeat(MIRProjection::Deref).take(depth));
                    scrut_place.ty = ty;
                    let span = self.ctx.builder.locals[place.local].span;
                    let discr = MIRRValue {
                        kind: MIRRValueKind::Discriminant(scrut_place),
                        ty: int_id(self.db).into(),
                        span,
                    };
                    let discr_place = self.ctx.synthetic_place(ty, span);
                    self.ctx.builder.emit(Stmt::Assign {
                        dest: discr_place.clone(),
                        rvalue: discr,
                    });

                    let current = self.ctx.builder.current_block();

                    let branches: BTreeMap<u128, MIRBlockID> = cases
                        .iter()
                        .map(|(ctor, next_dec_tree)| {
                            let Constructor::Variant(variant_idx) = ctor else {
                                unreachable!()
                            };
                            let next_dec_tree_bb = self.ctx.builder.new_block(None);
                            self.ctx.switch_to(next_dec_tree_bb);
                            self._lower_dt(next_dec_tree, bbs, diverge);
                            (*variant_idx as u128, next_dec_tree_bb)
                        })
                        .collect();

                    let default = if let Some(default) = default {
                        let default_bb = self.ctx.builder.new_block(None);
                        self.ctx.switch_to(default_bb);
                        self._lower_dt(default, bbs, diverge);
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
                } else {
                    todo!()
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
            .map(|idx| {
                self.ctx
                    .builder
                    .new_block(Some(format!("match-branch-{idx}")))
            })
            .collect_vec();
        let diverge = self.ctx.builder.new_block(Some("match-diverge".into()));
        self._lower_dt(dt, &branch_bodies, diverge);
        for (bb, branch) in branch_bodies.into_iter().zip(branches) {
            self.ctx.builder.switch_to_block(bb).unwrap();
            self.ctx.build_stmts(&branch.body);
            if !self.ctx.current_block_is_terminated() {
                self.ctx.goto(self.merge_bb);
            }
        }
        self.ctx.switch_to(diverge);
        self.ctx.build_terminator(MIRTerminator::Diverge);
        self.ctx.switch_to(self.merge_bb);
    }

    pub fn lower(&mut self, branches: &'a [ThirMatchBranch]) {
        let matrix = self.build_match_matrix(branches);
        let decision_tree = matrix.compile(self.ctx);
        self.lower_decision_tree(&decision_tree, branches);
    }
}

impl EnumRef {
    pub fn as_type_ref(self, db: &dyn Db) -> TypeRef {
        TypeId::new(db, TypeDefId::Enum(self.def), self.args).into()
    }
}

impl StructRef {
    pub fn as_type_ref(self, db: &dyn Db) -> TypeRef {
        TypeId::new(db, TypeDefId::Struct(self.def), self.args).into()
    }
}

impl<'a> ThirToMIR<'a> {
    pub fn wrap_ref_to_fit(&mut self, target: TypeRef, place: &MIRPlace) -> MIRRValue {
        let span = self.builder.locals[place.local].span;
        if place.ty == target {
            return MIRRValue {
                kind: MIRRValueKind::Use(self.move_or_copy(place.clone())),
                ty: place.ty,
                span,
            };
        }
        todo!()
    }
}
