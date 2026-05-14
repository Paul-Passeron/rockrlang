use std::{
    collections::{HashMap, HashSet, VecDeque, hash_map::Entry},
    iter::once,
    sync::Arc,
};

use itertools::Itertools;

use crate::{
    common::symbols::Symbol,
    hir::{Mutability, impl_items},
    parse_tree::top_level::AstImplItem,
    ril::{
        BuiltinTypeId, FunctionId, ImplSource, InterfaceId, PtrKind, ScopeOwnerId, TypeDefId,
        display::RilDisplay,
    },
    thir::{
        ExprId, InferCallInfos,
        inference::{
            InferenceConstraint, InferenceConstraintId, InferenceConstraintKind, InferenceCtx,
            InterfaceImplem, UnificationError, implicit::ImplicitContext, var::InferVar,
        },
    },
};

use super::{InferTy, implems::PotentialBlockRes};

enum ConstraintSolveResult {
    Solved,
    Pending,
    Error(UnificationError),
}

impl<'db> InferenceCtx<'db> {
    fn solve_deref_constraint(&mut self, var: InferVar, target: &InferTy) -> ConstraintSolveResult {
        if let Some(value) = self.table.probe_value(var) {
            match value {
                InferTy::Var(_) => ConstraintSolveResult::Pending,
                InferTy::Adt { def, fields } => {
                    if def.is_ptr_like(self.db).is_none() || fields.len() != 1 {
                        return ConstraintSolveResult::Error(UnificationError::ExpectedPtrLike(
                            def,
                        ));
                    }
                    if let Err(err) = self.unify(target.clone(), fields.into_iter().next().unwrap())
                    {
                        return ConstraintSolveResult::Error(err);
                    }
                    ConstraintSolveResult::Solved
                }
                InferTy::Param(id) => {
                    ConstraintSolveResult::Error(UnificationError::TemplateDereferencing(id))
                }
                InferTy::Zelf => todo!(),
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
            let to_unify = if let Some(PtrKind::Ref(mutability)) = def.is_ptr_like(self.db) {
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
            if let Err(err) = self.unify(InferTy::Var(ty), to_unify) {
                ConstraintSolveResult::Error(err)
            } else {
                ConstraintSolveResult::Solved
            }
        } else {
            ConstraintSolveResult::Pending
        }
    }

    fn is_builtin_indexed_by_int(&self, ty: &InferTy) -> Option<InferTy> {
        self.is_slice(ty)
            .or_else(|| self.is_ref_to_slice(ty))
            .or_else(|| self.is_ptr(ty))
    }

    fn solve_indexed_by_constraint(
        &mut self,
        elem_var: InferVar,
        base_ty: &InferTy,
        index_ty: &InferTy,
    ) -> ConstraintSolveResult {
        let found = self.find(base_ty);
        if let Some(elem_ty) = self.is_builtin_indexed_by_int(&found) {
            if let Err(err) = self
                .unify(InferTy::Var(elem_var), elem_ty)
                .and_then(|_| self.unify(index_ty.clone(), self.int_ty()))
            {
                ConstraintSolveResult::Error(err)
            } else {
                ConstraintSolveResult::Solved
            }
        } else if found.is_adt().is_some() {
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
                self.unify(InferTy::Var(elem_var), fields[has_index as usize].clone())
            {
                ConstraintSolveResult::Error(err)
            } else {
                ConstraintSolveResult::Solved
            }
        } else if let Some((def, _)) = found.is_adt() {
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
                if let Err(err) = self.unify(InferTy::Var(elem_var), ty) {
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
        } else if let Some((def, _)) = found.is_adt() {
            ConstraintSolveResult::Error(UnificationError::ExpectedStructWithField { def, field })
        } else {
            ConstraintSolveResult::Pending
        }
    }

    fn solve_method_constraint(
        &mut self,
        ret_var: InferVar,
        receiver: &InferTy,
        id: ExprId,
        method: Symbol,
        args: &[InferTy],
        interface_hint: Option<InterfaceId>,
        is_static: bool,
    ) -> ConstraintSolveResult {
        let mut possible_blocks = self
            .get_potential_blocks(receiver)
            .into_iter()
            .unique_by(|(src, _)| src.id(self.db))
            .filter(|(src, _)| {
                let items = impl_items(self.db, src.id(self.db).interned());
                for item in items {
                    if let AstImplItem::Fundef(def) = item
                        && def.data.name == method
                        && def.data.receiver.is_static() == is_static
                        && def.data.args.len() == args.len()
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
                    src.id(self.db)
                        .interface(self.db)
                        .is_some_and(|impl_interface_id| impl_interface_id.def(self.db) == id)
                })
                .collect();
        }
        let possible_blocks = self.get_working_impls(possible_blocks);

        if possible_blocks.is_empty() {
            todo!()
        } else if possible_blocks.len() > 1 {
            for possible in possible_blocks {
                let src = possible.0;
                println!("Here: {}", src.id(self.db).display(self.db))
            }
            todo!()
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
        if let Err((inference_constraint, unification_error)) = self.solve_constraints() {
            return ConstraintSolveResult::Error(UnificationError::UnmetConstraint(
                inference_constraint,
                Box::new(unification_error),
            ));
        }

        let method_id = FunctionId::new(self.db, method, ScopeOwnerId::Impl(src.id(self.db)));

        let ast = impl_items(self.db, src.id(self.db).interned())
            .into_iter()
            .find_map(|item| match item {
                AstImplItem::Fundef(def) if def.data.name == method => Some(def),
                _ => None,
            })
            .unwrap();

        if ast.data.receiver.is_static() != is_static {
            return ConstraintSolveResult::Error(UnificationError::StaticMethodCallOnReceiver(
                id, method_id,
            ));
        }

        if ast.data.args.len() != args.len() {
            return ConstraintSolveResult::Error(UnificationError::ArgCountMismatch(
                method_id,
                args.len(),
            ));
        }

        let method_templates = templates
            .iter()
            .map(|var| InferTy::Var(*var))
            .chain(ast.data.template_args.iter().map(|ast_template| {
                if !ast_template.constraints.is_empty() {
                    todo!()
                }
                InferTy::Var(self.fresh_var())
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

        if let Err(err) = self.unify(InferTy::Var(ret_var), ret_ty) {
            return ConstraintSolveResult::Error(err);
        }

        let call_infos = InferCallInfos {
            expr_id: id,
            callee: method_id,
            substitution: method_templates,
            variadic: false,
        };

        self.call_infos.insert(id, call_infos);

        ConstraintSolveResult::Solved
    }

    fn solve_implements_constraint(
        &mut self,
        _id: InferenceConstraintId,
        ty: &InferTy,
        interface_id: InterfaceId,
        args: &[InferTy],
    ) -> ConstraintSolveResult {
        let ty = &self.find(ty);
        println!(
            "Does {} implement {} ?",
            ty.display(self.db),
            interface_id.display(self.db)
        );
        if self.has_implementation(interface_id, ty, args) {
            return ConstraintSolveResult::Solved;
        }

        self.add_implementation(interface_id, ty.clone(), args);
        // TODO: remove implementation when erroring out, maybe

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
            return ConstraintSolveResult::Error(UnificationError::NoImplemCandidateFor(
                self.find(ty),
                interface_id,
                args.iter().cloned().collect(),
            ));
        }
        let competing_impls = self.get_working_impls(competing_impls);
        let impl_ = if competing_impls.len() == 1 {
            let (_, impl_) = competing_impls.into_iter().next().unwrap();
            impl_
        } else {
            todo!("Ambiguous implem")
        };
        for constraint in impl_.constraints {
            self.emit_constraint(constraint.clone());
        }
        if let Err((inference_constraint, unification_error)) = self.solve_constraints() {
            return ConstraintSolveResult::Error(UnificationError::UnmetConstraint(
                inference_constraint,
                Box::new(unification_error),
            ));
        }

        ConstraintSolveResult::Solved
    }

    fn get_working_impls(
        &mut self,
        competing_impls: impl IntoIterator<Item = (ImplSource<'db>, PotentialBlockRes)>,
    ) -> HashMap<ImplSource<'db>, PotentialBlockRes> {
        let competing_impls = competing_impls.into_iter().collect::<Box<[_]>>();
        if competing_impls.len() > 1 {
            let competing_impls = competing_impls.into_iter().collect::<Box<[_]>>();

            let mut possibles = HashSet::new();
            for (i, (_, impl_)) in competing_impls.iter().enumerate() {
                let mut this = self.clone();
                for constraint in &impl_.constraints {
                    this.emit_constraint(constraint.clone());
                }
                if this.solve_constraints() == Ok(()) {
                    possibles.insert(i);
                }
            }
            competing_impls
                .into_iter()
                .enumerate()
                .filter(|(i, _)| possibles.contains(i))
                .map(|(_, elem)| elem)
                .collect()
        } else {
            competing_impls.into_iter().collect()
        }
    }

    fn has_implementation(&mut self, id: InterfaceId, ty: &InferTy, templates: &[InferTy]) -> bool {
        let ty = self.find(ty);
        let templates = templates.iter().map(|t| self.find(t)).collect::<Box<[_]>>();
        if !self.implements.contains_key(&id) {
            return false;
        }
        for implem in self
            .implements
            .get(&id)
            .unwrap()
            .iter()
            .cloned()
            .collect::<Box<[_]>>()
            .iter()
        {
            let implem_ty = self.find(&implem.ty);
            let implem_templates = implem
                .templates
                .iter()
                .map(|t| self.find(t))
                .collect::<Box<[_]>>();
            if ty == implem_ty && templates == implem_templates {
                return true;
            }
        }
        false
    }

    fn add_implementation(&mut self, id: InterfaceId, ty: InferTy, templates: &[InferTy]) {
        let ty = self.find(&ty);
        let templates = templates.iter().map(|t| self.find(t)).collect::<Box<[_]>>();
        if let Entry::Vacant(e) = self.implements.entry(id) {
            e.insert(HashSet::from_iter(once(InterfaceImplem {
                interface: id,
                ty,
                templates,
            })));
        } else {
            for implem in self
                .implements
                .get(&id)
                .unwrap()
                .iter()
                .cloned()
                .collect::<Box<[_]>>()
                .iter()
            {
                let implem_ty = self.find(&implem.ty);
                let implem_templates = implem
                    .templates
                    .iter()
                    .map(|t| self.find(t))
                    .collect::<Box<[_]>>();
                if ty == implem_ty && templates == implem_templates {
                    return;
                }
            }
            self.implements
                .get_mut(&id)
                .unwrap()
                .insert(InterfaceImplem {
                    interface: id,
                    ty,
                    templates,
                });
        }
    }

    fn solve_unify_constraint(&mut self, a: &InferTy, b: &InferTy) -> ConstraintSolveResult {
        if let Err(err) = self.unify(a.clone(), b.clone()) {
            ConstraintSolveResult::Error(err)
        } else {
            ConstraintSolveResult::Solved
        }
    }

    fn try_solve_constraint(&mut self, constraint: &InferenceConstraint) -> ConstraintSolveResult {
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
            InferenceConstraintKind::Method {
                ret_var,
                ty,
                id,
                method,
                args,
                interface_hint,
                is_static,
            } => self.solve_method_constraint(
                *ret_var,
                ty,
                *id,
                *method,
                args,
                *interface_hint,
                *is_static,
            ),
            InferenceConstraintKind::Implements { ty, id, args } => {
                self.solve_implements_constraint(constraint.id, ty, *id, args)
            }
            InferenceConstraintKind::Unify { a, b } => self.solve_unify_constraint(a, b),
        }
    }

    fn try_solve_constraints(
        &mut self,
    ) -> Result<(), (Arc<InferenceConstraint>, UnificationError)> {
        let mut len = 0;
        let mut worklist = VecDeque::from(std::mem::take(&mut self.current_constraints));

        while len != worklist.len()
            && let Some(constraint) = worklist.pop_front()
        {
            match self.try_solve_constraint(constraint.as_ref()) {
                // The constraint has been solved, no need to push it back in the worklist
                ConstraintSolveResult::Solved => {
                    self.solved_constraints.insert(constraint.id);
                }

                // The constraint could not be solved but there were no errors, we push it back onto
                // the worklist
                ConstraintSolveResult::Pending => worklist.push_back(constraint),

                // An error was encountered, we return the constraint which caused the error
                ConstraintSolveResult::Error(error) => return Err((constraint, error)),
            }
            // Solving the constraints might have generated more constraints so we insert them on the worklist
            worklist.extend_front(std::mem::take(&mut self.current_constraints));
            len = worklist.len();
        }
        self.current_constraints.extend(worklist);
        Ok(())
    }

    /// Fix-point iteration on the constraints worklist.
    pub fn solve_constraints(
        &mut self,
    ) -> Result<(), (Arc<InferenceConstraint>, UnificationError)> {
        let constraints = self.current_constraints.clone();
        if let Err(err) = self.try_solve_constraints() {
            // roll-back constraints that were eaten during their resolution
            self.current_constraints = constraints;
            return Err(err);
        }
        Ok(())
    }

    fn fresh_constraint(&mut self, constraint: InferenceConstraintKind) -> InferenceConstraint {
        let res = InferenceConstraint {
            id: InferenceConstraintId(self.next_constraint_id),
            kind: constraint,
        };
        self.next_constraint_id += 1;
        res
    }

    pub fn emit_constraint(&mut self, constraint: InferenceConstraintKind) {
        let constraint = Arc::new(self.fresh_constraint(constraint));
        self.current_constraints.push(constraint.clone());
        self.all_constraints.insert(constraint.id, constraint);
    }

    pub fn emit_deref_constraint(&mut self, pointee: InferTy) -> InferVar {
        let ptr_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::Deref {
            var: ptr_var,
            target: pointee,
        });
        ptr_var
    }

    pub fn emit_indexed_by_constraint(&mut self, base_ty: InferTy, index_ty: InferTy) -> InferVar {
        let elem_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::IndexedBy {
            elem_var,
            base_ty,
            index_ty,
        });
        elem_var
    }

    pub fn emit_tuple_constraint(&mut self, tuple_ty: InferTy, has_index: u32) -> InferVar {
        let elem_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::Tuple {
            elem_var,
            tuple_ty,
            has_index,
        });
        elem_var
    }

    pub fn emit_struct_field_constraint(&mut self, struct_ty: InferTy, field: Symbol) -> InferVar {
        let elem_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::StructField {
            elem_var,
            struct_ty,
            field,
        });
        elem_var
    }

    pub fn emit_method_constraint(
        &mut self,
        id: ExprId, // Used to populate the call infos
        ty: InferTy,
        method: Symbol,
        args: Box<[InferTy]>,
        interface_hint: Option<InterfaceId>,
        is_static: bool,
    ) -> InferVar {
        let ret_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::Method {
            ret_var,
            ty,
            id,
            method,
            args,
            interface_hint,
            is_static,
        });
        ret_var
    }

    #[allow(dead_code)]
    pub fn emit_implements_constraint(
        &mut self,
        ty: InferTy,
        id: InterfaceId,
        args: Box<[InferTy]>,
    ) {
        self.emit_constraint(InferenceConstraintKind::Implements { ty, id, args });
    }
}
