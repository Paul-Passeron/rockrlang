use std::collections::VecDeque;

use crate::{
    common::symbols::Symbol,
    hir::{Mutability, impl_items},
    parse_tree::top_level::{self, AstImplItem},
    ril::{BuiltinTypeId, FunctionId, InterfaceId, PtrKind, ScopeOwnerId, TypeDefId},
    thir::{
        ExprId,
        inference::{InferenceConstraint, InferenceCtx, UnificationError, var::InferVar},
    },
};

use super::{InferTy, implems::PotentialBlockRes};

enum ConstraintSolveResult {
    Solved,
    Pending,
    Error(UnificationError),
}

impl<'db> InferenceCtx<'db> {
    fn solve_deref_constraint(&mut self, var: InferVar, target: InferTy) -> ConstraintSolveResult {
        if let Some(value) = self.table.probe_value(var) {
            match value {
                InferTy::Var(_) => ConstraintSolveResult::Pending,
                InferTy::Adt { def, fields } => {
                    if def.is_ptr_like(self.db).is_none() || fields.len() != 1 {
                        return ConstraintSolveResult::Error(UnificationError::ExpectedPtrLike(
                            def,
                        ));
                    }
                    if let Err(err) = self.unify(target, fields.into_iter().next().unwrap()) {
                        return ConstraintSolveResult::Error(err);
                    }
                    ConstraintSolveResult::Solved
                }
                InferTy::Param(id) => {
                    ConstraintSolveResult::Error(UnificationError::TemplateDereferencing(id))
                }
            }
        } else {
            ConstraintSolveResult::Pending
        }
    }

    fn solve_binds_like_constraint(
        &mut self,
        ty: InferVar,
        inner: InferTy,
        like: InferVar,
    ) -> ConstraintSolveResult {
        if let Some(InferTy::Adt { def, .. }) = self.table.probe_value(like) {
            let to_unify = if let Some(PtrKind::Ref(mutability)) = def.is_ptr_like(self.db) {
                InferTy::Adt {
                    def: TypeDefId::Builtin(match mutability {
                        Mutability::Const => BuiltinTypeId::ref_(self.db),
                        Mutability::Mutable => BuiltinTypeId::mut_ref(self.db),
                    }),
                    fields: Box::new([inner]),
                }
            } else {
                inner
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
        base_ty: InferTy,
        index_ty: InferTy,
    ) -> ConstraintSolveResult {
        let found = self.find(&base_ty);
        if let Some(elem_ty) = self.is_builtin_indexed_by_int(&found) {
            if let Err(err) = self
                .unify(InferTy::Var(elem_var), elem_ty)
                .and_then(|_| self.unify(index_ty, self.int_ty()))
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
        tuple_ty: InferTy,
        has_index: u32,
    ) -> ConstraintSolveResult {
        let found = self.find(&tuple_ty);
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
        struct_ty: InferTy,
        field: Symbol,
    ) -> ConstraintSolveResult {
        let found = self.find(&struct_ty);
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
        receiver: InferTy,
        id: ExprId,
        method: Symbol,
        args: &[InferTy],
        interface_hint: Option<InterfaceId>,
    ) -> ConstraintSolveResult {
        let mut possible_blocks = self.get_potential_blocks(&receiver);
        if let Some(id) = interface_hint {
            possible_blocks = possible_blocks
                .into_iter()
                .filter(|(src, _)| {
                    src.id(self.db)
                        .interface(self.db)
                        .is_some_and(|impl_interface_id| impl_interface_id.def(self.db) == id)
                })
                .filter(|(src, _)| {
                    let items = impl_items(self.db, src.id(self.db).interned());
                    for item in items {
                        match item {
                            AstImplItem::Fundef(def) => {
                                if def.data.name == method {
                                    return true;
                                }
                            }
                            _ => (),
                        }
                    }
                    false
                })
                .collect();
        }
        if possible_blocks.is_empty() {
            todo!()
        } else if possible_blocks.len() > 1 {
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
        if let Err(err) = self.solve_constraints() {
            todo!("{:?}", err)
        }

        let module = src.id(self.db).parent(self.db);

        let method_id = FunctionId::new(self.db, method, ScopeOwnerId::Impl(src.id(self.db)));

        let ast = impl_items(self.db, src.id(self.db).interned())
            .into_iter()
            .find_map(|item| match item {
                AstImplItem::Fundef(def) if def.data.name == method => Some(def),
                _ => None,
            })
            .unwrap();

        match ast.data.receiver {
            top_level::AstReceiver::None => {
                return ConstraintSolveResult::Error(UnificationError::StaticMethodCallOnReceiver(
                    id, method_id,
                ));
            }
            _ => (),
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

        let ast_method_templates = src
            .templates(self.db)
            .into_iter()
            .chain(ast.data.template_args.clone())
            .collect::<Box<[_]>>();

        if let Err(err) = args
            .iter()
            .zip(&ast.data.args)
            .try_for_each(|(arg, ast_ty)| {
                let arg_ty = self
                    .allocate_ast_type_expr(
                        &ast_ty.ty.data,
                        module,
                        &ast_method_templates,
                        &method_templates,
                    )
                    .unwrap();
                self.unify(arg.clone(), arg_ty)
            })
        {
            return ConstraintSolveResult::Error(err);
        }

        let ast_ret_ty = &ast.data.return_type;
        let ret_ty = self
            .allocate_ast_type_expr(
                &ast_ret_ty.data,
                module,
                &ast_method_templates,
                &method_templates,
            )
            .unwrap();

        if let Err(err) = self.unify(InferTy::Var(ret_var), ret_ty) {
            return ConstraintSolveResult::Error(err);
        }

        ConstraintSolveResult::Solved
    }

    fn solve_implements_constraint(
        &mut self,
        _ty: InferTy,
        _id: InterfaceId,
        _args: &[InferTy],
    ) -> ConstraintSolveResult {
        todo!()
    }

    fn solve_unify_constraint(&mut self, a: InferTy, b: InferTy) -> ConstraintSolveResult {
        if let Err(err) = self.unify(a, b) {
            ConstraintSolveResult::Error(err)
        } else {
            ConstraintSolveResult::Solved
        }
    }

    fn try_solve_constraint(&mut self, constraint: InferenceConstraint) -> ConstraintSolveResult {
        match constraint {
            InferenceConstraint::Deref { var, target } => self.solve_deref_constraint(var, target),
            InferenceConstraint::BindsLike { ty, inner, like } => {
                self.solve_binds_like_constraint(ty, inner, like)
            }
            InferenceConstraint::IndexedBy {
                elem_var,
                base_ty,
                index_ty,
            } => self.solve_indexed_by_constraint(elem_var, base_ty, index_ty),
            InferenceConstraint::Tuple {
                elem_var,
                tuple_ty,
                has_index,
            } => self.solve_tuple_constraint(elem_var, tuple_ty, has_index),
            InferenceConstraint::StructField {
                elem_var,
                struct_ty,
                field,
            } => self.solve_struct_field_constraint(elem_var, struct_ty, field),
            InferenceConstraint::Method {
                ret_var,
                ty,
                id,
                method,
                args,
                interface_hint,
            } => self.solve_method_constraint(ret_var, ty, id, method, &args, interface_hint),
            InferenceConstraint::Implements { ty, id, args } => {
                self.solve_implements_constraint(ty, id, &args)
            }
            InferenceConstraint::Unify { a, b } => self.solve_unify_constraint(a, b),
        }
    }

    fn try_solve_constraints(
        &mut self,
    ) -> Result<(), Box<(InferenceConstraint, UnificationError)>> {
        let mut len = 0;
        let mut worklist = VecDeque::from(std::mem::take(&mut self.constraints));

        while len != worklist.len()
            && let Some(constraint) = worklist.pop_front()
        {
            match self.try_solve_constraint(constraint.clone()) {
                // The constraint has been solved, no need to push it back in the worklist
                ConstraintSolveResult::Solved => (),

                // The constraint could not be solved but there were no errors, we push it back onto
                // the worklist
                ConstraintSolveResult::Pending => worklist.push_back(constraint),

                // An error was encountered, we return the constraint which caused the error
                ConstraintSolveResult::Error(error) => return Err(Box::new((constraint, error))),
            }
            // Solving the constraints might have generated more constraints so we insert them on the worklist
            worklist.extend_front(std::mem::take(&mut self.constraints));
            len = worklist.len();
        }
        self.constraints.extend(worklist);
        Ok(())
    }

    /// Fix-point iteration on the constraints worklist.
    pub fn solve_constraints(
        &mut self,
    ) -> Result<(), Box<(InferenceConstraint, UnificationError)>> {
        let constraints = self.constraints.clone();
        if let Err(err) = self.try_solve_constraints() {
            // roll-back constraints that were eaten during their resolution
            self.constraints = constraints;
            return Err(err);
        }
        Ok(())
    }

    pub fn emit_constraint(&mut self, constraint: InferenceConstraint) {
        self.constraints.push(constraint);
    }

    pub fn emit_deref_constraint(&mut self, pointee: InferTy) -> InferVar {
        let ptr_var = self.fresh_var();
        self.constraints.push(InferenceConstraint::Deref {
            var: ptr_var,
            target: pointee,
        });
        ptr_var
    }

    pub fn emit_indexed_by_constraint(&mut self, base_ty: InferTy, index_ty: InferTy) -> InferVar {
        let elem_var = self.fresh_var();
        self.constraints.push(InferenceConstraint::IndexedBy {
            elem_var,
            base_ty,
            index_ty,
        });
        elem_var
    }

    pub fn emit_tuple_constraint(&mut self, tuple_ty: InferTy, has_index: u32) -> InferVar {
        let elem_var = self.fresh_var();
        self.constraints.push(InferenceConstraint::Tuple {
            elem_var,
            tuple_ty,
            has_index,
        });
        elem_var
    }

    pub fn emit_struct_field_constraint(&mut self, struct_ty: InferTy, field: Symbol) -> InferVar {
        let elem_var = self.fresh_var();
        self.constraints.push(InferenceConstraint::StructField {
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
    ) -> InferVar {
        let ret_var = self.fresh_var();
        self.constraints.push(InferenceConstraint::Method {
            ret_var,
            ty,
            id,
            method,
            args,
            interface_hint,
        });
        ret_var
    }

    pub fn emit_implements_constraint(
        &mut self,
        ty: InferTy,
        id: InterfaceId,
        args: Box<[InferTy]>,
    ) {
        self.constraints
            .push(InferenceConstraint::Implements { ty, id, args });
    }
}
