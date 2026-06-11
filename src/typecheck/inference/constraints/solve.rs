use std::{collections::HashMap, sync::Arc};

use itertools::Itertools;

use crate::{
    common::symbols::Symbol,
    hir::{Mutability, impl_items},
    parse_tree::{expr::BinaryOperator, top_level::AstImplItem},
    printer::type_printer::{TypePrinter, TypePrinterOption, TypePrinterOptionSet},
    ril::{
        BuiltinTypeId, FunctionId, ImplSource, InterfaceId, PtrKind, ScopeOwnerId,
        TypeDefId,
    },
    typecheck::{
        CallKind, InferCallInfos,
        inference::{
            InferTy, InferenceCtx, UnificationError,
            constraints::{
                ConstraintSolveResult, InferenceConstraint, InferenceConstraintId,
                InferenceConstraintKind, MAX_IMPL_DEPTH, MethodConstraint,
            },
            implems::PotentialBlockRes,
            implicit::ImplicitContext,
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
                    if let Err(err) =
                        self.unify(target.clone(), fields.into_iter().next().unwrap())
                    {
                        return ConstraintSolveResult::Error(err);
                    }
                    ConstraintSolveResult::Solved
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
                    InferTy::Adt {
                        def: TypeDefId::Builtin(match mutability {
                            Mutability::Const => BuiltinTypeId::ref_(self.db),
                            Mutability::Mutable => BuiltinTypeId::mut_ref(self.db),
                        }),
                        fields: Box::new([inner.clone()]),
                    }
                } else {
                    inner.clone()
                };
            if let Err(err) = self.unify(ty.into(), to_unify) {
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
            if let Err(err) = self.unify(elem_var.into(), elem_ty) {
                return ConstraintSolveResult::Error(err);
            }
            let idx = self.emit_intlike_constraint();
            if let Err(err) = self.unify(index_ty.clone(), idx.into()) {
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
                self.unify(elem_var.into(), fields[has_index as usize].clone())
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
        if let Some((struct_id, mut fields)) = self.is_struct(&found) {
            if let Some(ty) = fields.remove(&field) {
                if let Err(err) = self.unify(elem_var.into(), ty) {
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
                found.to_string(self.db)
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
                if let Err(err) = self.unify(lhs_ty, rhs_ty) {
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
                if let Err(err) = self.unify(v.into(), rhs_ty.clone()) {
                    return ConstraintSolveResult::Error(err);
                }
                self.solve_int_binop(res_ty, int_like, int_like, op)
            }
            (lhs_ty, rhs_ty) => todo!(
                "Implement non arithmetic binops: `{} {op} {}`",
                lhs_ty.to_string(self.db),
                rhs_ty.to_string(self.db)
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
                if let Err(err) = self.unify(res_ty.into(), self.bool_ty()) {
                    return ConstraintSolveResult::Error(err);
                }
                ConstraintSolveResult::Solved
            }
            BinaryOperator::Plus => {
                if lid == rid {
                    let ty = InferTy::Adt {
                        def: TypeDefId::Builtin(lid),
                        fields: Box::new([]),
                    };
                    if let Err(err) = self.unify(res_ty.into(), ty) {
                        return ConstraintSolveResult::Error(err);
                    }
                    ConstraintSolveResult::Solved
                } else {
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

        if let Some(result) = self.try_resolve_via_known_impl(
            *ret_var,
            receiver,
            *method,
            args,
            *interface_hint,
            *is_static,
        ) {
            return result;
        }

        let possible_blocks = self.compute_possible_blocks(
            receiver,
            *method,
            *interface_hint,
            args.len(),
            *is_static,
        );

        if possible_blocks.is_empty() {
            return ConstraintSolveResult::Error(UnificationError::Custom(format!(
                "Could not find an implementation for {} with arity {} on {}",
                method.display(self.db),
                args.len(),
                self.find(receiver).to_string(self.db)
            )));
        } else if possible_blocks.len() > 1 {
            for possible in possible_blocks {
                let src = possible.0;
                println!("Here: {}", src.id(self.db).to_string(self.db))
            }
            return ConstraintSolveResult::Pending;
        }
        let (
            src,
            PotentialBlockRes {
                templates,
                constraints,
                ..
            },
        ) = possible_blocks.into_iter().next().unwrap();
        constraints
            .into_iter()
            .for_each(|constraint| self.emit_constraint(constraint));

        let method_id =
            FunctionId::new(self.db, *method, ScopeOwnerId::Impl(src.id(self.db)));

        let ast = impl_items(self.db, src.id(self.db).interned())
            .into_iter()
            .find_map(|item| match item {
                AstImplItem::Fundef(def) if def.data.name.data == *method => Some(def),
                _ => None,
            })
            .unwrap();

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

        let method_templates = templates
            .iter()
            .map(|var| var.into())
            .chain(ast.data.template_args.iter().map(|ast_template| {
                if !ast_template.constraints.is_empty() {
                    todo!()
                }
                self.fresh_var().into()
            }))
            .collect::<Box<[_]>>();

        let method_ctx = ImplicitContext::from_function(
            self.db,
            method_id,
            method_templates.iter().cloned().collect(),
            Some(receiver.clone()),
        )
        .unwrap();

        if let Err(err) = args
            .iter()
            .zip(&ast.data.args)
            .try_for_each(|(arg, ast_ty)| {
                let arg_ty = self
                    .allocate_ast_type_expr(&ast_ty.ty.data, &method_ctx)
                    .unwrap();
                self.unify(arg.clone(), arg_ty)
            })
        {
            return ConstraintSolveResult::Error(err);
        }

        let ast_ret_ty = &ast.data.return_type;
        let ret_ty = self
            .allocate_ast_type_expr(&ast_ret_ty.data, &method_ctx)
            .unwrap();

        if let Err(err) = self.unify(ret_var.into(), ret_ty.clone()) {
            return ConstraintSolveResult::Error(err);
        }

        let call_infos = InferCallInfos {
            expr_id: *id,
            callee: method_id,
            substitution: method_templates,
            call_kind: CallKind::Method {
                receiver_deref_depth: 0,
            },
        };

        self.call_infos.insert(*id, call_infos);
        ConstraintSolveResult::Solved
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
            self.add_implementation(interface_id, ty.clone(), args);

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
                let (_, impl_) = competing_impls.into_iter().next().unwrap();
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

        if let Err(err) = self.unify(metadata_var.into(), self.usize_ty()) {
            return ConstraintSolveResult::Error(err);
        }

        ConstraintSolveResult::Solved
    }

    pub fn solve_constraints(
        &mut self,
    ) -> Result<(), (Arc<InferenceConstraint>, UnificationError)> {
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

            // For all pending constraints remaining, solve them using the default
            // behaviour Might want to do this one at a time, in order to
            // avoid non-determinism issues

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
            InferenceConstraintKind::IndexedBy {
                elem_var,
                base_ty,
                index_ty,
            } => self.solve_indexed_by_constraint(*elem_var, base_ty, index_ty),
            InferenceConstraintKind::Tuple {
                elem_var,
                tuple_ty,
                has_index,
            } => self.solve_tuple_constraint(*elem_var, tuple_ty, *has_index),
            InferenceConstraintKind::StructField {
                elem_var,
                struct_ty,
                field,
            } => self.solve_struct_field_constraint(*elem_var, struct_ty, *field),
            InferenceConstraintKind::Method(method) => {
                self.solve_method_constraint(method)
            }
            InferenceConstraintKind::Implements { ty, id, args } => {
                self.solve_implements_constraint(constraint.id, ty, *id, args)
            }
            InferenceConstraintKind::Unify { a, b } => self.solve_unify_constraint(a, b),
            InferenceConstraintKind::Binop {
                res_ty,
                lhs_ty,
                rhs_ty,
                op,
            } => self.solve_binop_constraint(*res_ty, lhs_ty, rhs_ty, *op),
            InferenceConstraintKind::IntLike { res_ty } => {
                let t = self.find(&res_ty.into());
                match t {
                    InferTy::Var(_) => ConstraintSolveResult::Pending,
                    InferTy::Adt { def, .. } => {
                        if def.is_int_like(self.db).is_some() {
                            ConstraintSolveResult::Solved
                        } else {
                            todo!("Not an int like type")
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
                                get_ref_inner(ctx, fields.into_iter().next().unwrap())
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
                    if let Err(err) = self.unify(inner, ty) {
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
            InferenceConstraintKind::MetadataOfFatPtr {
                fat_ptr_var,
                metadata_var,
            } => self.solve_metadata_of_fat_ptr_constraint(*fat_ptr_var, *metadata_var),
        }
    }

    fn solve_unify_constraint(
        &mut self,
        a: &InferTy,
        b: &InferTy,
    ) -> ConstraintSolveResult {
        if let Err(err) = self.unify(a.clone(), b.clone()) {
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
            possible_blocks =
                possible_blocks
                    .into_iter()
                    .filter(|(src, _)| {
                        src.id(self.db).interface(self.db).is_some_and(
                            |impl_interface_id| impl_interface_id.def(self.db) == id,
                        )
                    })
                    .collect();
        }
        self.get_working_impls(possible_blocks)
    }
}
