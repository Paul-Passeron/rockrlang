use crate::{
    Db,
    common::location::Span,
    ril::{TypeRef, bool_id, void_id},
    thir::{
        ExprId, PlaceId, Thir, ThirExprWithSetup, ThirMatchBranch,
        stmt::{StmtKind, ThirStmt},
    },
};

pub struct SanityError {
    pub expected: TypeRef,
    pub got: TypeRef,
    pub span: Span,
}

pub struct SanityChecker<'db> {
    pub errs: Vec<SanityError>,
    pub db: &'db dyn Db,
    pub thir: &'db Thir,
}

impl<'db> SanityChecker<'db> {
    pub fn new(db: &'db dyn Db, thir: &'db Thir) -> Self {
        Self {
            errs: Vec::new(),
            db,
            thir,
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

    fn check_branch(&mut self, scrut_ty: TypeRef, branch: &ThirMatchBranch) {
        todo!()
    }

    fn check_expr(&mut self, expr: ExprId) {
        todo!()
    }

    fn check_place(&mut self, place: PlaceId) {
        todo!()
    }

    pub fn check_return(&mut self, expr: Option<ExprId>, span: Span) {
        let ret_ty = self.thir.get_ret_ty(self.db);

        match expr {
            Some(expr) => {
                self.check_expr_with_expected_type(expr, ret_ty);
            }
            None => {
                let void_ty = TypeRef::Concrete(void_id(self.db));
                if ret_ty != void_ty {
                    self.errs.push(SanityError {
                        expected: ret_ty,
                        got: void_ty,
                        span: span,
                    });
                }
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
        if expected != got {
            self.errs.push(SanityError {
                expected,
                got,
                span,
            });
        }
    }
}

pub fn sanity_check(db: &dyn Db, thir: &Thir) {
    let mismatches = SanityChecker::new(db, thir).check();
    assert!(mismatches.is_empty())
}
