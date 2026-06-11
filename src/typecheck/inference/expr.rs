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

use crate::{
    common::{location::Span, symbols::Symbol},
    hir::{
        FunctionLikeAst, HirConstructorArgs, HirExpr, HirExprDesc, HirPlace,
        HirPlaceKind, LocalId, PartialTypeArg, PartialTypeRef, function_ast,
    },
    name_resolve::type_expr::{
        enum_item, get_templates_of_fun, struct_item, templates_of_enum,
        templates_of_struct,
    },
    parse_tree::{
        expr::BinaryOperator,
        top_level::{AstEnumVariantKind, AstStructDef, AstStructDefField},
    },
    ril::{EnumId, FunctionId, InterfaceId, ScopeOwnerId, StructId, TypeDefId, TypeRef},
    typecheck::{
        ExprId, InferCallInfos, PlaceId,
        inference::{implicit::ImplicitContext, var::InferVar},
    },
};

use super::{InferTy, InferenceCtx, UnificationError};

impl<'db> InferenceCtx<'db> {
    fn _infer_expr(&mut self, expr: &HirExpr) -> Result<InferTy, UnificationError> {
        match &expr.data {
            HirExprDesc::IntLit(_) => Ok(self.emit_intlike_constraint().into()),
            HirExprDesc::CharLit(_) => Ok(self.char_ty()),
            HirExprDesc::StrLit(_) => Ok(self.str_ty()),
            HirExprDesc::CStrLit(_) => Ok(self.cstr_ty()),
            HirExprDesc::BoolLit(_) => Ok(self.bool_ty()),
            HirExprDesc::Use(place) => self.infer_place(place),
            HirExprDesc::AddressOf { place, .. } | HirExprDesc::Ref { place, .. } => {
                let place_ty = self.infer_place(place)?;
                Ok(self.some_ptr_to(place_ty))
            }
            HirExprDesc::CallDirect { target, args } => {
                self.infer_direct(ExprId(expr.id), *target, args)
            }
            HirExprDesc::CallMethod {
                receiver,
                method,
                args,
                interface_hint,
            } => self.infer_method(
                ExprId(expr.id),
                receiver,
                *method,
                args,
                *interface_hint,
            ),
            HirExprDesc::CallStatic { ty, method, args } => {
                self.infer_static(ExprId(expr.id), ty, *method, args)
            }
            HirExprDesc::BinOp { lhs, op, rhs } => self.infer_binop(lhs, *op, rhs),
            HirExprDesc::StructLit { ty, fields } => {
                self.infer_struct_lit(ty, fields, expr.span)
            }
            HirExprDesc::Neg(hir_expr) => {
                let ty = self.infer_expr(hir_expr)?;
                self.unify(ty.clone(), self.int_ty())?;
                Ok(ty)
            }
            HirExprDesc::Not(hir_expr) => {
                let ty = self.infer_expr(hir_expr)?;
                self.unify(ty.clone(), self.bool_ty())?;
                Ok(ty)
            }
            HirExprDesc::Tuple(exprs) => {
                let tys = exprs
                    .iter()
                    .map(|expr| self.infer_expr(expr))
                    .collect::<Result<Box<[_]>, _>>()?;
                Ok(self.tuple_of(tys))
            }
            HirExprDesc::SliceLit(exprs) => {
                let elem_var = self.fresh_var();
                exprs.iter().try_for_each(|expr| {
                    let ty = self.infer_expr(expr)?;
                    self.unify(elem_var.into(), ty)?;
                    Ok(())
                })?;
                Ok(self.slice_of(elem_var.into()))
            }
            HirExprDesc::SizeOf(_) => Ok(self.int_ty()),
            HirExprDesc::Constructor {
                enum_def,
                name,
                args,
                template_hints,
            } => self.infer_constructor(*enum_def, *name, args, template_hints),
            HirExprDesc::UnresolvedCallDirect { args, .. } => {
                args.iter().for_each(|arg| {
                    let _ = self._infer_expr(arg);
                });
                println!("TODO: function not found in current scope");
                Ok(InferTy::Var(self.fresh_var()))
            }
            HirExprDesc::Error => Ok(self.fresh_var().into()),
            HirExprDesc::Metadata(hir_expr) => {
                let fat_ptr_ty = self.infer_expr(hir_expr)?;
                let fat_ptr_var = self.emit_fat_ptr_constraint();
                let metadata_var = self.emit_metadata_of_fat_ptr_constraint(fat_ptr_var);
                self.unify(fat_ptr_ty, fat_ptr_var.into())?;
                Ok(metadata_var.into())
            },
        }
    }

    fn _infer_place(&mut self, place: &HirPlace) -> Result<InferTy, UnificationError> {
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
                self.unify(ptr_var.into(), ptr_ty)?;
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
        self.inferred_places.insert(PlaceId(place.id), val.clone());
        Ok(val)
    }

    pub fn infer_place(&mut self, place: &HirPlace) -> Result<InferTy, UnificationError> {
        self.snapshot(|this| this._infer_place(place))
    }

    pub fn infer_expr(&mut self, expr: &HirExpr) -> Result<InferTy, UnificationError> {
        self.snapshot(|this| {
            let ty = this._infer_expr(expr)?;
            this.inferred_exprs.insert(ExprId(expr.id), ty.clone());
            Ok(ty)
        })
    }

    pub fn local_var(&self, local_id: LocalId) -> InferVar {
        self.local_map[&local_id]
    }

    pub fn infer_local(&mut self, local_id: LocalId) -> InferTy {
        self.local_var(local_id).into()
    }

    pub fn some_ptr_to(&mut self, pointee: InferTy) -> InferTy {
        self.emit_deref_constraint(pointee).into()
    }

    fn allocate_struct_partial_ref(
        &mut self,
        type_ref: &PartialTypeRef,
    ) -> Option<(StructId, Box<[InferTy]>)> {
        match type_ref {
            PartialTypeRef::Resolved(TypeRef::Zelf) => {
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
            PartialTypeRef::Resolved(TypeRef::Concrete(type_id)) => {
                match type_id.def(self.db) {
                    TypeDefId::Struct(struct_id) => {
                        let templates =
                            templates_of_struct(self.db, struct_id.interned());
                        let templates = templates
                            .iter()
                            .map(|_| self.fresh_var().into())
                            .collect::<Box<[InferTy]>>();
                        let args = type_id.args(self.db);
                        self.snapshot(|this| {
                            templates.iter().zip(args.iter()).try_for_each(
                                |(infer_ty, t_ref)| {
                                    let t_ref = this.allocate_type_ref(
                                        t_ref,
                                        this.implicit_ctx().as_ref(),
                                    );
                                    this.unify(infer_ty.clone(), t_ref)
                                },
                            )
                        })
                        .ok()?;
                        Some((struct_id, templates))
                    }
                    _ => None,
                }
            }
            PartialTypeRef::WithHoles {
                def: TypeDefId::Struct(struct_id),
                args,
            } => {
                let struct_id = *struct_id;
                let templates = templates_of_struct(self.db, struct_id.interned());
                let templates = templates
                    .iter()
                    .map(|_| self.fresh_var().into())
                    .collect::<Box<[InferTy]>>();

                self.snapshot(|this| {
                    args.iter()
                        .map(|arg| {
                            this.allocate_partial_type_arg(
                                arg,
                                this.implicit_ctx().as_ref(),
                            )
                        })
                        .collect::<Box<[_]>>()
                        .into_iter()
                        .zip(templates.iter())
                        .try_for_each(|(t_ref, infer_ty)| {
                            this.unify(infer_ty.clone(), t_ref)
                        })
                })
                .ok()?;
                Some((struct_id, templates))
            }
            _ => None,
        }
    }

    fn infer_struct_lit(
        &mut self,
        ty: &PartialTypeRef,
        fields: &[(Symbol, HirExpr)],
        span: Span,
    ) -> Result<InferTy, UnificationError> {
        if let Some((struct_id, templates)) = self.allocate_struct_partial_ref(ty) {
            let ast = struct_item(self.db, struct_id.interned());
            let inferred_fields = fields
                .iter()
                .map(|(name, expr)| self.infer_expr(expr).map(|res| (*name, res)))
                .collect::<Result<HashMap<_, _>, _>>()?;

            self.diagnose_bad_struct_fields(
                fields,
                span,
                &ast,
                &inferred_fields,
                struct_id,
            )?;

            let module = struct_id.parent(self.db);
            let zelf = self.fresh_var();

            let ctx = ImplicitContext::new(
                self.db,
                ScopeOwnerId::Module(module),
                templates_of_struct(self.db, struct_id.interned())
                    .iter()
                    .cloned()
                    .collect(),
                templates.iter().cloned().collect(),
                Some(InferTy::Var(zelf)),
            )
            .inspect_err(|err| {
                let loc_info = span.start().loc_info(self.db);
                println!("{loc_info}: {err:#?}")
            })
            .unwrap();

            self.snapshot(|this| {
                ast.fields.iter().try_for_each(|ast| {
                    let ty = inferred_fields.get(&ast.name).unwrap().clone();
                    let resolved =
                        this.allocate_ast_type_expr(&ast.ty.data, &ctx).unwrap();
                    this.unify(ty, resolved)
                })
            })?;

            let as_struct = InferTy::Adt {
                def: TypeDefId::Struct(struct_id),
                fields: templates,
            };

            self.unify(InferTy::Var(zelf), as_struct.clone())?;

            Ok(as_struct)
        } else {
            Err(UnificationError::NonStructForStructLit(ty.clone()))
        }
    }

    fn diagnose_bad_struct_fields(
        &mut self,
        _fields: &[(Symbol, HirExpr)],
        _span: Span,
        ast: &Arc<AstStructDef>,
        inferred_fields: &HashMap<Symbol, InferTy>,
        _struct_id: StructId,
    ) -> Result<(), UnificationError> {
        let field_sets = (
            inferred_fields.keys().copied().collect::<HashSet<_>>(),
            ast.fields.iter().map(|f| f.name).collect::<HashSet<_>>(),
        );
        if field_sets.0 != field_sets.1 {
            // For ast fields not in inferred fields
            for _field in field_sets.1.difference(&field_sets.0) {
                // self.diagnostics.push(Diagnostic {
                //     kind: DiagnosticKind::UniError {
                //         err: UnificationError::IncompleteStructLit {
                //             id: struct_id,
                //             missing: *field,
                //         },
                //         message: format!("Missing field in struct lit"),
                //     },
                //     span: span,
                // });
                // println!(
                //     "[Info]: Missing field in struct lit: {}",
                //     field.display(self.db)
                // );
                todo!("Diagnostics")
            }
            // For inferred fields not in ast fields
            for _field in field_sets.0.difference(&field_sets.1) {
                todo!("Diagnostics");
                // self.diagnostics.push(Diagnostic {
                //     kind: DiagnosticKind::UniError {
                //         err: UnificationError::InvalidStructField {
                //             id: struct_id,
                //             invalid: *field,
                //         },
                //         message: format!("Invalid field in struct lit"),
                //     },
                //     span: fields
                //         .iter()
                //         .find(|(name, _)| name == field)
                //         .expect("field should be in fields")
                //         .1
                //         .span
                //         .clone(),
                // });
                // println!(
                //     "[Info]: Invalid field in struct lit: {}",
                //     field.display(self.db)
                // );
            }
            Err(UnificationError::AlreadyDiagnosed)
        } else {
            Ok(())
        }
    }

    fn diagnose_field_mismatches(
        &mut self,
        fields: &[(Symbol, HirExpr)],
        ast_fields: &[AstStructDefField],
    ) {
        let expected_fields: HashSet<Symbol> =
            HashSet::from_iter(ast_fields.iter().map(|f| f.name));
        let got_fields: HashSet<Symbol> = HashSet::from_iter(fields.iter().map(|f| f.0));
        if expected_fields != got_fields {
            todo!()
        }
    }

    fn infer_constructor(
        &mut self,
        enum_def: EnumId,
        name: Symbol,
        args: &HirConstructorArgs,
        template_hints: &[PartialTypeArg],
    ) -> Result<InferTy, UnificationError> {
        let variants = &enum_item(self.db, enum_def.interned()).variants;
        let Some(variant) = variants.iter().find(|variant| variant.name == name) else {
            todo!(
                "report unknown variant `{}` in type `{}`",
                name.display(self.db),
                enum_def.name(self.db).display(self.db)
            )
        };

        let enum_templates = templates_of_enum(self.db, enum_def.interned());

        let templates: Arc<[InferTy]> = enum_templates
            .iter()
            .enumerate()
            .map(|(i, _)| {
                // TODO: handle conformances for t
                let var: InferTy = self.fresh_var().into();
                if let Some(hint) = template_hints.get(i) {
                    let allocated =
                        self.allocate_partial_type_arg(hint, &self.implicit_ctx());
                    self.unify(allocated, var.clone())?;
                }
                Ok(var)
            })
            .try_collect()?;

        let zelf = self.fresh_var();

        let ctx = ImplicitContext::new(
            self.db,
            ScopeOwnerId::Module(enum_def.parent(self.db)),
            enum_templates.iter().cloned().collect(),
            templates.clone(),
            Some(zelf.into()),
        )
        .unwrap();

        match (args, &variant.kind) {
            (
                HirConstructorArgs::TupleLike(hir_exprs),
                AstEnumVariantKind::TupleLike(spanneds),
            ) => {
                assert_eq!(hir_exprs.len(), spanneds.len());
                for (hir, ast) in hir_exprs.iter().zip(spanneds) {
                    let hir_ty = self.infer_expr(hir)?;
                    let in_ctx = self.allocate_ast_type_expr(&ast.data, &ctx).unwrap();
                    self.unify(hir_ty, in_ctx)?;
                }
            }
            (
                HirConstructorArgs::StructLike { fields },
                AstEnumVariantKind::StructLike(ast_fields),
            ) => {
                let mut errors = vec![];
                self.diagnose_field_mismatches(fields, ast_fields);
                for field in fields {
                    let ast = ast_fields.iter().find(|ast| ast.name == field.0);
                    let field_ty = ast
                        .and_then(|ast| self.allocate_ast_type_expr(&ast.ty.data, &ctx))
                        .unwrap_or_else(|| self.fresh_var().into());
                    match self.infer_expr(&field.1) {
                        Ok(expr_ty) => {
                            if let Err(err) = self.unify(expr_ty, field_ty) {
                                errors.push(err);
                            }
                        }
                        Err(err) => errors.push(err),
                    }
                }
                assert!(errors.is_empty());
            }
            (HirConstructorArgs::None, AstEnumVariantKind::Unit) => (),
            _ => todo!("handle constructor kind mismatch between decl and use"),
        }

        let as_enum = InferTy::Adt {
            def: TypeDefId::Enum(enum_def),
            fields: templates.iter().cloned().collect(),
        };
        self.unify(InferTy::Var(zelf), as_enum.clone())?;
        Ok(InferTy::Var(zelf))
    }

    fn infer_static(
        &mut self,
        id: ExprId,
        ty: &PartialTypeRef,
        method: Symbol,
        args: &[HirExpr],
    ) -> Result<InferTy, UnificationError> {
        let receiver_ty =
            self.allocate_partial_type_ref(ty, self.implicit_ctx().as_ref());
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

    fn infer_method(
        &mut self,
        id: ExprId,
        receiver: &HirExpr,
        method: Symbol,
        args: &[HirExpr],
        interface_hint: Option<InterfaceId>,
    ) -> Result<InferTy, UnificationError> {
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
        )
        .unwrap();

        self.allocate_ast_type_expr(&ast_ret_ty.data, &ctx).unwrap()
    }

    fn infer_direct(
        &mut self,
        id: ExprId,
        target: FunctionId,
        args: &[HirExpr],
    ) -> Result<InferTy, UnificationError> {
        self.check_args_count(target, args.len())?;
        let (receiver, ast_args) = target.args(self.db);
        assert!(receiver.is_none());
        let templates = get_templates_of_fun(self.db, target.interned());
        let inferred_templates = templates
            .iter()
            .map(|_| InferTy::Var(self.fresh_var()))
            .collect::<Arc<[_]>>();

        let ctx = ImplicitContext::from_function(
            self.db,
            target,
            inferred_templates.clone(),
            None,
        )
        .unwrap();

        let inferred_ast_args = ast_args
            .iter()
            .map(|arg| self.allocate_ast_type_expr(&arg.ty.data, &ctx).unwrap())
            .collect::<Box<[_]>>();
        let inferred_args = args
            .iter()
            .map(|arg| self.infer_expr(arg))
            .collect::<Result<Box<[_]>, _>>()?;
        inferred_ast_args
            .into_iter()
            .zip(inferred_args)
            .try_for_each(|(a, b)| self.unify(a, b))?;

        self.call_infos.insert(
            id,
            InferCallInfos {
                expr_id: id,
                callee: target,
                substitution: inferred_templates.iter().cloned().collect(),
            },
        );

        Ok(self.get_ret_ty(
            target,
            &inferred_templates,
            None, // TODO: Same
        ))
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
