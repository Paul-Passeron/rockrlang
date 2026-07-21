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

use std::collections::{HashMap, HashSet};

use itertools::Itertools;
use salsa::Accumulator;

use crate::{
    Db,
    common::{location::Span, symbols::Symbol},
    compiler::{diagnostic::Diag, get_sig_of_function},
    hir::Mutability,
    name_resolve::type_expr::{enum_item, struct_item},
    parse_tree::{expr::BinaryOperator, top_level::AstEnumVariantKind},
    ril::{
        BuiltinTypeKind, ScopeOwnerId, TypeDefId, TypeId, TypeRef, bool_id, char_id,
        const_ptr_of, int_id, ptr_of, ref_of, slice_of, str_id, tuple_of, usize_id,
        void_id,
    },
    thir::{
        EnumRef, ExprId, ExprKind, FunctionRef, PlaceBase, PlaceId, Projection,
        StructRef, Thir, ThirConstructorArgs, ThirExprWithSetup, ThirMatchBranch,
        ThirPattern, ThirPatternKind,
        stmt::{StmtKind, ThirStmt},
    },
    typecheck::inference::implicit::{AsAstImplCtx, AstImplicitContext},
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
        Self { db, thir, errs: Vec::new() }
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
            StmtKind::If { cond, then, else_, .. } => {
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
                let (local_ty, span) = {
                    let loc = &self.thir.locals[*local];
                    (loc.ty, loc.span)
                };
                let ty = self.check_expr(*init);
                self.check_types(ty, local_ty, span);
            }
            StmtKind::Assign { place, rhs } => {
                self.check_place(*place);
                let place_ty = self.thir.places[*place].ty;
                self.check_expr_with_expected_type(*rhs, place_ty);
            }
            StmtKind::Return(expr) => {
                self.check_return(*expr, stmt.span);
            }
            StmtKind::Match { scrutinee, branches } => {
                let ThirExprWithSetup { stmts, expr } = scrutinee;
                self.check_stmts(stmts);
                self.check_expr(*expr);
                let scrut_ty = self.expr_ty(*expr);
                branches.iter().for_each(|branch| self.check_branch(scrut_ty, branch));
            }
            StmtKind::Expr(expr) => {
                let _ = self.check_expr(*expr);
            }
            StmtKind::Break(_) | StmtKind::Continue(_) | StmtKind::Error => (),
        }
    }

    fn get_peeled(
        &mut self,
        ty: TypeRef,
        expected_ty: TypeRef,
        span: Span,
    ) -> RefWrappedTy {
        RefWrappedTy::peel_until(self.db, ty, expected_ty).unwrap_or_else(|| {
            println!(
                "ERROR: Could not peel {} until {} :(",
                ty.to_string(self.db),
                expected_ty.to_string(self.db)
            );
            self.check_types(expected_ty, ty, span);
            RefWrappedTy::from_type_ref(self.db, ty)
        })
    }

    fn check_pattern(&mut self, expected_ty: TypeRef, pat: &ThirPattern) {
        let peeled = self.get_peeled(expected_ty, pat.ty, pat.span);
        match &pat.kind {
            ThirPatternKind::Error
            | ThirPatternKind::Any
            | ThirPatternKind::Bind { .. } => (),
            ThirPatternKind::Tuple(pats) => {
                for pat in pats {
                    self.check_pattern(pat.ty, pat);
                }
            }
            ThirPatternKind::Struct { def, fields } => {
                let actual = def.get_fields_ty(self.db);
                for (sym, fpat) in fields {
                    let ty = peeled.wrap_like(self.db, actual[sym]);
                    self.check_pattern(ty, fpat);
                }
                let bare = TypeRef::Concrete(TypeId::new(
                    self.db,
                    TypeDefId::Struct(def.def),
                    def.args.clone(),
                ));
                self.check_types(bare, pat.ty, pat.span);
            }
            ThirPatternKind::Constructor { def, idx, args } => {
                let enum_ty = TypeRef::Concrete(TypeId::new(
                    self.db,
                    TypeDefId::Enum(def.def),
                    def.args.clone(),
                ));
                self.check_types(enum_ty, pat.ty, pat.span);
                if let Some(cty) = def.get_cons(self.db, *idx) {
                    self.check_constructor_pat(&cty, args, &peeled, pat.span);
                } else {
                    Diag::generic_error("Unknown constructor for enum".into(), pat.span)
                        .accumulate(self.db);
                }
            }
            ThirPatternKind::IntLit(_) => {
                if !expected_ty
                    .as_type_id()
                    .and_then(|ty| ty.def(self.db).is_int_like(self.db))
                    .is_some()
                {
                    self.check_types(expected_ty, int_id(self.db).into(), pat.span);
                }
            }
        }
    }

    fn check_branch(&mut self, scrut_ty: TypeRef, branch: &ThirMatchBranch) {
        self.check_pattern(scrut_ty, &branch.pattern);
        if let Some(guard) = &branch.guard {
            self.check_stmts(&guard.stmts);
            self.check_expr_with_expected_type(
                guard.expr,
                TypeRef::Concrete(bool_id(self.db)),
            );
        }
        self.check_stmts(&branch.body);
    }

    fn check_expr(&mut self, expr: ExprId) -> TypeRef {
        let infos = &self.thir.exprs[expr];
        match &infos.kind {
            ExprKind::SizeOf(_) | ExprKind::TypeName(_) | ExprKind::IntLit(_) => {
                // TODO
            }
            ExprKind::Charlit(_) => self.check_types(
                TypeRef::Concrete(char_id(self.db)),
                infos.ty,
                infos.span,
            ),
            ExprKind::StrLit(_) => {
                self.check_types(TypeRef::Concrete(str_id(self.db)), infos.ty, infos.span)
            }
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
            ExprKind::AddressOf { place, mutability } => {
                self.check_place(*place);
                let place_ty = self.thir.places[*place].ty;
                let expected_ty =
                    TypeRef::Concrete(ptr_of(self.db, place_ty, mutability.is_mut()));
                self.check_types(expected_ty, infos.ty, infos.span);
            }
            ExprKind::Ref { place, mutability } => {
                self.check_place(*place);
                let place_ty = self.thir.places[*place].ty;
                let ref_ty =
                    TypeRef::Concrete(ref_of(self.db, place_ty, mutability.is_mut()));
                self.check_types(infos.ty, ref_ty, infos.span);
            }
            ExprKind::Call { called, args } => {
                let args_ty =
                    args.iter().map(|arg| (self.check_expr(*arg), *arg)).collect_vec();
                let ret = called.ret_ty(self.db);
                let params = called.params(self.db);
                params.into_iter().zip(args_ty).for_each(
                    |((_, param), (arg_ty, arg))| {
                        self.check_types(param, arg_ty, self.thir.exprs[arg].span)
                    },
                );
                self.check_types(ret, infos.ty, infos.span);
            }
            ExprKind::BinOp { op, lhs, rhs } => {
                let lhs_ty = self.check_expr(*lhs);
                let rhs_ty = self.check_expr(*rhs);
                let lhs_span = self.thir.exprs[*lhs].span;
                let rhs_span = self.thir.exprs[*rhs].span;
                match op {
                    BinaryOperator::Plus
                    | BinaryOperator::Minus
                    | BinaryOperator::Times
                    | BinaryOperator::Div
                    | BinaryOperator::Modulo => {
                        self.check_types(infos.ty, lhs_ty, self.thir.exprs[*lhs].span);
                        self.check_types(infos.ty, rhs_ty, self.thir.exprs[*rhs].span);
                        if infos
                            .ty
                            .as_type_id()
                            .and_then(|ty| ty.def(self.db).is_int_like(self.db))
                            .is_none()
                        {
                            self.check_types(
                                int_id(self.db).into(),
                                infos.ty,
                                infos.span,
                            );
                        }
                    }
                    BinaryOperator::Eq
                    | BinaryOperator::Diff
                    | BinaryOperator::Lt
                    | BinaryOperator::Leq
                    | BinaryOperator::Gt
                    | BinaryOperator::Geq => {
                        if lhs_ty
                            .as_type_id()
                            .and_then(|ty| ty.def(self.db).is_int_like(self.db))
                            .is_none()
                        {
                            self.check_types(
                                int_id(self.db).into(),
                                infos.ty,
                                infos.span,
                            );
                        }
                        self.check_types(lhs_ty, rhs_ty, infos.span);
                        self.check_types(bool_id(self.db).into(), infos.ty, infos.span);
                    }
                    BinaryOperator::And | BinaryOperator::Or => {
                        let bool_id = bool_id(self.db).into();
                        self.check_types(bool_id, lhs_ty, lhs_span);
                        self.check_types(bool_id, rhs_ty, rhs_span);
                        self.check_types(bool_id, infos.ty, infos.span);
                    }
                    BinaryOperator::BitAnd
                    | BinaryOperator::BitOr
                    | BinaryOperator::BitXor => {
                        Diag::todo("Handle bit manipulation ops".into(), infos.span)
                            .accumulate(self.db);
                    }
                }
            }
            ExprKind::Neg(operand) => {
                let operand_ty = self.check_expr(*operand);
                self.check_types(operand_ty, infos.ty, infos.span);
            }
            ExprKind::Not(operand) => {
                let operand_ty = self.check_expr(*operand);
                self.check_types(
                    TypeRef::Concrete(bool_id(self.db)),
                    operand_ty,
                    self.thir.exprs[*operand].span,
                );
                self.check_types(
                    TypeRef::Concrete(bool_id(self.db)),
                    infos.ty,
                    infos.span,
                );
            }
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
            ExprKind::SliceLit(items) => {
                if items.is_empty() {
                    if infos.ty.as_slice(self.db).is_none() {
                        let mock_expected =
                            TypeRef::Concrete(slice_of(self.db, TypeRef::Unknown));
                        self.check_types(mock_expected, infos.ty, infos.span);
                    }
                } else {
                    let tys = items
                        .iter()
                        .map(|expr| {
                            self.check_expr(*expr);
                            self.thir.exprs[*expr].ty
                        })
                        .collect::<HashSet<_>>();
                    for (ty_a, ty_b) in tys.iter().tuple_combinations() {
                        self.check_types(*ty_a, *ty_b, infos.span);
                    }
                    let witness_type = tys
                        .iter()
                        .find_or_first(|ty| {
                            !matches!(ty, TypeRef::Error | TypeRef::Unknown)
                        })
                        .copied()
                        .unwrap_or(TypeRef::Error);
                    let expected = TypeRef::Concrete(slice_of(self.db, witness_type));
                    self.check_types(expected, infos.ty, infos.span);
                }
            }
            ExprKind::Constructor { enum_def, idx, args } => {
                let ty = TypeRef::Concrete(TypeId::new(
                    self.db,
                    TypeDefId::Enum(enum_def.def),
                    enum_def.args.clone(),
                ));
                self.check_types(infos.ty, ty, infos.span);
                let constructor_ty = enum_def.get_cons(self.db, *idx).unwrap();
                self.check_constructor_expr(&constructor_ty, args, infos.span);
            }
            ExprKind::StructLit { struct_def, fields } => {
                let ty = TypeRef::Concrete(TypeId::new(
                    self.db,
                    TypeDefId::Struct(struct_def.def),
                    struct_def.args.clone(),
                ));
                self.check_types(ty, infos.ty, infos.span);
                let struct_fields = struct_def.get_fields_ty(self.db);
                for field in fields {
                    let matching_field = struct_fields[&field.field];
                    let field_ty = self.check_expr(field.expr);
                    let span = self.thir.exprs[field.expr].span;
                    self.check_types(matching_field, field_ty, span);
                }
            }

            ExprKind::Metadata(idx) => {
                let ty = self.check_expr(*idx);
                if let Some(_value) =
                    ty.as_ref(self.db).and_then(|(_, ty)| ty.as_slice(self.db))
                {
                    self.check_types(
                        ty.typeof_metadata(self.db).unwrap_or(TypeRef::Error),
                        infos.ty,
                        infos.span,
                    );
                } else {
                    // No other fatptr type is supported
                    let expr_span = self.thir.exprs[*idx].span;
                    self.check_types(
                        TypeRef::ref_slice_of(self.db, TypeRef::Unknown),
                        ty,
                        expr_span,
                    );
                }
            }
            ExprKind::Error => (),
            ExprKind::Cast(expr, type_ref) => {
                self.check_expr(*expr);
                self.check_types(*type_ref, infos.ty, infos.span);
            }
        };
        infos.ty
    }

    fn check_types(&mut self, expected: TypeRef, got: TypeRef, span: Span) {
        let expected = self.normalize_type(expected);
        let got = self.normalize_type(got);
        if expected != got {
            self.errs.push(SanityError { expected, got, span });
        }
    }

    fn check_projection(
        &mut self,
        before: TypeRef,
        projection: &Projection,
        expected: TypeRef,
        span: Span,
    ) {
        if let Some(ty) = self.compute_type_after_projection(before, projection, span) {
            self.check_types(expected, ty, span);
        } else {
            Diag::generic_error(
                "Could not compute the type after projection".into(),
                span,
            )
            .accumulate(self.db);
        }
    }

    fn check_place(&mut self, place: PlaceId) {
        let info = &self.thir.places[place];
        let base_ty = match info.base {
            PlaceBase::Local(idx) => self.thir.locals[idx].ty,
        };
        let end_ty = info.projections.iter().fold(base_ty, |before, projection| {
            let expected = match projection {
                Projection::Field(_, type_ref) | Projection::TupleField(_, type_ref) => {
                    Some(*type_ref)
                }
                _ => None,
            };
            let expected = expected
                .or_else(|| {
                    self.compute_type_after_projection(before, projection, info.span)
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

    pub fn check_expr_with_expected_type(&mut self, expr: ExprId, expected: TypeRef) {
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
                    let args = before.args(self.db);
                    assert_eq!(args.len(), 1);
                    Some(args[0])
                }
            }
            Projection::Field(symbol, type_ref) => {
                let struct_ref = before.as_struct_ref(self.db)?;
                let typeof_field = struct_ref.typeof_field(self.db, *symbol)?;
                self.check_types(typeof_field, *type_ref, span);
                Some(typeof_field)
            }
            Projection::TupleField(idx, type_ref) => {
                let as_tuple = before.as_tuple_ref(self.db)?;
                let typeof_field = *as_tuple.get(*idx as usize)?;
                self.check_types(typeof_field, *type_ref, span);
                Some(typeof_field)
            }
            Projection::Index(idx_expr) => {
                let ty = self.check_expr(*idx_expr);
                if ty
                    .as_type_id()
                    .and_then(|ty| ty.def(self.db).is_int_like(self.db))
                    .is_none()
                {
                    let idx_span = self.thir.exprs[*idx_expr].span;
                    self.check_types(ty, TypeRef::Concrete(int_id(self.db)), idx_span);
                    return None;
                }
                before.element_of_indexed(self.db)
            }
        }
    }

    pub fn check_constructor_expr(
        &mut self,
        ty: &ConstructorType,
        pat: &ThirConstructorArgs<ExprId>,
        span: Span,
    ) {
        match (ty, pat) {
            (ConstructorType::None, ThirConstructorArgs::None) => (),
            (
                ConstructorType::Struct(field_tys),
                ThirConstructorArgs::Struct(pat_tys),
            ) => {
                assert_eq!(field_tys.len(), pat_tys.len());
                for field in field_tys {
                    let field_expr = pat_tys.iter().find(|p| p.field == field.0).unwrap().expr;
                    let field_ty = self.check_expr(field_expr);
                    let span = self.thir.exprs[field_expr].span;
                    self.check_types(field.1, field_ty, span);
                }
            }
            (ConstructorType::Tuple(tys), ThirConstructorArgs::Tuple(exprs)) => {
                if tys.len() != exprs.len() {
                    Diag::generic_error(
                        "Wrong number of args for the tuple variant constructor".into(),
                        span,
                    )
                    .accumulate(self.db);
                }
                // Make sure we see all exprs even if mismatch
                let expr_tys =
                    exprs.iter().map(|expr| self.check_expr(*expr)).collect_vec();
                for (ty, expr_ty) in tys.iter().zip(expr_tys) {
                    self.check_types(*ty, expr_ty, span);
                }
            }
            _ => Diag::generic_error("Mismatched constructor kinds in expr".into(), span)
                .accumulate(self.db),
        }
    }

    pub fn check_constructor_pat(
        &mut self,
        ty: &ConstructorType,
        pat: &ThirConstructorArgs<ThirPattern>,
        peeled: &RefWrappedTy,
        span: Span,
    ) {
        match (ty, pat) {
            (ConstructorType::None, ThirConstructorArgs::None) => (),
            (ConstructorType::Struct(tys), ThirConstructorArgs::Struct(pats)) => {
                assert_eq!(tys.len(), pats.len());
                for (sym, fty) in tys {
                    let fpat = &pats.iter().find(|p| p.field == *sym).unwrap().expr;
                    self.check_pattern(peeled.wrap_like(self.db, *fty), fpat);
                }
            }
            (ConstructorType::Tuple(tys), ThirConstructorArgs::Tuple(pats)) => {
                assert_eq!(tys.len(), pats.len());
                tys.iter().zip(pats).for_each(|(ty, pat)| {
                    self.check_pattern(peeled.wrap_like(self.db, *ty), pat);
                });
            }
            _ => Diag::generic_error(
                "Mismatched constructor kinds in pattern".into(),
                span,
            )
            .accumulate(self.db),
        }
    }

    fn normalize_type(&self, ty: TypeRef) -> TypeRef {
        match ty {
            TypeRef::Zelf => {
                self.thir.id.parent(self.db).get_canonical_zelf(self.db).unwrap()
            }
            TypeRef::Concrete(type_id) => TypeRef::Concrete(TypeId::new(
                self.db,
                type_id.def(self.db),
                type_id.args(self.db).iter().map(|ty| self.normalize_type(*ty)).collect(),
            )),
            _ => ty,
        }
    }
}

pub enum ConstructorType {
    Tuple(Vec<TypeRef>),
    Struct(Vec<(Symbol, TypeRef)>),
    None,
}

impl ConstructorType {
    pub fn with_substitution(&self, db: &dyn Db, sub: &[TypeRef]) -> Self {
        match self {
            ConstructorType::Tuple(type_refs) => ConstructorType::Tuple(
                type_refs.iter().map(|ty| ty.with_substitution(db, sub)).collect(),
            ),
            ConstructorType::Struct(items) => ConstructorType::Struct(
                items
                    .iter()
                    .map(|(name, ty)| (*name, ty.with_substitution(db, sub)))
                    .collect(),
            ),
            ConstructorType::None => ConstructorType::None,
        }
    }
}

impl EnumRef {
    pub fn get_cons(&self, db: &dyn Db, idx: usize) -> Option<ConstructorType> {
        let item = enum_item(db, self.def.interned());
        let variant = item.variants.get(idx)?;
        let raw = match &variant.kind {
            AstEnumVariantKind::Unit => Some(ConstructorType::None),
            AstEnumVariantKind::StructLike(fields) => {
                let ctx = AstImplicitContext::new(
                    db,
                    ScopeOwnerId::Module(self.def.parent(db)),
                    item.template_args.iter().cloned().collect(),
                )
                .unwrap();
                Some(ConstructorType::Struct(
                    fields
                        .iter()
                        .map(|field| (field.name, ctx.resolve_err(db, &field.ty.data)))
                        .collect(),
                ))
            }
            AstEnumVariantKind::TupleLike(spanneds) => {
                let ctx = AstImplicitContext::new(
                    db,
                    ScopeOwnerId::Module(self.def.parent(db)),
                    item.template_args.iter().cloned().collect(),
                )
                .unwrap();
                Some(ConstructorType::Tuple(
                    spanneds.iter().map(|ast| ctx.resolve_err(db, &ast.data)).collect(),
                ))
            }
        }?;
        Some(raw.with_substitution(db, &self.args))
    }

    pub fn variants(&self, db: &dyn Db) -> Vec<ConstructorType> {
        let n = enum_item(db, self.def.interned()).variants.len();
        (0..n).into_iter().map(|i| self.get_cons(db, i).unwrap()).collect()
    }
}

pub fn sanity_check(db: &dyn Db, thir: &Thir) {
    let mismatches = SanityChecker::new(db, thir).check();
    for mismatch in &mismatches {
        Diag::generic_error(
            format!(
                "Type mismatch: Expected {} but got {}",
                mismatch.expected.to_string(db),
                mismatch.got.to_string(db),
            ),
            mismatch.span,
        )
        .accumulate(db);
    }
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
            let type_ref =
                ctx.resolve_err(db, &field.ty.data).with_substitution(db, &self.args);
            res.insert(field.name, type_ref);
        }
        res
    }
}

impl FunctionRef {
    pub fn ret_ty(&self, db: &dyn Db) -> TypeRef {
        let sig = get_sig_of_function(db, self.id.interned());
        let zelf = self.zelf_binding(db);
        sig.ret.instantiate(db, &self.args, zelf)
    }

    pub fn params(&self, db: &dyn Db) -> Vec<(Symbol, TypeRef)> {
        let sig = get_sig_of_function(db, self.id.interned());
        let zelf = self.zelf_binding(db);
        let zelf_arg = match (zelf, sig.zelf) {
            (_, None) => None,
            (Some(ty), Some(r)) => Some(r.as_type_ref_for(db, ty)),
            (None, _) => unreachable!("missing zelf from binding"),
        };
        zelf_arg
            .into_iter()
            .map(|ty| (Symbol::new(db, "self"), ty))
            .chain(
                sig.args.iter().map(|(s, ty)| (*s, ty.instantiate(db, &self.args, zelf))),
            )
            .collect()
    }

    fn zelf_binding(&self, db: &dyn Db) -> Option<TypeRef> {
        self.self_ty.or_else(|| match self.id.parent(db) {
            ScopeOwnerId::Impl(i) => {
                Some(i.implemented(db).with_substitution(db, &self.args))
            }
            _ => None,
        })
    }
}

impl TypeRef {
    pub fn as_slice(self, db: &dyn Db) -> Option<Self> {
        let (b, args) = self.as_builtin(db)?;
        match b.kind(db) {
            BuiltinTypeKind::Slice => Some(args[0]),
            _ => None,
        }
    }

    pub fn element_of_indexed(self, db: &dyn Db) -> Option<Self> {
        if let Some((_, inner)) = self.as_ptr(db) {
            Some(inner)
        } else if let Some(inner) = self.as_slice(db) {
            Some(inner)
        } else if let Some(inner) = self.as_ref(db).and_then(|t| t.1.as_slice(db)) {
            Some(inner)
        } else {
            None
        }
    }

    pub fn ref_slice_of(db: &dyn Db, inner: Self) -> Self {
        TypeRef::Concrete(slice_of(db, TypeRef::Concrete(slice_of(db, inner))))
    }

    pub fn typeof_metadata(&self, db: &dyn Db) -> Option<Self> {
        if self.as_ref(db).and_then(|(_, ty)| ty.as_slice(db)).is_some() {
            Some(Self::Concrete(usize_id(db)))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapKind {
    Ref(Mutability),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefWrappedTy {
    pub inner: TypeRef,
    pub refs: Vec<WrapKind>, // [&A, &B, &C] means &C &B &A T
}

impl RefWrappedTy {
    pub fn wrap_like(&self, db: &dyn Db, to_wrap: TypeRef) -> TypeRef {
        self.refs.iter().fold(to_wrap, |ty, ref_kind| ref_kind.wrap(db, ty))
    }

    pub fn as_type_ref(&self, db: &dyn Db) -> TypeRef {
        self.wrap_like(db, self.inner)
    }

    pub fn peel_until(db: &dyn Db, to_peel: TypeRef, target: TypeRef) -> Option<Self> {
        let mut refs = vec![];
        let mut inner = to_peel;
        while inner != target
            && let Some((mutability, ty)) = inner.as_ref(db)
        {
            inner = ty;
            refs.push(WrapKind::Ref(mutability));
        }
        if inner != target {
            return None;
        }
        Some(Self { inner, refs })
    }

    pub fn from_type_ref(db: &dyn Db, ty: TypeRef) -> Self {
        let mut refs = vec![];
        let mut inner = ty;
        while let Some((mutability, ty)) = inner.as_ref(db) {
            inner = ty;
            refs.push(WrapKind::Ref(mutability));
        }
        refs.reverse();
        Self { inner, refs }
    }

    pub fn depth(&self) -> usize {
        self.refs.len()
    }
}

impl WrapKind {
    pub fn wrap(self, db: &dyn Db, ty: TypeRef) -> TypeRef {
        let Self::Ref(mutability) = self;
        ref_of(db, ty, mutability.is_mut()).into()
    }
}

impl TypeRef {
    pub fn instantiate(
        self,
        db: &dyn Db,
        subs: &[TypeRef],
        zelf: Option<TypeRef>,
    ) -> TypeRef {
        match self {
            TypeRef::Concrete(id) => TypeRef::Concrete(TypeId::new(
                db,
                id.def(db),
                id.args(db).iter().map(|t| t.instantiate(db, subs, zelf)).collect(),
            )),
            TypeRef::Param(p) => subs[p.0], // callee-space param
            TypeRef::Zelf => {
                zelf.expect("Zelf in signature but no self type on FunctionRef")
            }
            other => other,
        }
    }
}
