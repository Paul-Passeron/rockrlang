use std::collections::HashMap;

use itertools::Itertools;

use crate::{
    Db,
    common::{location::Span, symbols::Symbol},
    compiler::get_sig_of_function,
    name_resolve::type_expr::struct_item,
    ril::{
        ScopeOwnerId, TypeRef, bool_id, char_id, const_ptr_of, ref_of, str_id,
        tuple_of, void_id,
    },
    thir::{
        ExprId, ExprKind, FunctionRef, PlaceBase, PlaceId, Projection,
        StructRef, Thir, ThirExprWithSetup, ThirMatchBranch, ThirPattern,
        ThirPatternKind,
        stmt::{StmtKind, ThirStmt},
    },
    typecheck::inference::implicit::AstImplicitContext,
};

pub struct SanityError {
    pub expected: TypeRef,
    pub got: TypeRef,
    pub span: Span,
}

pub struct SanityChecker<'db> {
    pub db: &'db dyn Db,
    pub thir: &'db Thir,
    pub errs: Vec<SanityError>,
}

impl<'db> SanityChecker<'db> {
    pub fn new(db: &'db dyn Db, thir: &'db Thir) -> Self {
        Self {
            db,
            thir,
            errs: Vec::new(),
        }
    }

    pub fn check(mut self) -> Vec<SanityError> {
        self.check_stmts(&self.thir.root);
        self.errs
    }

    fn check_stmts(&mut self, stmts: &[ThirStmt]) {
        stmts.iter().for_each(|stmt| self.check_stmt(stmt));
    }

    fn expr_ty(&self, expr: ExprId) -> TypeRef {
        self.thir.exprs[expr].ty
    }

    fn check_stmt(&mut self, stmt: &ThirStmt) {
        match &stmt.kind {
            StmtKind::Block { stmts, .. } => self.check_stmts(stmts),
            StmtKind::If {
                cond, then, else_, ..
            } => {
                let bool_ty = TypeRef::Concrete(bool_id(self.db));
                let ThirExprWithSetup { stmts, expr } = cond;
                self.check_stmts(stmts);
                self.check_expr_with_expected_type(*expr, bool_ty);
                self.check_stmts(then);
                if let Some(body) = else_ {
                    self.check_stmts(body);
                }
            }
            StmtKind::While { cond, body, .. } => {
                let bool_ty = TypeRef::Concrete(bool_id(self.db));
                let ThirExprWithSetup { stmts, expr } = cond;
                self.check_stmts(stmts);
                self.check_expr_with_expected_type(*expr, bool_ty);
                self.check_stmts(body);
            }
            StmtKind::Let { local, init } => {
                let local_ty = self.thir.locals[*local].ty;
                self.check_expr_with_expected_type(*init, local_ty);
            }
            StmtKind::Assign { place, rhs } => {
                self.check_place(*place);
                let place_ty = self.thir.places[*place].ty;
                self.check_expr_with_expected_type(*rhs, place_ty);
            }
            StmtKind::Return(expr) => {
                self.check_return(*expr, stmt.span);
            }
            StmtKind::Match {
                scrutinee,
                branches,
            } => {
                let ThirExprWithSetup { stmts, expr } = scrutinee;
                self.check_stmts(stmts);
                self.check_expr(*expr);
                let scrut_ty = self.expr_ty(*expr);
                branches
                    .iter()
                    .for_each(|branch| self.check_branch(scrut_ty, branch));
            }
            StmtKind::Expr(expr) => self.check_expr(*expr),
            StmtKind::Break(_) | StmtKind::Continue(_) | StmtKind::Error => (),
        }
    }

    fn check_pattern_against_scrut_ty(
        &mut self,
        scrut_ty: TypeRef,
        pat: &ThirPattern,
    ) {
        if let Some((mutability, inner_type)) = scrut_ty.as_ref(self.db) {
            todo!()
        } else {
            self.check_pattern_with_expected_ty(pat, scrut_ty)
        }
    }

    fn check_pattern_with_expected_ty(
        &mut self,
        pat: &ThirPattern,
        expected: TypeRef,
    ) {
        self.check_pattern(pat);
        self.check_types(expected, pat.ty, pat.span);
    }

    fn check_pattern(&mut self, pat: &ThirPattern) {
        match &pat.kind {
            ThirPatternKind::Error
            | ThirPatternKind::Any
            | ThirPatternKind::Bind { .. } => (),
            ThirPatternKind::Tuple(thir_patterns) => {
                let tys = thir_patterns
                    .iter()
                    .map(|pat| {
                        self.check_pattern(pat);
                        pat.ty
                    })
                    .collect_vec();
                let expected = TypeRef::Concrete(tuple_of(self.db, tys));
                self.check_types(expected, pat.ty, pat.span);
            }
            ThirPatternKind::Struct { def, fields } => {
                // All fields should be here, this is checked previously
                let actual_fields = def.get_fields_ty(self.db);
                for (symbol, thir_pattern) in fields {
                    let actual_ty = actual_fields[symbol];
                    self.check_types(
                        actual_ty,
                        thir_pattern.ty,
                        thir_pattern.span,
                    );
                }
            }
            ThirPatternKind::Constructor { def, idx, args } => {
                todo!()
            }
            ThirPatternKind::IntLit(_) => {
                // TODO: Do something here
            }
        }
    }

    fn check_branch(&mut self, scrut_ty: TypeRef, branch: &ThirMatchBranch) {
        self.check_pattern_against_scrut_ty(scrut_ty, &branch.pattern);
        if let Some(guard) = &branch.guard {
            self.check_stmts(&guard.stmts);
            self.check_expr_with_expected_type(
                guard.expr,
                TypeRef::Concrete(bool_id(self.db)),
            );
        }
        self.check_stmts(&branch.body);
    }

    fn check_expr(&mut self, expr: ExprId) {
        let infos = &self.thir.exprs[expr];
        match &infos.kind {
            ExprKind::SizeOf(_) | ExprKind::IntLit(_) => {
                // TODO
            }
            ExprKind::Charlit(_) => self.check_types(
                TypeRef::Concrete(char_id(self.db)),
                infos.ty,
                infos.span,
            ),
            ExprKind::StrLit(_) => self.check_types(
                TypeRef::Concrete(str_id(self.db)),
                infos.ty,
                infos.span,
            ),
            ExprKind::CStrLit(_) => self.check_types(
                TypeRef::Concrete(const_ptr_of(
                    self.db,
                    TypeRef::Concrete(char_id(self.db)),
                )),
                infos.ty,
                infos.span,
            ),
            ExprKind::BoolLit(_) => self.check_types(
                TypeRef::Concrete(bool_id(self.db)),
                infos.ty,
                infos.span,
            ),
            ExprKind::Use(place) => {
                self.check_place(*place);
                let place_ty = self.thir.places[*place].ty;
                self.check_types(infos.ty, place_ty, infos.span);
            }
            ExprKind::AddressOf { place, mutability } => todo!(),
            ExprKind::Ref { place, mutability } => {
                self.check_place(*place);
                let place_ty = self.thir.places[*place].ty;
                let ref_ty = TypeRef::Concrete(ref_of(
                    self.db,
                    place_ty,
                    mutability.is_mut(),
                ));
                self.check_types(infos.ty, ref_ty, infos.span);
            }
            ExprKind::Call { called, args } => {
                let args_ty = args
                    .iter()
                    .map(|arg| {
                        self.check_expr(*arg);
                        (self.thir.exprs[*arg].ty, *arg)
                    })
                    .collect_vec();
                let ret = called.ret_ty(self.db);
                let params = called.params(self.db);
                params.into_iter().zip(args_ty).for_each(
                    |((_, param), (arg_ty, arg))| {
                        self.check_types(
                            param,
                            arg_ty,
                            self.thir.exprs[arg].span,
                        )
                    },
                );
                self.check_types(ret, infos.ty, infos.span);
            }
            ExprKind::BinOp { op, lhs, rhs } => todo!(),
            ExprKind::StructLit { struct_def, fields } => todo!(),
            ExprKind::Neg(idx) => todo!(),
            ExprKind::Not(idx) => todo!(),
            ExprKind::Tuple(items) => {
                let tys = items
                    .iter()
                    .map(|expr| {
                        self.check_expr(*expr);
                        self.thir.exprs[*expr].ty
                    })
                    .collect_vec();
                let expected = TypeRef::Concrete(tuple_of(self.db, tys));
                self.check_types(expected, infos.ty, infos.span);
            }
            ExprKind::SliceLit(items) => todo!(),
            ExprKind::Constructor {
                enum_def,
                idx,
                args,
            } => todo!(),
            ExprKind::Error => (),
        }
    }

    fn check_types(&mut self, expected: TypeRef, got: TypeRef, span: Span) {
        if expected != got {
            self.errs.push(SanityError {
                expected,
                got,
                span,
            });
        }
    }

    fn check_projection(
        &mut self,
        before: TypeRef,
        projection: &Projection,
        expected: TypeRef,
        span: Span,
    ) {
        if let Some(ty) =
            self.compute_type_after_projection(before, projection, span)
        {
            self.check_types(expected, ty, span);
        } else {
            todo!()
        }
    }

    fn check_place(&mut self, place: PlaceId) {
        let info = &self.thir.places[place];
        let base_ty = match info.base {
            PlaceBase::Local(idx) => self.thir.locals[idx].ty,
        };
        let end_ty =
            info.projections.iter().fold(base_ty, |before, projection| {
                let expected = match projection {
                    Projection::Field(_, type_ref)
                    | Projection::TupleField(_, type_ref) => Some(*type_ref),
                    _ => None,
                };
                let expected = expected
                    .or_else(|| {
                        self.compute_type_after_projection(
                            before, projection, info.span,
                        )
                    })
                    .unwrap_or(TypeRef::Error);
                self.check_projection(before, projection, expected, info.span);
                expected
            });
        self.check_types(end_ty, info.ty, info.span);
    }

    pub fn check_return(&mut self, expr: Option<ExprId>, span: Span) {
        let ret_ty = self.thir.get_ret_ty(self.db);

        match expr {
            Some(expr) => {
                self.check_expr_with_expected_type(expr, ret_ty);
            }
            None => {
                let void_ty = TypeRef::Concrete(void_id(self.db));
                self.check_types(ret_ty, void_ty, span);
            }
        }
    }

    pub fn check_expr_with_expected_type(
        &mut self,
        expr: ExprId,
        expected: TypeRef,
    ) {
        self.check_expr(expr);
        let (got, span) = {
            let expr = &self.thir.exprs[expr];
            (expr.ty, expr.span)
        };
        self.check_types(expected, got, span);
    }

    fn compute_type_after_projection(
        &mut self,
        before: TypeRef,
        projection: &Projection,
        span: Span,
    ) -> Option<TypeRef> {
        match projection {
            Projection::Deref => {
                let before = before.as_type_id()?;
                if before.def(self.db).is_ptr_like(self.db).is_none() {
                    None
                } else {
                    let mut args = before.args(self.db);
                    assert_eq!(args.len(), 1);
                    args.pop()
                }
            }
            Projection::Field(symbol, type_ref) => {
                let struct_ref = before.as_struct_ref(self.db)?;
                let typeof_field = struct_ref.typeof_field(self.db, *symbol)?;
                self.check_types(typeof_field, *type_ref, span);
                Some(typeof_field)
            }
            Projection::TupleField(_, type_ref) => todo!(),
            Projection::Index(idx) => todo!(),
        }
    }
}

pub fn sanity_check(db: &dyn Db, thir: &Thir) {
    let mismatches = SanityChecker::new(db, thir).check();
    assert!(mismatches.is_empty())
}

impl StructRef {
    pub fn get_fields_ty(&self, db: &dyn Db) -> HashMap<Symbol, TypeRef> {
        let item = struct_item(db, self.def.interned());
        let mut res = HashMap::new();
        for field in &item.fields {
            let ctx = AstImplicitContext::new(
                db,
                ScopeOwnerId::Module(self.def.parent(db)),
                item.template_args.iter().cloned().collect(),
            )
            .unwrap();
            let type_ref = ctx
                .resolve(db, &field.ty.data)
                .unwrap_or(TypeRef::Error)
                .with_substitution(db, &self.args);
            res.insert(field.name, type_ref);
        }
        res
    }
}

impl FunctionRef {
    pub fn ret_ty(&self, db: &dyn Db) -> TypeRef {
        let sig = get_sig_of_function(db, self.id.interned());
        sig.ret.with_substitution(db, &self.args)
    }

    pub fn params(&self, db: &dyn Db) -> Vec<(Symbol, TypeRef)> {
        let sig = get_sig_of_function(db, self.id.interned());
        sig.args
            .iter()
            .map(|(symb, ty)| (*symb, ty.with_substitution(db, &self.args)))
            .collect()
    }
}
