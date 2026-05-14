use std::collections::HashMap;

use crate::{
    common::symbols::Symbol,
    hir::{
        HirConstructorArgs, HirExpr, HirExprDesc, HirPlace, LocalId, PartialTypeArg, PartialTypeRef,
    },
    name_resolve::type_expr::{struct_item, templates_of_struct},
    ril::{EnumId, FunctionId, InterfaceId, StructId, TypeDefId, TypeRef},
};

use super::{InferTy, InferenceCtx, UnificationError};

impl<'db> InferenceCtx<'db> {
    fn _infer_expr(&mut self, expr: &HirExpr) -> Result<InferTy, UnificationError> {
        match &expr.data {
            HirExprDesc::IntLit(_) => Ok(self.int_ty()),
            HirExprDesc::CharLit(_) => Ok(self.char_ty()),
            HirExprDesc::StrLit(_) => Ok(self.str_ty()),
            HirExprDesc::CStrLit(_) => Ok(self.cstr_ty()),
            HirExprDesc::BoolLit(_) => Ok(self.bool_ty()),
            HirExprDesc::Use(place) => self.infer_place(place),
            HirExprDesc::AddressOf { place, .. } | HirExprDesc::Ref { place, .. } => {
                let place_ty = self.infer_place(place)?;
                Ok(self.some_ptr_to(place_ty))
            }
            HirExprDesc::CallDirect { target, args } => self.infer_direct(*target, args),
            HirExprDesc::CallMethod {
                receiver,
                method,
                args,
                interface_hint,
            } => self.infer_method(receiver, *method, args, *interface_hint),
            HirExprDesc::CallStatic { ty, method, args } => self.infer_static(ty, *method, args),
            HirExprDesc::BinOp { lhs, op, rhs } => self.infer_binop(lhs, *op, rhs),
            HirExprDesc::StructLit { ty, fields } => self.infer_struct_lit(ty, fields),
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
                    self.unify(InferTy::Var(elem_var), ty)?;
                    Ok(())
                })?;
                Ok(self.slice_of(InferTy::Var(elem_var)))
            }
            HirExprDesc::SizeOf(_) => Ok(self.int_ty()),
            HirExprDesc::Constructor {
                enum_def,
                name,
                args,
                template_hints,
            } => self.infer_constructor(*enum_def, *name, args, template_hints),
        }
    }

    fn _infer_place(&mut self, place: &HirPlace) -> Result<InferTy, UnificationError> {
        match place {
            HirPlace::Local(local_id) => Ok(self.infer_local(*local_id)),
            HirPlace::Field { base, field } => {
                let base_ty = self.infer_place(base)?;
                let elem_var = self.emit_struct_field_constraint(base_ty, *field);
                Ok(InferTy::Var(elem_var))
            }
            HirPlace::TupleField { base, index } => {
                let base_ty = self.infer_place(base)?;
                let elem_var = self.emit_tuple_constraint(base_ty, *index);
                Ok(InferTy::Var(elem_var))
            }
            HirPlace::Deref(hir_place) => {
                let ptr_ty = self.infer_place(hir_place)?;
                let pointee_var = self.fresh_var();
                let ptr_var = self.emit_deref_constraint(InferTy::Var(pointee_var));
                self.unify(InferTy::Var(ptr_var), ptr_ty)?;
                Ok(InferTy::Var(pointee_var))
            }
            HirPlace::Index { base, index } => {
                let index_ty = self.infer_expr(index)?;
                let base_ty = self.infer_place(base)?;
                let element_var = self.emit_indexed_by_constraint(base_ty, index_ty);
                Ok(InferTy::Var(element_var))
            }
            HirPlace::Temporary(hir_expr) => self.infer_expr(hir_expr),
        }
    }

    pub fn infer_place(&mut self, place: &HirPlace) -> Result<InferTy, UnificationError> {
        self.snapshot(|this| this._infer_place(place))
    }

    pub fn infer_expr(&mut self, expr: &HirExpr) -> Result<InferTy, UnificationError> {
        self.snapshot(|this| this._infer_expr(expr))
    }

    fn infer_local(&mut self, local_id: LocalId) -> InferTy {
        InferTy::Var(self.local_map[&local_id])
    }

    pub fn some_ptr_to(&mut self, pointee: InferTy) -> InferTy {
        InferTy::Var(self.emit_deref_constraint(pointee))
    }

    fn allocate_struct_partial_ref(
        &mut self,
        type_ref: &PartialTypeRef,
    ) -> Option<(StructId, Box<[InferTy]>)> {
        match type_ref {
            PartialTypeRef::Resolved(TypeRef::Concrete(type_id)) => match type_id.def(self.db) {
                TypeDefId::Struct(struct_id) => {
                    let templates = templates_of_struct(self.db, struct_id.interned());
                    let templates = templates
                        .iter()
                        .map(|_| InferTy::Var(self.fresh_var()))
                        .collect::<Box<[_]>>();
                    let args = type_id.args(self.db);
                    self.snapshot(|this| {
                        templates
                            .iter()
                            .zip(args.iter())
                            .try_for_each(|(infer_ty, t_ref)| {
                                let t_ref =
                                    this.allocate_type_ref(t_ref, this.templates().as_ref());
                                this.unify(infer_ty.clone(), t_ref)
                            })
                    })
                    .ok()?;
                    Some((struct_id, templates))
                }
                _ => None,
            },
            PartialTypeRef::WithHoles {
                def: TypeDefId::Struct(struct_id),
                args,
            } => {
                let struct_id = *struct_id;
                let templates = templates_of_struct(self.db, struct_id.interned());
                let templates = templates
                    .iter()
                    .map(|_| InferTy::Var(self.fresh_var()))
                    .collect::<Box<[_]>>();

                self.snapshot(|this| {
                    args.iter()
                        .map(|arg| this.allocate_partial_type_arg(arg, this.templates().as_ref()))
                        .collect::<Box<[_]>>()
                        .into_iter()
                        .zip(templates.iter())
                        .try_for_each(|(t_ref, infer_ty)| this.unify(infer_ty.clone(), t_ref))
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
    ) -> Result<InferTy, UnificationError> {
        if let Some((struct_id, templates)) = self.allocate_struct_partial_ref(ty) {
            let ast = struct_item(self.db, struct_id.interned());
            let inferred_fields = fields
                .iter()
                .map(|(name, expr)| self.infer_expr(expr).map(|res| (*name, res)))
                .collect::<Result<HashMap<_, _>, _>>()?;
            if ast.fields.len() != inferred_fields.len() {
                let missing = ast
                    .fields
                    .iter()
                    .find(|field| inferred_fields.get(&field.name).is_none())
                    .unwrap()
                    .name;
                return Err(UnificationError::IncompleteStructLit {
                    id: struct_id,
                    missing,
                });
            }

            let module = struct_id.parent(self.db);
            let ast_template_args = &ast.template_args;

            self.snapshot(|this| {
                ast.fields.iter().try_for_each(|ast| {
                    let ty = inferred_fields.get(&ast.name).unwrap().clone();
                    let resolved = this
                        .allocate_ast_type_expr(&ast.ty, module, ast_template_args, &templates)
                        .unwrap();
                    this.unify(ty, resolved)
                })
            })?;

            Ok(InferTy::Adt {
                def: TypeDefId::Struct(struct_id),
                fields: templates,
            })
        } else {
            Err(UnificationError::NonStructForStructLit(ty.clone()))
        }
    }

    fn infer_constructor(
        &mut self,
        _enum_def: EnumId,
        _name: Symbol,
        _args: &HirConstructorArgs,
        _template_hints: &[PartialTypeArg],
    ) -> Result<InferTy, UnificationError> {
        todo!()
    }

    fn infer_static(
        &self,
        _ty: &PartialTypeRef,
        _method: Symbol,
        _args: &[HirExpr],
    ) -> Result<InferTy, UnificationError> {
        todo!()
    }

    fn infer_method(
        &self,
        _receiver: &HirExpr,
        _method: Symbol,
        _args: &[HirExpr],
        _interface_hint: Option<InterfaceId>,
    ) -> Result<InferTy, UnificationError> {
        todo!()
    }

    fn infer_direct(
        &self,
        _target: FunctionId,
        _args: &[HirExpr],
    ) -> Result<InferTy, UnificationError> {
        todo!()
    }

    fn infer_binop(
        &self,
        _lhs: &crate::hir::HirExpr,
        _op: crate::parse_tree::expr::BinaryOperator,
        _rhs: &crate::hir::HirExpr,
    ) -> Result<InferTy, UnificationError> {
        todo!()
    }
}
