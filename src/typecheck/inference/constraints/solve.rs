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

use std::{collections::HashMap, sync::Arc};

use itertools::Itertools;

use crate::{
    Db,
    common::symbols::Symbol,
    hir::{FunctionLikeAst, function_ast, impl_items},
    layout::{LIRTy, ScalarKind, layout_of},
    parse_tree::{
        expr::BinaryOperator,
        top_level::{AstImplItem, AstReceiver},
    },
    printer::type_printer::{TypePrinter, TypePrinterOption, TypePrinterOptionSet},
    resolved::{
        BuiltinTypeId, FunctionId, ImplSource, InterfaceId, PtrKind, ScopeOwnerId,
        TypeDefId, TypeId, TypeRef,
    },
    thir_to_mir::lower_match::int_ty_with_witdh,
    typecheck::{
        CallKind, ExprId, InferCallInfos, ReceiverAdjustment,
        conformance::{MethodImpl, method_impl_for},
        inference::{
            InferTy, InferenceCtx, UnificationError,
            constraints::{
                ConstraintSolveResult, InferenceConstraint, InferenceConstraintId,
                InferenceConstraintKind, MAX_IMPL_DEPTH, MethodConstraint,
            },
            implems::PotentialBlockRes,
            implicit::{AsAstImplCtx, ImplicitContext},
            var::InferVar,
        },
    },
};

impl<'db> InferenceCtx<'db> {
    fn solve_deref_constraint(
        &mut self,
        var: InferVar,
        target: &InferTy,
    ) -> ConstraintSolveResult {
        if let Some(value) = self.table.probe_value(var) {
            match value {
                InferTy::Var(_) => ConstraintSolveResult::Pending,
                InferTy::Adt { def, fields } => {
                    if def.is_ptr_like(self.db).is_none() || fields.len() != 1 {
                        return ConstraintSolveResult::Error(
                            UnificationError::ExpectedPtrLike(def),
                        );
                    }
                    let Some(field) = fields.first() else {
                        return ConstraintSolveResult::Error(
                            UnificationError::AlreadyDiagnosed,
                        );
                    };

                    self.unify(target, field).err().map_or(
                        ConstraintSolveResult::Solved,
                        ConstraintSolveResult::Error,
                    )
                }
                InferTy::Param(id) => ConstraintSolveResult::Error(
                    UnificationError::TemplateDereferencing(id),
                ),
            }
        } else {
            ConstraintSolveResult::Pending
        }
    }

    fn solve_binds_like_constraint(
        &mut self,
        ty: InferVar,
        inner: &InferTy,
        like: InferVar,
    ) -> ConstraintSolveResult {
        if let Some(InferTy::Adt { def, .. }) = self.table.probe_value(like) {
            let to_unify =
                if let Some(PtrKind::Ref(mutability)) = def.is_ptr_like(self.db) {
                    &InferTy::Adt {
                        def: BuiltinTypeId::ref_(self.db, mutability).into(),
                        fields: vec![inner.clone()],
                    }
                } else {
                    inner
                };
            if let Err(err) = self.unify(&ty.into(), to_unify) {
                ConstraintSolveResult::Error(err)
            } else {
                ConstraintSolveResult::Solved
            }
        } else {
            ConstraintSolveResult::Pending
        }
    }

    fn solve_indexed_by_constraint(
        &mut self,
        elem_var: InferVar,
        base_ty: &InferTy,
        index_ty: &InferTy,
    ) -> ConstraintSolveResult {
        let found = self.find(base_ty);
        if let Some(elem_ty) = self.is_builtin_indexed_by_int(&found) {
            if let Err(err) = self.unify(&elem_var.into(), &elem_ty) {
                return ConstraintSolveResult::Error(err);
            }
            let idx = self.emit_intlike_constraint();
            if let Err(err) = self.unify(index_ty, &idx.into()) {
                return ConstraintSolveResult::Error(err);
            }
            ConstraintSolveResult::Solved
        } else if found.as_adt().is_some() {
            todo!("Trait-based indexing")
        } else {
            ConstraintSolveResult::Pending
        }
    }

    fn solve_tuple_constraint(
        &mut self,
        elem_var: InferVar,
        tuple_ty: &InferTy,
        has_index: u32,
    ) -> ConstraintSolveResult {
        let found = self.find(tuple_ty);
        if let Some(fields) = self.is_tuple(&found) {
            if fields.len() <= has_index as usize {
                ConstraintSolveResult::Error(UnificationError::MinTupleLengthMismatch {
                    expected: has_index as usize,
                    got: fields.len(),
                })
            } else if let Err(err) =
                self.unify(&elem_var.into(), &fields[has_index as usize])
            {
                ConstraintSolveResult::Error(err)
            } else {
                ConstraintSolveResult::Solved
            }
        } else if let Some((def, _)) = found.as_adt() {
            ConstraintSolveResult::Error(UnificationError::TypeDefIdMismatch(
                def,
                TypeDefId::Builtin(BuiltinTypeId::tuple(self.db)),
            ))
        } else {
            ConstraintSolveResult::Pending
        }
    }

    fn solve_struct_field_constraint(
        &mut self,
        elem_var: InferVar,
        struct_ty: &InferTy,
        field: Symbol,
    ) -> ConstraintSolveResult {
        let found = self.find(struct_ty);
        if let Some((struct_id, fields)) = self.is_struct(&found) {
            if let Some(ty) = &fields.get(&field) {
                if let Err(err) = self.unify(&elem_var.into(), ty) {
                    ConstraintSolveResult::Error(err)
                } else {
                    ConstraintSolveResult::Solved
                }
            } else {
                ConstraintSolveResult::Error(UnificationError::ExpectedStructWithField {
                    def: TypeDefId::Struct(struct_id),
                    field,
                })
            }
        } else if let Some((def, _)) = found.as_adt() {
            ConstraintSolveResult::Error(UnificationError::ExpectedStructWithField {
                def,
                field,
            })
        } else {
            println!(
                "Pending here ! found type to be {}",
                self.find(&found).to_string(self.db)
            );
            ConstraintSolveResult::Pending
        }
    }

    fn solve_binop_constraint(
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
                if let Err(err) = self.unify(&lhs_ty, &rhs_ty) {
                    return ConstraintSolveResult::Error(err);
                }
                return self.solve_int_binop(res_ty, lid, lid, op);
            }
        }

        match (&lhs_ty, &rhs_ty) {
            (InferTy::Var(_), InferTy::Var(_)) => ConstraintSolveResult::Pending,
            (InferTy::Var(v), InferTy::Adt { def, fields })
            | (InferTy::Adt { def, fields }, InferTy::Var(v))
                if fields.is_empty()
                    && let Some(int_like) = def.is_int_like(self.db) =>
            {
                if let Err(err) = self.unify(&v.into(), &rhs_ty) {
                    return ConstraintSolveResult::Error(err);
                }
                self.solve_int_binop(res_ty, int_like, int_like, op)
            }
            (lhs_ty, rhs_ty) => todo!(
                "Implement non arithmetic binops: `{} {op} {}`",
                self.find(lhs_ty).to_string(self.db),
                self.find(rhs_ty).to_string(self.db)
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
                    let ty =
                        InferTy::Adt { def: TypeDefId::Builtin(lid), fields: Vec::new() };
                    if let Err(err) = self.unify(&res_ty.into(), &ty) {
                        return ConstraintSolveResult::Error(err);
                    }
                    ConstraintSolveResult::Solved
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
                        let ty = int_ty_with_witdh(self.db, max_width);
                        let infer_ty =
                            InferTy::Adt { def: ty.def(self.db), fields: Vec::new() };
                        if let Err(err) = self.unify(&res_ty.into(), &infer_ty) {
                            return ConstraintSolveResult::Error(err);
                        }
                        return ConstraintSolveResult::Solved;
                    }
                    todo!()
                }
            }
            op => {
                let printer = TypePrinter {
                    options: TypePrinterOptionSet::default()
                        .with(TypePrinterOption::PrintPath),
                };
                todo!(
                    "{} {op} {}",
                    printer.type_def_id_to_string(self.db, TypeDefId::Builtin(lid)),
                    printer.type_def_id_to_string(self.db, TypeDefId::Builtin(rid))
                )
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
        let FunctionLikeAst::Method(ast) =
            function_ast(self.db, method_call_infos.method_id.interned()).inner(self.db)
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

        if let Err(err) = method_call_infos.args.iter().zip(&ast.data.args).try_for_each(
            |(arg, ast_ty)| {
                let arg_ty = self.allocate_ast_type_expr(&ast_ty.ty.data, &method_ctx);
                self.unify(arg, &arg_ty)
            },
        ) {
            return ConstraintSolveResult::Error(err);
        }

        let ret_ty = self.allocate_type_ref(
            method_ctx
                .resolve(self.db, &ast.data.return_type.data)
                .unwrap_or(TypeRef::Error),
            &method_ctx,
        );

        if let Err(err) = self.unify(&method_call_infos.ret_var.into(), &ret_ty) {
            return ConstraintSolveResult::Error(err);
        }

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

        self.call_infos.insert(expr_id, call_infos);
        ConstraintSolveResult::Solved
    }

    fn solve_method_constraint(
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
            return ConstraintSolveResult::Error(UnificationError::Custom(format!(
                "Could not find an implementation for {} with arity {} on {}",
                method.display(self.db),
                args.len(),
                self.find(receiver).to_string(self.db)
            )));
        }

        if possible_blocks.len() > 1 {
            for possible in possible_blocks {
                let src = possible.0;
                println!("Here: {}", src.id(self.db).to_string(self.db));
            }
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
                UnificationError::StaticMethodCallOnReceiver(*id, method_id),
            );
        }

        if ast.data.args.len() != args.len() {
            return ConstraintSolveResult::Error(UnificationError::ArgCountMismatch(
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

    fn solve_implements_constraint(
        &mut self,
        _id: InferenceConstraintId,
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
        let mut result_fun = || {
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
            if competing_impls.is_empty() {
                return ConstraintSolveResult::Error(
                    UnificationError::NoImplemCandidateFor(
                        self.find(ty),
                        interface_id,
                        args.iter().cloned().collect(),
                    ),
                );
            }
            let competing_impls = self.get_working_impls(competing_impls);

            let impl_ = if competing_impls.len() == 1 {
                let (_, impl_) = competing_impls
                    .into_iter()
                    .next()
                    .expect("We know that competing impls has a single element");
                impl_
            } else if competing_impls.is_empty() {
                return ConstraintSolveResult::Error(
                    UnificationError::NoImplemCandidateFor(
                        self.find(ty),
                        interface_id,
                        args.iter().cloned().collect(),
                    ),
                );
            } else {
                return ConstraintSolveResult::Pending;
            };
            for constraint in impl_.constraints {
                self.emit_constraint(constraint.clone());
            }
            if let Err((inference_constraint, unification_error)) =
                self.solve_constraints()
            {
                return ConstraintSolveResult::Error(UnificationError::UnmetConstraint(
                    inference_constraint,
                    Box::new(unification_error),
                ));
            }

            ConstraintSolveResult::Solved
        };
        let result = result_fun();
        self.in_flight_impls.remove(&key);
        self.impl_depth -= 1;
        result
    }

    fn solve_fat_ptr_constraint(
        &mut self,
        fat_ptr_var: InferVar,
    ) -> ConstraintSolveResult {
        let fat_ptr_ty = self.find(&fat_ptr_var.into());
        let Some((_, ty)) = fat_ptr_ty.as_ref(self.db) else {
            return ConstraintSolveResult::Pending;
        };
        if !ty.is_adt() {
            return ConstraintSolveResult::Pending;
        }
        let Some(_) = ty.as_slice(self.db) else {
            return ConstraintSolveResult::Error(UnificationError::Custom(format!(
                "Expected a fat ptr type but got {}",
                fat_ptr_ty.to_string(self.db)
            )));
        };
        ConstraintSolveResult::Solved
    }

    fn solve_metadata_of_fat_ptr_constraint(
        &mut self,
        fat_ptr_var: InferVar,
        metadata_var: InferVar,
    ) -> ConstraintSolveResult {
        let fat_ptr_ty = self.find(&fat_ptr_var.into());

        // Only fat ptr type supported for now is ref to slices
        if fat_ptr_ty.as_ref_slice(self.db).is_none() {
            return ConstraintSolveResult::Error(UnificationError::Custom(format!(
                "Expected a fat ptr type but got {}",
                fat_ptr_ty.to_string(self.db)
            )));
        }

        if let Err(err) = self.unify(&metadata_var.into(), &self.usize_ty()) {
            return ConstraintSolveResult::Error(err);
        }

        ConstraintSolveResult::Solved
    }

    pub fn solve_constraints(
        &mut self,
    ) -> Result<(), (Arc<InferenceConstraint>, UnificationError)> {
        debug_assert!(
            !self.in_snapshot(),
            "solve_constraints must not run inside a snapshot"
        );
        loop {
            while let Some(id) = self.ready.pop_front() {
                if self.error_constraints.contains(&id) {
                    continue;
                }
                let constraint = self.all_constraints[&id].clone();
                match self.try_solve_constraint(&constraint) {
                    ConstraintSolveResult::Solved => {
                        self.solved_constraints.insert(id);
                    }
                    ConstraintSolveResult::Pending => {
                        self.register_listeners(&constraint);
                    }
                    ConstraintSolveResult::Error(e) => {
                        self.error_constraints.insert(constraint.id);
                        return Err((constraint, e));
                    }
                }
            }

            // For all pending constraints remaining, solve them using the
            // default behaviour Might want to do this one at a
            // time, in order to avoid non-determinism issues

            let pending = self
                .all_constraints
                .iter()
                .find(|(id, constraint)| {
                    !self.solved_constraints.contains(*id)
                        && (constraint.kind.has_default_behaviour())
                })
                .map(|(_, val)| val.clone());
            match pending {
                None => {
                    break;
                }
                Some(constraint) => {
                    let res = self.try_default_constraint(constraint.as_ref());
                    match res {
                        ConstraintSolveResult::Solved => {
                            self.solved_constraints.insert(constraint.id);
                        }
                        ConstraintSolveResult::Pending => {
                            panic!(
                                "Default constraint solving should never return pending"
                            )
                        }
                        ConstraintSolveResult::Error(e) => {
                            return Err((constraint, e));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn try_solve_constraint(
        &mut self,
        constraint: &InferenceConstraint,
    ) -> ConstraintSolveResult {
        if self.solved_constraints.contains(&constraint.id) {
            return ConstraintSolveResult::Solved;
        }
        match &constraint.kind {
            InferenceConstraintKind::Deref { var, target } => {
                self.solve_deref_constraint(*var, target)
            }
            InferenceConstraintKind::BindsLike { ty, inner, like } => {
                self.solve_binds_like_constraint(*ty, inner, *like)
            }
            InferenceConstraintKind::IndexedBy { elem_var, base_ty, index_ty } => {
                self.solve_indexed_by_constraint(*elem_var, base_ty, index_ty)
            }
            InferenceConstraintKind::Tuple { elem_var, tuple_ty, has_index } => {
                self.solve_tuple_constraint(*elem_var, tuple_ty, *has_index)
            }
            InferenceConstraintKind::StructField { elem_var, struct_ty, field } => {
                self.solve_struct_field_constraint(*elem_var, struct_ty, *field)
            }
            InferenceConstraintKind::Method(method) => {
                self.solve_method_constraint(method)
            }
            InferenceConstraintKind::Implements { ty, id, args } => {
                self.solve_implements_constraint(constraint.id, ty, *id, args)
            }
            InferenceConstraintKind::Unify { a, b } => self.solve_unify_constraint(a, b),
            InferenceConstraintKind::Binop { res_ty, lhs_ty, rhs_ty, op } => {
                self.solve_binop_constraint(*res_ty, lhs_ty, rhs_ty, *op)
            }
            InferenceConstraintKind::IntLike { res_ty } => {
                let t = self.find(&res_ty.into());
                match t {
                    InferTy::Var(_) => ConstraintSolveResult::Pending,
                    InferTy::Adt { def, .. } => {
                        if def.is_int_like(self.db).is_some() {
                            ConstraintSolveResult::Solved
                        } else {
                            todo!("Not an int like type: {}", t.to_string(self.db))
                        }
                    }
                    InferTy::Param(type_param_id) => {
                        todo!(
                            "Trying to constraint param `T{}` to int like type",
                            type_param_id.0
                        )
                    }
                }
            }
            InferenceConstraintKind::IsInner { inner, ref_ty } => {
                fn get_ref_inner(ctx: &mut InferenceCtx, ty: InferTy) -> Option<InferTy> {
                    match ty {
                        InferTy::Var(_) => None,
                        InferTy::Adt { def, fields } => {
                            if matches!(def.is_ptr_like(ctx.db), Some(PtrKind::Ref(_))) {
                                assert_eq!(fields.len(), 1);
                                get_ref_inner(ctx, fields.into_iter().next()?)
                            } else {
                                Some(InferTy::Adt { def, fields })
                            }
                        }
                        InferTy::Param(id) => Some(InferTy::Param(id)),
                    }
                }
                let ref_ty = self.find(ref_ty);
                let inner = self.find(inner);
                if ref_ty == inner {
                    // We're done
                    ConstraintSolveResult::Solved
                } else if let Some(ty) = get_ref_inner(self, ref_ty) {
                    if let Err(err) = self.unify(&inner, &ty) {
                        ConstraintSolveResult::Error(err)
                    } else {
                        ConstraintSolveResult::Solved
                    }
                } else {
                    ConstraintSolveResult::Pending
                }
            }
            InferenceConstraintKind::FatPtr { fat_ptr_var } => {
                self.solve_fat_ptr_constraint(*fat_ptr_var)
            }
            InferenceConstraintKind::MetadataOfFatPtr { fat_ptr_var, metadata_var } => {
                self.solve_metadata_of_fat_ptr_constraint(*fat_ptr_var, *metadata_var)
            }
        }
    }

    fn solve_unify_constraint(
        &mut self,
        a: &InferTy,
        b: &InferTy,
    ) -> ConstraintSolveResult {
        if let Err(err) = self.unify(a, b) {
            ConstraintSolveResult::Error(err)
        } else {
            ConstraintSolveResult::Solved
        }
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
        fields: ty
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
