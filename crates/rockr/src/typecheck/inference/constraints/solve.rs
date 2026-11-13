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

use crate::{
    hir::{FunctionLikeAst, impl_items},
    layout::{LIRTy, ScalarKind, layout_of},
    model::{
        BuiltinTypeId, CastClass, FunctionId, ImplSource, InterfaceId, PtrKind,
        ScopeOwnerId, TyLike, TypeDefId, TypeId, TypeRef, concrete_ty::int_ty_with_width,
    },
    typecheck::{
        CallKind, ExprId, InferCallInfos, ReceiverAdjustment,
        conformance::{MethodImpl, method_impl_for},
        inference::{
            InferTy, InferenceCtx, TypeInferenceError,
            constraints::{ConstraintSolveResult, MAX_IMPL_DEPTH, MethodConstraint},
            implems::PotentialBlockRes,
            implicit::{AsAstImplCtx, ImplicitContext},
            var::InferVar,
        },
    },
};
use itertools::Itertools;
use rockr_base::{Db, symbol::Symbol};
use rockr_constraint_solver::{ConstraintId, SolverHostExt};
use rockr_syntax::parse_tree::{
    expr::BinaryOperator,
    top_level::{AstImplItem, AstReceiver},
};
use std::collections::HashMap;
use std::sync::Arc;

impl<'db> InferenceCtx<'db> {
    pub(crate) fn solve_deref_constraint(
        &mut self,
        var: InferVar,
        target: &InferTy,
    ) -> ConstraintSolveResult {
        if let Some(value) = self.table.probe_value(var) {
            match value {
                InferTy::Var(_) => ConstraintSolveResult::Pending,
                InferTy::Adt { def, args: fields } => {
                    if def.is_ptr_like(self.db).is_none() || fields.len() != 1 {
                        return ConstraintSolveResult::Error(
                            TypeInferenceError::ExpectedPtrLike(def),
                        );
                    }
                    let Some(field) = fields.first() else {
                        return ConstraintSolveResult::Error(
                            TypeInferenceError::AlreadyDiagnosed,
                        );
                    };

                    self.unify(target, field).into()
                }
                InferTy::Param(id) => ConstraintSolveResult::Error(
                    TypeInferenceError::TemplateDereferencing(id),
                ),
                InferTy::Error => {
                    ConstraintSolveResult::Error(TypeInferenceError::AlreadyDiagnosed)
                }
            }
        } else {
            ConstraintSolveResult::Pending
        }
    }

    pub(crate) fn solve_binds_like_constraint(
        &mut self,
        ty: InferVar,
        inner: &InferTy,
        like: InferVar,
    ) -> ConstraintSolveResult {
        let Some(InferTy::Adt { def, .. }) = self.table.probe_value(like) else {
            return ConstraintSolveResult::Pending;
        };
        let to_unify = if let Some(PtrKind::Ref(mutability)) = def.is_ptr_like(self.db) {
            &self.ref_of(inner.clone(), mutability)
        } else {
            inner
        };
        self.unify(&ty.into(), to_unify).into()
    }

    pub(crate) fn solve_indexed_by_constraint(
        &mut self,
        elem_var: InferVar,
        base_ty: &InferTy,
        index_ty: &InferTy,
    ) -> ConstraintSolveResult {
        let found = self.find(base_ty);
        if let Some(elem_ty) = self.is_builtin_indexed_by_int(&found) {
            self.unify(&elem_var.into(), &elem_ty)?;
            let idx = self.emit_intlike_constraint();
            self.unify(index_ty, &idx.into()).into()
        } else if found.as_adt().is_some() {
            todo!("Trait-based indexing for {}", found.to_string(self.db))
        } else {
            ConstraintSolveResult::Pending
        }
    }

    pub(crate) fn solve_tuple_constraint(
        &mut self,
        elem_var: InferVar,
        tuple_ty: &InferTy,
        has_index: u32,
    ) -> ConstraintSolveResult {
        let found = self.find(tuple_ty);
        if let Some(fields) = found.as_tuple(self.db) {
            if fields.len() <= has_index as usize {
                return ConstraintSolveResult::Error(
                    TypeInferenceError::MinTupleLengthMismatch {
                        expected: has_index as usize,
                        got: fields.len(),
                    },
                );
            }
            self.unify(&elem_var.into(), &fields[has_index as usize]).into()
        } else if let Some((def, _)) = found.as_adt() {
            ConstraintSolveResult::Error(TypeInferenceError::TypeDefIdMismatch(
                def,
                TypeDefId::Builtin(BuiltinTypeId::tuple(self.db)),
            ))
        } else {
            ConstraintSolveResult::Pending
        }
    }

    pub(crate) fn solve_struct_field_constraint(
        &mut self,
        elem_var: InferVar,
        struct_ty: &InferTy,
        field: Symbol,
    ) -> ConstraintSolveResult {
        let found = self.find(struct_ty);
        if let Some((struct_id, fields)) = self.is_struct(&found) {
            if let Some(ty) = &fields.get(&field) {
                self.unify(&elem_var.into(), ty).into()
            } else {
                ConstraintSolveResult::Error(
                    TypeInferenceError::ExpectedStructWithField {
                        def: TypeDefId::Struct(struct_id),
                        field,
                    },
                )
            }
        } else if let Some((def, _)) = found.as_adt() {
            ConstraintSolveResult::Error(TypeInferenceError::ExpectedStructWithField {
                def,
                field,
            })
        } else {
            println!("Pending here ! found type to be {}", self.show(&found));
            ConstraintSolveResult::Pending
        }
    }

    pub(crate) fn solve_binop_constraint(
        &mut self,
        res_ty: InferVar,
        lhs_ty: &InferTy,
        rhs_ty: &InferTy,
        op: BinaryOperator,
    ) -> ConstraintSolveResult {
        let lhs_ty = self.find(lhs_ty);
        let rhs_ty = self.find(rhs_ty);

        // Is one of them a builtin arithmetic type ?
        // if yes: handle that case specifically
        // otherwise, both must be of the same type
        // and this type must implement the <op> interface
        // or something etc...

        if let Some((lid, _)) = lhs_ty.as_adt() {
            if let Some((rid, _)) = rhs_ty.as_adt()
                && let Some(lid) = lid.is_int_like(self.db)
                && let Some(rid) = rid.is_int_like(self.db)
            {
                return self.solve_int_binop(res_ty, lid, rid, op);
            }
            if let InferTy::Var(_) = rhs_ty
                && let Some(lid) = lid.is_int_like(self.db)
            {
                self.unify(&lhs_ty, &rhs_ty)?;
                return self.solve_int_binop(res_ty, lid, lid, op);
            }
        }

        match (&lhs_ty, &rhs_ty) {
            (InferTy::Var(_), InferTy::Var(_)) => ConstraintSolveResult::Pending,
            (InferTy::Var(v), InferTy::Adt { def, args: fields })
            | (InferTy::Adt { def, args: fields }, InferTy::Var(v))
                if fields.is_empty()
                    && let Some(int_like) = def.is_int_like(self.db) =>
            {
                self.unify(&v.into(), &rhs_ty)?;
                self.solve_int_binop(res_ty, int_like, int_like, op)
            }
            (lhs_ty, rhs_ty) => ConstraintSolveResult::Error(
                TypeInferenceError::NonIntegerTypesInBitwiseOperation {
                    lhs: self.find(lhs_ty).to_string(self.db),
                    rhs: self.find(rhs_ty).to_string(self.db),
                },
            ),
        }
    }

    fn solve_int_binop(
        &mut self,
        res_ty: InferVar,
        lid: BuiltinTypeId,
        rid: BuiltinTypeId,
        op: BinaryOperator,
    ) -> ConstraintSolveResult {
        match op {
            BinaryOperator::Diff
            | BinaryOperator::Eq
            | BinaryOperator::Geq
            | BinaryOperator::Leq
            | BinaryOperator::Gt
            | BinaryOperator::Lt => {
                // We know they are int-like, so it is safe to just say
                // that res_ty must be bool
                if let Err(err) = self.unify(&res_ty.into(), &self.bool_ty()) {
                    return ConstraintSolveResult::Error(err);
                }
                ConstraintSolveResult::Solved
            }
            BinaryOperator::Plus | BinaryOperator::Minus | BinaryOperator::Times => {
                if lid == rid {
                    let ty = InferTy::simple(TypeDefId::Builtin(lid));
                    self.unify(&res_ty.into(), &ty).into()
                } else {
                    let llayout = layout_of(
                        self.db,
                        TypeId::new(self.db, TypeDefId::Builtin(lid), vec![]).into(),
                    );
                    let rlayout = layout_of(
                        self.db,
                        TypeId::new(self.db, TypeDefId::Builtin(rid), vec![]).into(),
                    );
                    let lty = LIRTy { layout: llayout, origin: None };
                    let rty = LIRTy { layout: rlayout, origin: None };
                    if let Some(ScalarKind::Int(lwidth)) = lty.scalar(self.db)
                        && let Some(ScalarKind::Int(rwidth)) = rty.scalar(self.db)
                    {
                        let max_width = lwidth.max(rwidth);
                        let ty = int_ty_with_width(self.db, max_width);
                        let infer_ty = InferTy::simple(ty.def(self.db));
                        self.unify(&res_ty.into(), &infer_ty)?;
                        return ConstraintSolveResult::Solved;
                    }
                    todo!()
                }
            }
            BinaryOperator::Div | BinaryOperator::Modulo => {
                let lty = InferTy::simple(TypeDefId::Builtin(lid));
                let rty = InferTy::simple(TypeDefId::Builtin(rid));
                self.unify(&lty, &rty)?;
                self.unify(&lty, &res_ty.into()).into()
            }
            BinaryOperator::And => todo!(),
            BinaryOperator::Or => todo!(),
            BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor => {
                let lty = InferTy::Adt { def: TypeDefId::Builtin(lid), args: Vec::new() };
                let rty = InferTy::Adt { def: TypeDefId::Builtin(rid), args: Vec::new() };
                self.unify(&lty, &rty)?;
                self.unify(&lty, &res_ty.into())?;
                ConstraintSolveResult::Solved
            }
        }
    }

    fn resolve_with_auto_deref(
        &mut self,
        receiver: &InferTy,
        method: Symbol,
        interface_hint: Option<InterfaceId>,
        arity: usize,
        is_static: bool,
    ) -> Option<(usize, HashMap<ImplSource<'db>, PotentialBlockRes>)> {
        let mut cur = self.find(receiver);
        let mut depth = 0;
        loop {
            let blocks = self.compute_possible_blocks(
                &cur,
                method,
                interface_hint,
                arity,
                is_static,
            );
            if !blocks.is_empty() {
                return Some((depth, blocks));
            }
            if let Some((_, inner)) = cur.ptr_like(self.db) {
                cur = inner.clone();
                depth += 1;
            } else {
                return None;
            }
        }
    }

    pub(super) fn get_adjustments_for(
        &self,
        mthd: FunctionId,
        depth: usize,
    ) -> ReceiverAdjustment {
        match mthd.receiver(self.db) {
            // Error here but best to return that
            AstReceiver::None => ReceiverAdjustment::None,

            AstReceiver::Zelf(_)
            | AstReceiver::MutZelf(_)
            | AstReceiver::PtrZelf(_)
            | AstReceiver::MutPtrZelf(_) => {
                if depth == 0 {
                    ReceiverAdjustment::None
                } else {
                    ReceiverAdjustment::Deref(depth)
                }
            }
            AstReceiver::RefZelf(_) => {
                if depth == 0 {
                    ReceiverAdjustment::Ref
                } else {
                    ReceiverAdjustment::DerefThenRef(depth)
                }
            }
            AstReceiver::MutRefZelf(_) => {
                if depth == 0 {
                    ReceiverAdjustment::MutRef
                } else {
                    ReceiverAdjustment::DerefThenMutRef(depth)
                }
            }
        }
    }

    pub(super) fn peel_receiver(&mut self, receiver: &InferTy, depth: usize) -> InferTy {
        let mut zelf_ty = self.find(receiver);
        for _ in 0..depth {
            if let Some(adt) = zelf_ty.as_adt() {
                zelf_ty = adt.1[0].clone();
            } else {
                let pointee = self.fresh_var();
                let ptr = self.emit_deref_constraint(pointee.into());
                self.unify(&zelf_ty, &ptr.into())
                    .expect("Fresh var should never fail unifying");
                zelf_ty = pointee.into();
            }
        }
        zelf_ty
    }

    fn finish_method_call(
        &mut self,
        expr_id: ExprId,
        receiver: &InferTy,
        method_call_infos: &MethodCallInfos<'_>,
    ) -> ConstraintSolveResult {
        let FunctionLikeAst::Method(ast) = method_call_infos.method_id.ast(self.db)
        else {
            unreachable!()
        };
        let method_templates = method_call_infos
            .templates
            .iter()
            .map(Into::into)
            .chain(ast.data.template_args.iter().map(|ast_template| {
                if !ast_template.constraints.is_empty() {
                    todo!()
                }
                self.fresh_var().into()
            }))
            .collect_vec();

        let zelf_ty = self.peel_receiver(receiver, method_call_infos.depth);

        let method_ctx = ImplicitContext::from_function(
            self.db,
            method_call_infos.method_id,
            method_templates.iter().cloned().collect(),
            Some(zelf_ty.clone()),
        );

        method_call_infos.args.iter().zip(&ast.data.args).try_for_each(
            |(arg, ast_ty)| {
                let arg_ty = self.allocate_ast_type_expr(&ast_ty.ty.data, &method_ctx);
                self.unify(arg, &arg_ty)
            },
        )?;

        let ret_ty = self.allocate_type_ref(
            method_ctx
                .resolve(self.db, &ast.data.return_type.data)
                .unwrap_or(TypeRef::Error),
            &method_ctx,
        );

        self.unify(&method_call_infos.ret_var.into(), &ret_ty)?;

        let call_infos = InferCallInfos {
            expr_id,
            callee: method_call_infos.method_id,
            substitution: method_templates,
            call_kind: if method_call_infos.is_static {
                CallKind::Static
            } else {
                CallKind::Method {
                    adjustment: self.get_adjustments_for(
                        method_call_infos.method_id,
                        method_call_infos.depth,
                    ),
                }
            },
            zelf_ty: Some(zelf_ty),
        };

        self.insert_call_info(expr_id, call_infos);
        ConstraintSolveResult::Solved
    }

    pub(crate) fn solve_method_constraint(
        &mut self,
        method_constraint: &MethodConstraint,
    ) -> ConstraintSolveResult {
        let MethodConstraint {
            ret_var,
            ty: receiver,
            id,
            method,
            args,
            interface_hint,
            is_static,
        } = method_constraint;

        if self.call_infos.contains_key(id) {
            return ConstraintSolveResult::Solved;
        }

        if let Some(tid) = receiver.as_concrete(self.db) {
            let mut ty = tid;
            let mut depth = 0;
            loop {
                if let Some(value) = self.try_known_concrete_impls(
                    *ret_var,
                    receiver,
                    *id,
                    *method,
                    args,
                    *interface_hint,
                    *is_static,
                    ty,
                    depth,
                ) {
                    return value;
                }
                match concrete_deref_target(self.db, ty) {
                    Some(inner) => {
                        ty = inner;
                        depth += 1;
                    }
                    None => break,
                }
            }
        }

        if let Some(result) = self.try_resolve_via_known_impl(method_constraint) {
            return result;
        }

        let Some((depth, possible_blocks)) = self.resolve_with_auto_deref(
            receiver,
            *method,
            *interface_hint,
            args.len(),
            *is_static,
        ) else {
            return ConstraintSolveResult::Pending;
        };

        if possible_blocks.is_empty() {
            return ConstraintSolveResult::Error(TypeInferenceError::Custom(format!(
                "Could not find an implementation for {} with arity {} on {}",
                method.display(self.db),
                args.len(),
                self.find(receiver).to_string(self.db)
            )));
        }

        if possible_blocks.len() > 1 {
            return ConstraintSolveResult::Pending;
        }
        let (src, PotentialBlockRes { templates, constraints }) = possible_blocks
            .into_iter()
            .next()
            .expect("Earlier conditions ensure that we have exactly one possible block");
        for constraint in constraints {
            self.emit_constraint(constraint);
        }

        let method_id =
            FunctionId::new(self.db, *method, ScopeOwnerId::Impl(*src.id(self.db)));

        let ast = impl_items(self.db, src.id(self.db).interned())
            .iter()
            .find_map(|item| match item {
                AstImplItem::Fundef(def) if def.data.name.data == *method => Some(def),
                _ => None,
            })
            .expect(
                "If name lookup is correct, we should find the corresponding AST node",
            );

        if ast.data.receiver.is_static() != *is_static {
            return ConstraintSolveResult::Error(
                TypeInferenceError::StaticMethodCallOnReceiver(*id, method_id),
            );
        }

        if ast.data.args.len() != args.len() {
            return ConstraintSolveResult::Error(TypeInferenceError::ArgCountMismatch(
                method_id,
                args.len(),
            ));
        }

        self.finish_method_call(
            *id,
            receiver,
            &MethodCallInfos {
                method_id,
                args,
                ret_var: *ret_var,
                is_static: *is_static,
                templates: &templates,
                depth,
            },
        )
    }

    fn try_known_concrete_impls(
        &mut self,
        ret_var: InferVar,
        receiver: &InferTy,
        id: ExprId,
        method: Symbol,
        args: &[InferTy],
        interface_hint: Option<InterfaceId>,
        is_static: bool,
        ty: TypeId,
        depth: usize,
    ) -> Option<ConstraintSolveResult> {
        if let Some(MethodImpl { method_id, subs, .. }) =
            method_impl_for(self.db, ty, method, args.len(), is_static, interface_hint)
        {
            let templates = subs
                .iter()
                .map(|tid| {
                    let var = self.fresh_var();
                    if let Some(infer_ty) = concrete_to_infer(self.db, *tid) {
                        self.unify(&infer_ty, &var.into())
                            .expect("A fresh var should never fail to unify");
                    }
                    var
                })
                .collect_vec();

            return Some(self.finish_method_call(
                id,
                receiver,
                &MethodCallInfos {
                    method_id: *method_id,
                    args,
                    ret_var,
                    is_static,
                    templates: &templates,
                    depth,
                },
            ));
        }
        None
    }

    pub(crate) fn solve_implements_constraint(
        &mut self,
        _id: ConstraintId,
        ty: &InferTy,
        interface_id: InterfaceId,
        args: &[InferTy],
    ) -> ConstraintSolveResult {
        let key = (interface_id, self.canonize(ty));
        if self.in_flight_impls.contains(&key) {
            return ConstraintSolveResult::Pending; // assume it'll work; break the cycle
        }
        self.in_flight_impls.insert(key.clone());

        if self.impl_depth >= MAX_IMPL_DEPTH {
            return ConstraintSolveResult::Pending;
        }

        self.impl_depth += 1;
        let result = self.solve_implements_constraint_impl(ty, interface_id, args);
        self.in_flight_impls.remove(&key);
        self.impl_depth -= 1;
        result
    }

    fn solve_implements_constraint_impl(
        &mut self,
        ty: &InferTy,
        interface_id: InterfaceId,
        args: &[InferTy],
    ) -> ConstraintSolveResult {
        let ty = &self.find(ty);

        if self.has_implementation(interface_id, ty, args) {
            return ConstraintSolveResult::Solved;
        }

        // TODO: remove implementation when erroring out, maybe
        self.add_implementation(interface_id, ty, args);

        let impls = self.get_potential_blocks(ty);

        let competing_impls = impls
            .into_iter()
            .filter(|(src, _)| {
                src.id(self.db)
                    .interface(self.db)
                    .is_some_and(|this_id| this_id.def(self.db) == interface_id)
            })
            .collect::<HashMap<_, _>>();
        let competing_impls = if competing_impls.is_empty() {
            competing_impls
        } else {
            self.get_working_impls(competing_impls)
        };
        let mut competing_impls = competing_impls.into_iter();
        let impl_ = match (competing_impls.next(), competing_impls.next()) {
            (Some((_, impl_)), None) => impl_,
            (None, _) => {
                return ConstraintSolveResult::Error(
                    TypeInferenceError::NoImplemCandidateFor(
                        self.find(ty),
                        interface_id,
                        args.iter().cloned().collect(),
                    ),
                );
            }
            (Some(_), Some(_)) => return ConstraintSolveResult::Pending,
        };
        for constraint in impl_.constraints {
            self.emit_constraint(constraint);
        }

        self.solve_constraints().map_err(|(cs, err)| {
            TypeInferenceError::UnmetConstraint(Arc::new(cs.kind.clone()), Box::new(err))
        })?;
        ConstraintSolveResult::Solved
    }

    pub(crate) fn solve_fat_ptr_constraint(
        &mut self,
        fat_ptr_var: InferVar,
    ) -> ConstraintSolveResult {
        let fat_ptr_ty = self.find(&fat_ptr_var.into());
        if fat_ptr_ty.as_slice(self.db).is_some() {
            return ConstraintSolveResult::Solved;
        }
        ConstraintSolveResult::Pending
    }

    pub(crate) fn solve_metadata_of_fat_ptr_constraint(
        &mut self,
        fat_ptr_var: InferVar,
        metadata_var: InferVar,
    ) -> ConstraintSolveResult {
        let fat_ptr_ty = self.find(&fat_ptr_var.into());

        // Only fat ptr type supported for now is ref to slices
        if fat_ptr_ty.as_slice(self.db).is_none() {
            return ConstraintSolveResult::Error(TypeInferenceError::Custom(format!(
                "Expected a fat ptr type but got {}",
                fat_ptr_ty.to_string(self.db)
            )));
        }

        self.unify(&metadata_var.into(), &self.usize_ty()).into()
    }

    pub(crate) fn solve_cast_constraint(
        &mut self,
        from: &InferTy,
        to: &InferTy,
    ) -> ConstraintSolveResult {
        let from = self.find(from);
        let to = self.find(to);
        if matches!(from, InferTy::Var(_)) || matches!(to, InferTy::Var(_)) {
            return ConstraintSolveResult::Pending;
        }

        let reject = |reason: &'static str| {
            ConstraintSolveResult::Error(TypeInferenceError::InvalidCast {
                from: from.clone(),
                to: to.clone(),
                reason,
            })
        };

        let (Some(from_class), Some(to_class)) =
            (self.infer_cast_class(&from), self.infer_cast_class(&to))
        else {
            return reject("only integer and pointer-like types can be cast");
        };

        match (from_class, to_class) {
            (CastClass::Int | CastClass::ThinPtr(_), CastClass::Int)
            | (CastClass::Int, CastClass::ThinPtr(_)) => ConstraintSolveResult::Solved,
            (
                CastClass::ThinPtr(from_m) | CastClass::FatPtr(from_m),
                CastClass::ThinPtr(to_m),
            )
            | (CastClass::FatPtr(from_m), CastClass::FatPtr(to_m)) => {
                if to_m.is_mut() && !from_m.is_mut() {
                    reject("a const pointer/reference cannot be cast to a mutable one")
                } else {
                    ConstraintSolveResult::Solved
                }
            }
            (CastClass::Int, CastClass::FatPtr(_)) => {
                reject("a fat pointer's metadata cannot be created by a cast")
            }
            (CastClass::ThinPtr(from_m), CastClass::FatPtr(to_m)) => {
                // test if we have &[T; N] -> &[T]
                if to_m.is_mut() && !from_m.is_mut() {
                    return reject(
                        "a const pointer/reference cannot be cast to a mutable one",
                    );
                }
                if let Some((inner_ty, _)) =
                    from.as_ref(self.db).and_then(|ty| ty.1.as_array(self.db))
                {
                    let Some((casted_inner_ty, _)) = to.as_slice(self.db) else {
                        unreachable!("Only slices are fat ptrs")
                    };
                    self.unify(inner_ty, casted_inner_ty)?;
                    ConstraintSolveResult::Solved
                } else {
                    ConstraintSolveResult::Pending
                }
            }
            (CastClass::FatPtr(_), CastClass::Int) => {
                reject("cast the fat pointer to a thin pointer first")
            }
        }
    }

    pub(crate) fn solve_unify_constraint(
        &mut self,
        a: &InferTy,
        b: &InferTy,
    ) -> ConstraintSolveResult {
        self.unify(a, b).into()
    }

    fn compute_possible_blocks(
        &mut self,
        receiver: &InferTy,
        method: Symbol,
        interface_hint: Option<InterfaceId>,
        arity: usize,
        is_static: bool,
    ) -> HashMap<ImplSource<'db>, PotentialBlockRes> {
        let mut possible_blocks = self
            .get_potential_blocks(receiver)
            .into_iter()
            .unique_by(|(src, _)| src.id(self.db))
            .filter(|(src, _)| {
                let items = impl_items(self.db, src.id(self.db).interned());
                for item in items {
                    if let AstImplItem::Fundef(def) = item
                        && def.data.name.data == method
                        && def.data.receiver.is_static() == is_static
                        && def.data.args.len() == arity
                    {
                        return true;
                    }
                }
                false
            })
            .collect::<Box<[_]>>();
        if let Some(id) = interface_hint {
            possible_blocks = possible_blocks
                .into_iter()
                .filter(|(src, _)| {
                    src.id(self.db).interface(self.db).is_some_and(|impl_interface_id| {
                        impl_interface_id.def(self.db) == id
                    })
                })
                .collect();
        }
        self.get_working_impls(possible_blocks)
    }
}

fn concrete_deref_target(db: &dyn Db, ty: TypeId) -> Option<TypeId> {
    let tref: TypeRef = ty.into();
    let (_, inner) = tref.as_ref(db).or_else(|| tref.as_ptr(db))?;
    inner.as_type_id()
}

fn concrete_to_infer(db: &dyn Db, ty: TypeId) -> Option<InferTy> {
    Some(InferTy::Adt {
        def: ty.def(db),
        args: ty
            .args(db)
            .iter()
            .map(|ty| concrete_to_infer(db, ty.as_type_id()?))
            .collect::<Option<_>>()?,
    })
}

struct MethodCallInfos<'a> {
    pub method_id: FunctionId,
    pub args: &'a [InferTy],
    pub ret_var: InferVar,
    pub is_static: bool,
    pub templates: &'a [InferVar],
    pub depth: usize,
}
