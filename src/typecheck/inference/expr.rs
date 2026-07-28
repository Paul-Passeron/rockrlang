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
    collections::{HashMap, HashSet},
    sync::Arc,
};

use itertools::Itertools;
use salsa::Accumulator;

use crate::{
    Db,
    common::{location::Span, symbols::Symbol},
    compiler::diagnostic::Diag,
    hir::{
        FunctionLikeAst, HirConstructorArgs, HirExpr, HirExprDesc, HirPlace,
        HirPlaceKind, HirStructField, LocalId, function_ast,
    },
    name_resolve::type_expr::{
        enum_item, get_templates_of_fun, struct_item, templates_of_enum,
        templates_of_struct,
    },
    parse_tree::{
        expr::BinaryOperator,
        top_level::{AstEnumVariantKind, AstStructDefField},
    },
    resolved::{EnumId, FunctionId, ScopeOwnerId, StructId, TypeDefId, TypeRef},
    typecheck::{
        CallKind, ExprId, InferCallInfos, PlaceId,
        inference::{
            InferTy, InferenceCtx, UnificationError, implicit::ImplicitContext,
            var::InferVar,
        },
    },
};

impl InferenceCtx<'_> {
    fn infer_expr_aux(&mut self, expr: &HirExpr) -> Result<InferTy, UnificationError> {
        match &expr.data {
            HirExprDesc::IntLit(_) => Ok(self.emit_intlike_constraint().into()),
            HirExprDesc::CharLit(_) => Ok(self.char_ty()),
            HirExprDesc::StrLit(_) | HirExprDesc::TypeName(_) => Ok(self.str_ty()),
            HirExprDesc::CStrLit(_) => Ok(self.cstr_ty()),
            HirExprDesc::BoolLit(_) => Ok(self.bool_ty()),
            HirExprDesc::Use(place) => self.infer_place(place),
            HirExprDesc::AddressOf { place, .. } | HirExprDesc::Ref { place, .. } => {
                let place_ty = self.infer_place(place)?;
                Ok(self.some_ptr_to(place_ty))
            }
            HirExprDesc::CallDirect { target, args, type_args } => {
                self.infer_direct(ExprId(expr.id), *target, type_args, args)
            }
            HirExprDesc::CallMethod {
                receiver,
                method,
                args,
                interface_hint,
                type_args,
            } => {
                let id = ExprId(expr.id);
                let method = *method;
                let interface_hint = *interface_hint;
                let span = expr.span;
                if !type_args.is_empty() {
                    Diag::todo(
                        "Turbofish on method call is not supported yet.".into(),
                        span,
                    )
                    .accumulate(self.db);
                }
                let receiver_ty = self.infer_expr(receiver)?;
                let inferred_args = args
                    .iter()
                    .map(|arg| self.infer_expr(arg))
                    .collect::<Result<Box<[_]>, _>>()?;
                let res_var = self.emit_method_constraint(
                    id,
                    receiver_ty,
                    method,
                    inferred_args,
                    interface_hint,
                    false,
                );
                Ok(InferTy::Var(res_var))
            }
            HirExprDesc::CallStatic { ty, method, args, type_args } => self.infer_static(
                ExprId(expr.id),
                ty,
                *method,
                type_args,
                args,
                expr.span,
            ),
            HirExprDesc::BinOp { lhs, op, rhs } => self.infer_binop(lhs, *op, rhs),
            HirExprDesc::StructLit { ty, fields } => {
                self.infer_struct_lit(ty, fields, expr.span)
            }
            HirExprDesc::Neg(hir_expr) => {
                let ty = self.infer_expr(hir_expr)?;
                self.unify(&ty, &self.int_ty())?;
                Ok(ty)
            }
            HirExprDesc::Not(hir_expr) => {
                let ty = self.infer_expr(hir_expr)?;
                self.unify(&ty, &self.bool_ty())?;
                Ok(ty)
            }
            HirExprDesc::Tuple(exprs) => {
                let tys = exprs.iter().map(|expr| self.infer_expr(expr)).try_collect()?;
                Ok(self.tuple_of(tys))
            }
            HirExprDesc::SliceLit(exprs) => {
                let elem_var = self.fresh_var();
                exprs.iter().try_for_each(|expr| {
                    let ty = self.infer_expr(expr)?;
                    self.unify(&elem_var.into(), &ty)?;
                    Ok(())
                })?;
                Ok(self.slice_of(elem_var.into()))
            }
            HirExprDesc::SizeOf(_) => Ok(self.usize_ty()),
            HirExprDesc::Constructor { enum_def, name, args, template_hints } => {
                self.infer_constructor(*enum_def, *name, args, template_hints, expr.span)
            }
            HirExprDesc::UnresolvedCallDirect { args, .. } => {
                for arg in args {
                    let _ = self.infer_expr_aux(arg);
                }
                Diag::generic_error(
                    "function not found in current scope".into(),
                    expr.span,
                )
                .accumulate(self.db);
                Ok(self.fresh_var().into())
            }
            HirExprDesc::Error => Ok(self.fresh_var().into()),
            HirExprDesc::Metadata(hir_expr) => {
                let fat_ptr_ty = self.infer_expr(hir_expr)?;
                let fat_ptr_var = self.emit_fat_ptr_constraint();
                let metadata_var = self.emit_metadata_of_fat_ptr_constraint(fat_ptr_var);
                self.unify(&fat_ptr_ty, &fat_ptr_var.into())?;
                Ok(metadata_var.into())
            }
            HirExprDesc::As { expr: castee, ty } => self.infer_as_cast(expr, castee, ty),
        }
    }

    fn infer_as_cast(
        &mut self,
        expr: &HirExpr,
        castee: &HirExpr,
        ty: &TypeRef,
    ) -> Result<InferTy, UnificationError> {
        if let Some((id, _)) = ty.as_builtin(self.db)
            && id.is_int_like(self.db).is_some()
        {
            let expr_ty = self.emit_intlike_constraint();
            let actual_expr_ty = self.infer_expr(castee)?;

            let casted_to =
                self.allocate_type_ref(*ty, self.implicit_ctx.clone().as_ref());
            if let Err(err) = self.unify(&expr_ty.into(), &actual_expr_ty) {
                Diag::generic_error(
                    format!("Invalid cast: {}", err.display(self.db)),
                    expr.span,
                )
                .accumulate(self.db);
            }
            return Ok(casted_to);
        }
        // For the moment, this only works on pointer types.
        // This can do ref -> ptr but not ptr -> ref
        let pointee = self.fresh_var();
        let expr_ptr_ty = self.emit_deref_constraint(pointee.into());
        let expr_ty = self.infer_expr(castee)?;
        if let Err(err) = self.unify(&expr_ptr_ty.into(), &expr_ty) {
            Diag::generic_error(
                format!("Unification error in as expr: {}", err.display(self.db)),
                castee.span,
            )
            .accumulate(self.db);
        }

        let actual_ty = self.allocate_type_ref(*ty, &self.implicit_ctx());

        Ok(actual_ty)
    }

    fn infer_place_aux(&mut self, place: &HirPlace) -> Result<InferTy, UnificationError> {
        let val = match &place.kind {
            HirPlaceKind::Local(local_id) => Ok(self.infer_local(*local_id)),
            HirPlaceKind::Field { base, field } => {
                let base_ty = self.infer_place(base)?;
                let elem_var = self.emit_struct_field_constraint(base_ty, *field);
                Ok(elem_var.into())
            }
            HirPlaceKind::TupleField { base, index } => {
                let base_ty = self.infer_place(base)?;
                let elem_var = self.emit_tuple_constraint(base_ty, *index);
                Ok(elem_var.into())
            }
            HirPlaceKind::Deref(hir_place) => {
                let ptr_ty = self.infer_place(hir_place)?;
                let pointee_var = self.fresh_var();
                let ptr_var = self.emit_deref_constraint(pointee_var.into());
                self.unify(&ptr_var.into(), &ptr_ty)?;
                Ok(pointee_var.into())
            }
            HirPlaceKind::Index { base, index } => {
                let index_ty = self.infer_expr(index)?;
                let base_ty = self.infer_place(base)?;
                let element_var = self.emit_indexed_by_constraint(base_ty, index_ty);
                Ok(element_var.into())
            }
            HirPlaceKind::Temporary(hir_expr) => self.infer_expr(hir_expr),
        }?;
        self.set_inferred_place(PlaceId(place.id), val.clone());
        Ok(val)
    }

    pub fn infer_place(&mut self, place: &HirPlace) -> Result<InferTy, UnificationError> {
        self.snapshot(|this| this.infer_place_aux(place))
    }

    pub fn infer_expr(&mut self, expr: &HirExpr) -> Result<InferTy, UnificationError> {
        let prev_span = self.set_current_span(expr.span);
        let res = self.snapshot(|this| {
            let ty = this.infer_expr_aux(expr)?;
            this.set_inferred_expr(ExprId(expr.id), ty.clone());
            Ok(ty)
        });
        self.set_current_span(prev_span);
        res
    }

    pub fn local_var(&self, local_id: LocalId) -> InferVar {
        self.local_map[&local_id]
    }

    pub fn infer_local(&self, local_id: LocalId) -> InferTy {
        self.local_var(local_id).into()
    }

    pub fn some_ptr_to(&mut self, pointee: InferTy) -> InferTy {
        self.emit_deref_constraint(pointee).into()
    }

    fn allocate_struct_partial_ref(
        &mut self,
        type_ref: &TypeRef,
    ) -> Option<(StructId, Vec<InferTy>)> {
        match type_ref {
            TypeRef::Zelf => {
                let zelf = self.implicit_ctx().zelf()?.clone();
                match zelf {
                    InferTy::Adt { def, fields } => {
                        let TypeDefId::Struct(struct_id) = def else {
                            return None;
                        };
                        Some((struct_id, fields))
                    }
                    _ => None,
                }
            }
            TypeRef::Concrete(type_id) => match type_id.def(self.db) {
                TypeDefId::Struct(struct_id) => {
                    let templates = templates_of_struct(self.db, struct_id.interned());
                    let templates: Vec<InferTy> =
                        templates.iter().map(|_| self.fresh_var().into()).collect_vec();
                    let args = type_id.args(self.db);
                    self.snapshot(|this| {
                        templates.iter().zip(args.iter()).try_for_each(
                            |(infer_ty, t_ref)| {
                                let t_ref = this.allocate_type_ref(
                                    *t_ref,
                                    this.implicit_ctx().as_ref(),
                                );
                                this.unify(infer_ty, &t_ref)
                            },
                        )
                    })
                    .ok()?;
                    Some((struct_id, templates))
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn infer_struct_lit(
        &mut self,
        ty: &TypeRef,
        fields: &[HirStructField],
        span: Span,
    ) -> Result<InferTy, UnificationError> {
        if let Some((struct_id, templates)) = self.allocate_struct_partial_ref(ty) {
            let ast = struct_item(self.db, struct_id.interned());
            let inferred_fields = fields
                .iter()
                .map(|field| self.infer_expr(&field.expr).map(|res| (field.field, res)))
                .collect::<Result<HashMap<_, _>, _>>()?;

            let field_keys: HashSet<_> = inferred_fields.keys().copied().collect();
            if !diagnose_bad_struct_fields(self.db, span, struct_id, &field_keys) {
                return Err(UnificationError::AlreadyDiagnosed);
            }

            let module = struct_id.parent(self.db);
            let zelf = self.fresh_var();

            let ctx = ImplicitContext::new(
                self.db,
                ScopeOwnerId::Module(module),
                templates_of_struct(self.db, struct_id.interned()),
                templates.iter().cloned().collect(),
                Some(InferTy::Var(zelf)),
            );

            self.snapshot(|this| {
                ast.fields.iter().try_for_each(|ast| {
                    let ty = inferred_fields
                        .get(&ast.name)
                        .cloned()
                        .unwrap_or_else(|| this.fresh_var().into());
                    let resolved = this.allocate_ast_type_expr(&ast.ty.data, &ctx);
                    this.unify(&ty, &resolved)
                })
            })?;

            let as_struct =
                InferTy::Adt { def: TypeDefId::Struct(struct_id), fields: templates };

            self.unify(&zelf.into(), &as_struct)?;

            Ok(as_struct)
        } else {
            Err(UnificationError::NonStructForStructLit(*ty))
        }
    }

    fn diagnose_field_mismatches(
        &self,
        enum_id: EnumId,
        variant: Symbol,
        fields: &[HirStructField],
        ast_fields: &[AstStructDefField],
        span: Span,
    ) {
        let expected_fields: HashSet<Symbol> =
            HashSet::from_iter(ast_fields.iter().map(|f| f.name));
        let got_fields: HashSet<Symbol> =
            HashSet::from_iter(fields.iter().map(|f| f.field));
        for field in got_fields.difference(&expected_fields) {
            Diag::generic_error(
                format!(
                    "Missing field `{}` in struct lit variant `{}` for type `{}`",
                    field.to_string(self.db),
                    variant.to_string(self.db),
                    enum_id.name(self.db).to_string(self.db)
                ),
                span,
            )
            .accumulate(self.db);
        }
        // For inferred fields not in ast fields
        for field in expected_fields.difference(&got_fields) {
            Diag::generic_error(
                format!(
                    "Invalid field `{}` in struct lit variant `{}` for type `{}`",
                    field.to_string(self.db),
                    variant.to_string(self.db),
                    enum_id.name(self.db).to_string(self.db)
                ),
                span,
            )
            .accumulate(self.db);
        }
    }

    fn infer_constructor(
        &mut self,
        enum_def: EnumId,
        name: Symbol,
        args: &HirConstructorArgs,
        template_hints: &[TypeRef],
        span: Span,
    ) -> Result<InferTy, UnificationError> {
        let variants = &enum_item(self.db, enum_def.interned()).variants;
        let Some(variant) = variants.iter().find(|variant| variant.name == name) else {
            Diag::generic_error(
                format!(
                    "Unknown variant `{}` in type `{}`",
                    name.display(self.db),
                    enum_def.name(self.db).display(self.db)
                ),
                span,
            );
            return Err(UnificationError::AlreadyDiagnosed);
        };

        let enum_templates = templates_of_enum(self.db, enum_def.interned());

        let templates: Arc<[InferTy]> = enum_templates
            .iter()
            .enumerate()
            .map(|(i, _)| {
                // TODO: handle conformances for t
                let var: InferTy = self.fresh_var().into();
                if let Some(hint) = template_hints.get(i) {
                    let allocated = self.allocate_type_ref(*hint, &self.implicit_ctx());
                    self.unify(&allocated, &var)?;
                }
                Ok(var)
            })
            .try_collect()?;

        let zelf = self.fresh_var();

        let ctx = ImplicitContext::new(
            self.db,
            ScopeOwnerId::Module(enum_def.parent(self.db)),
            enum_templates,
            templates.clone(),
            Some(zelf.into()),
        );

        match (args, &variant.kind) {
            (
                HirConstructorArgs::TupleLike(hir_exprs),
                AstEnumVariantKind::TupleLike(spanneds),
            ) => {
                assert_eq!(hir_exprs.len(), spanneds.len());
                for (hir, ast) in hir_exprs.iter().zip(spanneds) {
                    let hir_ty = self.infer_expr(hir)?;
                    let in_ctx = self.allocate_ast_type_expr(&ast.data, &ctx);
                    self.unify(&hir_ty, &in_ctx)?;
                }
            }
            (
                HirConstructorArgs::StructLike { fields },
                AstEnumVariantKind::StructLike(ast_fields),
            ) => {
                self.diagnose_field_mismatches(
                    enum_def,
                    variant.name,
                    fields,
                    ast_fields,
                    span,
                );
                for field in fields {
                    let ast = ast_fields.iter().find(|ast| ast.name == field.field);
                    let field_ty = if let Some(ast) = ast {
                        self.allocate_ast_type_expr(&ast.ty.data, &ctx)
                    } else {
                        self.fresh_var().into()
                    };
                    let err = match self.infer_expr(&field.expr) {
                        Ok(expr_ty) => self.unify(&expr_ty, &field_ty).err(),
                        Err(err) => Some(err),
                    };
                    if let Some(err) = err {
                        Diag::generic_error(
                            format!(
                                "Unification error in expr: {}",
                                err.display(self.db)
                            ),
                            field.expr.span,
                        )
                        .accumulate(self.db);
                    }
                }
            }
            (HirConstructorArgs::None, AstEnumVariantKind::Unit) => (),
            _ => Diag::generic_error("Constructor kind mismatch in expr".into(), span)
                .accumulate(self.db),
        }

        let as_enum = InferTy::Adt {
            def: TypeDefId::Enum(enum_def),
            fields: templates.iter().cloned().collect(),
        };
        self.unify(&zelf.into(), &as_enum)?;
        Ok(InferTy::Var(zelf))
    }

    fn infer_static(
        &mut self,
        id: ExprId,
        ty: &TypeRef,
        method: Symbol,
        type_args: &[TypeRef],
        args: &[HirExpr],
        span: Span,
    ) -> Result<InferTy, UnificationError> {
        if !type_args.is_empty() {
            Diag::todo(
                "Turbofish on static method call is not supported yet.".into(),
                span,
            )
            .accumulate(self.db);
        }
        let receiver_ty = self.allocate_type_ref(*ty, self.implicit_ctx().as_ref());
        let inferred_args = args
            .iter()
            .map(|arg| self.infer_expr(arg))
            .collect::<Result<Box<[_]>, _>>()?;
        let res_var = self.emit_method_constraint(
            id,
            receiver_ty,
            method,
            inferred_args,
            None,
            true,
        );
        Ok(InferTy::Var(res_var))
    }

    fn check_args_count(
        &self,
        target: FunctionId,
        arg_count: usize,
    ) -> Result<(), UnificationError> {
        let ast = function_ast(self.db, target.interned()).inner(self.db);
        let (_, args) = target.args(self.db);
        if args.len() == arg_count {
            Ok(())
        } else if let FunctionLikeAst::ExternDef(_, variadic) = ast
            && *variadic
            && args.len() <= arg_count
        {
            Ok(())
        } else {
            Err(UnificationError::ArgCountMismatch(target, arg_count))
        }
    }

    fn get_ret_ty(
        &mut self,
        target: FunctionId,
        templates: &[InferTy],
        zelf: Option<&InferTy>,
    ) -> InferTy {
        let ast = function_ast(self.db, target.interned()).inner(self.db);
        let ast_ret_ty = match ast {
            FunctionLikeAst::ExternDef(sig, _) => &sig.data.return_type,
            FunctionLikeAst::Fundef(def) => &def.data.return_type,
            FunctionLikeAst::Method(def) => &def.data.return_type,
            FunctionLikeAst::TraitMethod(sig) => &sig.data.return_type,
        };

        let ctx = ImplicitContext::from_function(
            self.db,
            target,
            templates.iter().cloned().collect(),
            zelf.cloned(),
        );

        self.allocate_ast_type_expr(&ast_ret_ty.data, &ctx)
    }

    fn infer_direct(
        &mut self,
        id: ExprId,
        target: FunctionId,
        type_args: &[TypeRef],
        args: &[HirExpr],
    ) -> Result<InferTy, UnificationError> {
        self.check_args_count(target, args.len())?;
        let (receiver, ast_args) = target.args(self.db);
        assert!(receiver.is_none());
        let templates = get_templates_of_fun(self.db, target.interned());
        let inferred_templates = if type_args.is_empty() {
            templates.iter().map(|_| InferTy::Var(self.fresh_var())).collect::<Arc<[_]>>()
        } else {
            assert_eq!(templates.len(), type_args.len());
            let ctx = self.implicit_ctx.clone();
            type_args
                .iter()
                .map(|arg| self.allocate_type_ref(*arg, ctx.as_ref()))
                .collect()
        };

        let ctx = ImplicitContext::from_function(
            self.db,
            target,
            inferred_templates.clone(),
            None,
        );

        let inferred_ast_args = ast_args
            .iter()
            .map(|arg| self.allocate_ast_type_expr(&arg.ty.data, &ctx))
            .collect::<Box<[_]>>();
        let inferred_args = args
            .iter()
            .map(|arg| self.infer_expr(arg))
            .collect::<Result<Box<[_]>, _>>()?;
        inferred_ast_args
            .iter()
            .zip(&inferred_args)
            .try_for_each(|(a, b)| self.unify(a, b))?;

        self.call_infos.insert(
            id,
            InferCallInfos {
                expr_id: id,
                callee: target,
                substitution: inferred_templates.iter().cloned().collect(),
                zelf_ty: None,
                call_kind: CallKind::Direct,
            },
        );

        Ok(self.get_ret_ty(target, &inferred_templates, None))
    }

    fn infer_binop(
        &mut self,
        lhs: &HirExpr,
        op: BinaryOperator,
        rhs: &HirExpr,
    ) -> Result<InferTy, UnificationError> {
        let lhs_ty = self.infer_expr(lhs)?;
        let rhs_ty = self.infer_expr(rhs)?;
        let res = self.emit_binop_constraint(lhs_ty, rhs_ty, op);
        Ok(InferTy::Var(res))
    }
}

pub fn diagnose_bad_struct_fields(
    db: &dyn Db,
    span: Span,
    struct_id: StructId,
    inferred_fields: &HashSet<Symbol>,
) -> bool {
    let ast = struct_item(db, struct_id.into());
    let field_sets = (
        inferred_fields.iter().copied().collect::<HashSet<_>>(),
        ast.fields.iter().map(|f| f.name).collect::<HashSet<_>>(),
    );
    if field_sets.0 == field_sets.1 {
        return true;
    }
    // For ast fields not in inferred fields
    for field in field_sets.1.difference(&field_sets.0) {
        Diag::generic_error(
            format!(
                "Missing field `{}` in struct lit for type `{}`",
                field.to_string(db),
                struct_id.name(db).to_string(db)
            ),
            span,
        )
        .accumulate(db);
    }
    // For inferred fields not in ast fields
    for field in field_sets.0.difference(&field_sets.1) {
        Diag::generic_error(
            format!(
                "Invalid field `{}` in struct lit for type `{}`",
                field.to_string(db),
                struct_id.name(db).to_string(db)
            ),
            span,
        )
        .accumulate(db);
    }
    false
}
