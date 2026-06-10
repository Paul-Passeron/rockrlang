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
    collections::{HashMap, HashSet, btree_map::Entry},
    fmt,
    iter::once,
    sync::Arc,
};

use itertools::Itertools;

use crate::{
    Db,
    common::symbols::Symbol,
    hir::{Mutability, impl_items, interface_items},
    parse_tree::{
        expr::BinaryOperator,
        top_level::{AstImplItem, AstInterfaceItem, AstMethodsig},
    },
    printer::type_printer::{TypePrinter, TypePrinterOption, TypePrinterOptionSet},
    ril::{
        BuiltinTypeId, FunctionId, ImplSource, InterfaceId, InterfaceRef, PtrKind,
        ScopeOwnerId, TypeDefId, TypeId, TypeRef, display::Display,
    },
    typecheck::{
        ExprId, InferCallInfos,
        inference::{
            InferenceCtx, InterfaceImplem, UnificationError, implicit::ImplicitContext,
            var::InferVar,
        },
    },
};

use super::{InferTy, implems::PotentialBlockRes};

const MAX_IMPL_DEPTH: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InferenceConstraintId(usize);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InferenceConstraint {
    pub id: InferenceConstraintId,
    pub kind: InferenceConstraintKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InferenceConstraintKind {
    Deref {
        var: InferVar,
        target: InferTy,
    },
    BindsLike {
        ty: InferVar,
        inner: InferTy,
        like: InferVar,
    },
    IndexedBy {
        elem_var: InferVar,
        base_ty: InferTy,
        index_ty: InferTy,
    },
    Tuple {
        elem_var: InferVar,
        tuple_ty: InferTy,
        has_index: u32,
    },
    StructField {
        elem_var: InferVar,
        struct_ty: InferTy,
        field: Symbol,
    },
    Method(MethodConstraint),
    Implements {
        ty: InferTy,
        id: InterfaceId,
        args: Box<[InferTy]>,
    },
    Unify {
        a: InferTy,
        b: InferTy,
    },
    Binop {
        res_ty: InferVar,
        lhs_ty: InferTy,
        rhs_ty: InferTy,
        op: BinaryOperator,
    },
    IntLike {
        res_ty: InferVar,
    },
    IsInner {
        inner: InferTy,
        ref_ty: InferTy,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MethodConstraint {
    ret_var: InferVar,
    ty: InferTy,
    id: ExprId,
    method: Symbol,
    args: Box<[InferTy]>,
    interface_hint: Option<InterfaceId>,
    is_static: bool,
}

impl InferenceConstraintKind {
    pub fn has_default_behaviour(&self) -> bool {
        matches!(
            self,
            InferenceConstraintKind::Deref { .. }
                | InferenceConstraintKind::IntLike { .. }
        )
    }
}

#[derive(Debug)]
enum ConstraintSolveResult {
    Solved,
    Pending,
    Error(UnificationError),
}

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

    fn is_builtin_indexed_by_int(&self, ty: &InferTy) -> Option<InferTy> {
        if self.is_slice(ty).is_some() {
            todo!()
        }
        self.is_ref_to_slice(ty).or_else(|| self.is_ptr(ty))
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
                self.unify(elem_var.into(), fields[has_index as usize].clone())
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
        } else if let Some((def, _)) = found.is_adt() {
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

    fn infer_to_ref(&mut self, ty: &InferTy) -> TypeRef {
        let ty = self.find(ty);
        match ty {
            InferTy::Var(_) => TypeRef::Error,
            InferTy::Adt { def, fields } => {
                let args = fields
                    .into_iter()
                    .map(|f| self.infer_to_ref(&f))
                    .collect::<Vec<_>>();
                for arg in &args {
                    if matches!(arg, TypeRef::Error) {
                        return TypeRef::Error;
                    }
                }
                TypeRef::Concrete(TypeId::new(self.db, def, args))
            }
            InferTy::Param(type_param_id) => TypeRef::Param(type_param_id),
        }
    }

    fn try_resolve_via_known_impl(
        &mut self,
        ret_var: InferVar,
        receiver: &InferTy,
        method: Symbol,
        args: &[InferTy],
        interface_hint: Option<InterfaceId>,
        is_static: bool,
    ) -> Option<ConstraintSolveResult> {
        if let Some((iface_id, implem, sig)) = self
            .known_impls_providing(
                receiver,
                method,
                args.len(),
                interface_hint,
                is_static,
            )
            .into_iter()
            .next()
        {
            return Some(self.apply_interface_method(
                iface_id,
                implem,
                sig.as_ref(),
                ret_var,
                args,
                is_static,
            ));
        }
        None
    }

    fn apply_interface_method(
        &mut self,
        iface_id: InterfaceId,
        implem: InterfaceImplem,
        sig: &AstMethodsig,
        ret_var: InferVar,
        args: &[InferTy],
        is_static: bool,
    ) -> ConstraintSolveResult {
        assert!(sig.data.receiver.is_static() == is_static);
        let ref_args = implem
            .templates
            .iter()
            .map(|a| self.infer_to_ref(a))
            .collect::<Vec<_>>();
        if ref_args.iter().any(|a| matches!(a, TypeRef::Error)) {
            return ConstraintSolveResult::Pending;
        }

        let iface_ref = InterfaceRef::new(self.db, iface_id, ref_args);
        let templates = implem
            .templates
            .iter()
            .cloned()
            .chain(args.iter().cloned())
            .collect::<Arc<[_]>>();
        let ctx = ImplicitContext::new(
            self.db,
            ScopeOwnerId::Interface(iface_ref),
            Arc::new([]),
            templates,
            Some(self.find(&implem.ty)),
        )
        .unwrap();

        let ret_ty = self
            .allocate_ast_type_expr(&sig.data.return_type.data, &ctx)
            .unwrap();
        if let Err(e) = self.unify(ret_var.into(), ret_ty) {
            return ConstraintSolveResult::Error(e);
        }

        for (ast_arg, call_arg) in sig.data.args.iter().zip(args) {
            let expected = self.allocate_ast_type_expr(&ast_arg.ty.data, &ctx).unwrap();
            if let Err(e) = self.unify(expected, call_arg.clone()) {
                return ConstraintSolveResult::Error(e);
            }
        }

        ConstraintSolveResult::Solved
    }

    fn known_impls_providing(
        &mut self,
        receiver: &InferTy,
        method: Symbol,
        arity: usize,
        hint: Option<InterfaceId>,
        is_static: bool,
    ) -> Vec<(InterfaceId, InterfaceImplem, Arc<AstMethodsig>)> {
        let mut out = vec![];
        let receiver = self.find(receiver);
        let implements = self.implements.clone(); // same clone as before, fix later
        'outer: for (iface_id, implems) in implements {
            if hint.is_some_and(|h| h != iface_id) {
                continue;
            }
            for implem in implems {
                if self.find(&implem.ty) != receiver {
                    continue 'outer;
                }
                for item in interface_items(self.db, iface_id.interned()).iter() {
                    if let AstInterfaceItem::Sig(sig) = item
                        && sig.data.name.data == method
                        && sig.data.args.len() == arity
                        && sig.data.receiver.is_static() == is_static
                    {
                        out.push((iface_id, implem.clone(), sig.clone()));
                    }
                }
            }
        }
        out
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

        let mut possible_blocks = self
            .get_potential_blocks(receiver)
            .into_iter()
            .unique_by(|(src, _)| src.id(self.db))
            .filter(|(src, _)| {
                let items = impl_items(self.db, src.id(self.db).interned());
                for item in items {
                    if let AstImplItem::Fundef(def) = item
                        && def.data.name.data == *method
                        && def.data.receiver.is_static() == *is_static
                        && def.data.args.len() == args.len()
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
                            |impl_interface_id| impl_interface_id.def(self.db) == *id,
                        )
                    })
                    .collect();
        }
        let possible_blocks = self.get_working_impls(possible_blocks);

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
            variadic: false,
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

    fn get_working_impls(
        &mut self,
        competing_impls: impl IntoIterator<Item = (ImplSource<'db>, PotentialBlockRes)>,
    ) -> HashMap<ImplSource<'db>, PotentialBlockRes> {
        let competing_impls = competing_impls.into_iter().collect::<Box<[_]>>();
        if competing_impls.len() > 1 {
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

    fn has_implementation(
        &mut self,
        id: InterfaceId,
        ty: &InferTy,
        templates: &[InferTy],
    ) -> bool {
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

    pub(super) fn add_implementation(
        &mut self,
        id: InterfaceId,
        ty: InferTy,
        templates: &[InferTy],
    ) {
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

    fn try_default_constraint(
        &mut self,
        constraint: &InferenceConstraint,
    ) -> ConstraintSolveResult {
        if !constraint.kind.has_default_behaviour() {
            panic!(
                "Cannot call `try_default_constraint` method on constraint that has no default behaviour"
            )
        }
        match &constraint.kind {
            InferenceConstraintKind::IntLike { res_ty } => {
                match self.unify(res_ty.into(), self.int_ty()) {
                    Ok(()) => ConstraintSolveResult::Solved,
                    Err(err) => ConstraintSolveResult::Error(err),
                }
            }
            InferenceConstraintKind::Deref { var, target } => {
                let adt = InferTy::Adt {
                    def: TypeDefId::Builtin(BuiltinTypeId::ref_(self.db)),
                    fields: Box::new([target.clone()]),
                };
                match self.unify(var.into(), adt) {
                    Ok(()) => ConstraintSolveResult::Solved,
                    Err(err) => ConstraintSolveResult::Error(err),
                }
            }
            _ => {
                unreachable!(
                    "`try_default_constraint` is not in sync with `has_default_behaviour`"
                )
            }
        }
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
        }
    }

    pub fn solve_constraints(
        &mut self,
    ) -> Result<(), (Arc<InferenceConstraint>, UnificationError)> {
        loop {
            while let Some(id) = self.ready.pop_front() {
                let constraint = self.all_constraints[&id].clone();
                match self.try_solve_constraint(&constraint) {
                    ConstraintSolveResult::Solved => {
                        self.solved_constraints.insert(id);
                    }
                    ConstraintSolveResult::Pending => {
                        self.register_listeners(&constraint);
                    }
                    ConstraintSolveResult::Error(e) => {
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

    fn fresh_constraint(
        &mut self,
        constraint: InferenceConstraintKind,
    ) -> InferenceConstraint {
        let res = InferenceConstraint {
            id: InferenceConstraintId(self.next_constraint_id),
            kind: constraint,
        };
        self.next_constraint_id += 1;
        res
    }

    pub fn register_listeners(&mut self, constraint: &InferenceConstraint) {
        for listener in constraint.listeners(self) {
            self.listeners
                .entry(listener)
                .or_default()
                .push(constraint.id);
        }
    }

    pub fn emit_constraint(&mut self, constraint: InferenceConstraintKind) {
        let constraint = Arc::new(self.fresh_constraint(constraint));
        self.ready.push_back(constraint.id);
        self.register_listeners(&constraint);
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

    pub fn emit_indexed_by_constraint(
        &mut self,
        base_ty: InferTy,
        index_ty: InferTy,
    ) -> InferVar {
        let elem_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::IndexedBy {
            elem_var,
            base_ty,
            index_ty,
        });
        elem_var
    }

    pub fn emit_tuple_constraint(
        &mut self,
        tuple_ty: InferTy,
        has_index: u32,
    ) -> InferVar {
        let elem_var = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::Tuple {
            elem_var,
            tuple_ty,
            has_index,
        });
        elem_var
    }

    pub fn emit_struct_field_constraint(
        &mut self,
        struct_ty: InferTy,
        field: Symbol,
    ) -> InferVar {
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
        self.emit_constraint(InferenceConstraintKind::Method(MethodConstraint {
            ret_var,
            ty,
            id,
            method,
            args,
            interface_hint,
            is_static,
        }));
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

    pub fn emit_binop_constraint(
        &mut self,
        lhs_ty: InferTy,
        rhs_ty: InferTy,
        op: BinaryOperator,
    ) -> InferVar {
        let res_ty = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::Binop {
            res_ty,
            lhs_ty,
            rhs_ty,
            op,
        });
        res_ty
    }

    pub fn emit_intlike_constraint(&mut self) -> InferVar {
        let res_ty = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::IntLike { res_ty });
        res_ty
    }

    pub fn emit_binds_like_constraint(
        &mut self,
        like: InferVar,
        ty: InferTy,
    ) -> InferVar {
        let res_ty = self.fresh_var();
        self.emit_constraint(InferenceConstraintKind::BindsLike {
            ty: res_ty,
            inner: ty,
            like,
        });
        res_ty
    }

    pub fn emit_is_inner_constraint(&mut self, inner: InferTy, ref_ty: InferTy) {
        self.emit_constraint(InferenceConstraintKind::IsInner { inner, ref_ty });
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

        if let Some((lid, _)) = lhs_ty.is_adt() {
            if let Some((rid, _)) = rhs_ty.is_adt()
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
}

impl InferenceConstraintKind {
    pub fn display<'a, 'b>(&'a self, db: &'b dyn Db) -> Display<'b, &'a Self> {
        Display { value: self, db }
    }
}

impl fmt::Display for Display<'_, &InferenceConstraintKind> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            InferenceConstraintKind::Deref { var, target } => {
                write!(
                    f,
                    "Deref {{var: {}, target: {}}}",
                    InferTy::Var(*var).to_string(self.db),
                    target.to_string(self.db)
                )
            }
            InferenceConstraintKind::BindsLike { ty, inner, like } => {
                write!(
                    f,
                    "BindsLike {{ty: {}, inner: {}, like: {}}}",
                    InferTy::Var(*ty).to_string(self.db),
                    inner.to_string(self.db),
                    InferTy::Var(*like).to_string(self.db)
                )
            }
            InferenceConstraintKind::IndexedBy {
                elem_var,
                base_ty,
                index_ty,
            } => {
                write!(
                    f,
                    "IndexedBy {{elem_var: {}, base_ty: {}, index_ty: {}}}",
                    InferTy::Var(*elem_var).to_string(self.db),
                    base_ty.to_string(self.db),
                    index_ty.to_string(self.db)
                )
            }
            InferenceConstraintKind::Tuple {
                elem_var,
                tuple_ty,
                has_index,
            } => {
                write!(
                    f,
                    "Tuple {{elem_var: {}, tuple_ty: {}, has_index: {has_index}}}",
                    InferTy::Var(*elem_var).to_string(self.db),
                    tuple_ty.to_string(self.db),
                )
            }
            InferenceConstraintKind::StructField {
                elem_var,
                struct_ty,
                field,
            } => {
                write!(
                    f,
                    "StructField {{elem_var: {}, struct_ty: {}, field: {}}}",
                    InferTy::Var(*elem_var).to_string(self.db),
                    struct_ty.to_string(self.db),
                    field.display(self.db)
                )
            }
            InferenceConstraintKind::Method(MethodConstraint {
                ret_var,
                ty,
                id,
                method,
                args,
                interface_hint,
                is_static,
            }) => {
                write!(
                    f,
                    "Method {{ret_var: {}, ty: {}, id: ExprId({:?}), method: {}, args: [{}], interface_hint: {}, is_static: {is_static}}}",
                    InferTy::Var(*ret_var).to_string(self.db),
                    ty.to_string(self.db),
                    id.0,
                    method.display(self.db),
                    args.iter().map(|a| a.to_string(self.db)).join(", "),
                    match interface_hint {
                        Some(hint) => hint.to_string(self.db).to_string(),
                        None => "".to_string(),
                    }
                )
            }
            InferenceConstraintKind::Implements { ty, id, args } => {
                write!(
                    f,
                    "Implements {{ty: {}, id: {}, args: [{}]}}",
                    ty.to_string(self.db),
                    id.to_string(self.db),
                    args.iter().map(|a| a.to_string(self.db)).join(", "),
                )
            }
            InferenceConstraintKind::Unify { a, b } => {
                write!(
                    f,
                    "Unify {{a: {}, b: {}}}",
                    a.to_string(self.db),
                    b.to_string(self.db),
                )
            }
            InferenceConstraintKind::Binop {
                res_ty,
                lhs_ty,
                rhs_ty,
                op,
            } => {
                write!(
                    f,
                    "Binop<{op}> {{lhs: {}, rhs: {}, res_ty: {res_ty}}}",
                    lhs_ty.to_string(self.db),
                    rhs_ty.to_string(self.db),
                )
            }
            InferenceConstraintKind::IntLike { res_ty } => {
                write!(f, "IntLike {{ res_ty: {res_ty} }}")
            }
            InferenceConstraintKind::IsInner { inner, ref_ty } => write!(
                f,
                "IsInner {{ inner: {}, ref_ty: {} }}",
                inner.to_string(self.db),
                ref_ty.to_string(self.db)
            ),
        }
    }
}

impl InferenceConstraint {
    #[inline(always)]
    pub fn listeners(&self, ctx: &mut InferenceCtx) -> HashSet<InferVar> {
        self.kind.listeners(ctx)
    }
}

impl InferTy {
    pub fn listeners(&self) -> HashSet<InferVar> {
        match self {
            InferTy::Var(var) => [*var].into(),
            InferTy::Adt { fields, .. } => {
                fields.iter().flat_map(|f| f.listeners()).collect()
            }
            InferTy::Param(_) => HashSet::new(),
        }
    }
}

impl InferenceConstraintKind {
    pub fn listeners(&self, ctx: &mut InferenceCtx) -> HashSet<InferVar> {
        match self {
            InferenceConstraintKind::Deref { var, target } => ctx
                .find(target)
                .listeners()
                .into_iter()
                .chain(ctx.find(&var.into()).listeners())
                .collect(),
            InferenceConstraintKind::BindsLike { ty, inner, like } => ctx
                .find(inner)
                .listeners()
                .into_iter()
                .chain(ctx.find(&ty.into()).listeners())
                .chain(ctx.find(&like.into()).listeners())
                .collect(),
            InferenceConstraintKind::IndexedBy {
                elem_var,
                base_ty,
                index_ty,
            } => ctx
                .find(base_ty)
                .listeners()
                .into_iter()
                .chain(ctx.find(index_ty).listeners())
                .chain(ctx.find(&elem_var.into()).listeners())
                .collect(),
            InferenceConstraintKind::Tuple {
                elem_var, tuple_ty, ..
            } => ctx
                .find(tuple_ty)
                .listeners()
                .into_iter()
                .chain(ctx.find(&elem_var.into()).listeners())
                .collect(),
            InferenceConstraintKind::StructField {
                elem_var,
                struct_ty,
                ..
            } => ctx
                .find(struct_ty)
                .listeners()
                .into_iter()
                .chain(ctx.find(&elem_var.into()).listeners())
                .collect(),
            InferenceConstraintKind::Method(MethodConstraint {
                ret_var,
                ty,
                args,
                ..
            }) => args
                .iter()
                .flat_map(|t| ctx.find(t).listeners())
                .collect::<Box<_>>()
                .into_iter()
                .chain(ty.listeners())
                .chain(ctx.find(&ret_var.into()).listeners())
                .collect(),
            InferenceConstraintKind::Implements { ty, args, .. } => args
                .iter()
                .flat_map(|t| ctx.find(t).listeners())
                .collect::<Box<_>>()
                .into_iter()
                .chain(ctx.find(ty).listeners())
                .collect(),
            InferenceConstraintKind::Unify { a, b } => ctx
                .find(a)
                .listeners()
                .into_iter()
                .chain(ctx.find(b).listeners())
                .collect(),
            InferenceConstraintKind::Binop {
                res_ty,
                lhs_ty,
                rhs_ty,
                ..
            } => lhs_ty
                .listeners()
                .into_iter()
                .chain(rhs_ty.listeners())
                .chain(ctx.find(&res_ty.into()).listeners())
                .collect(),
            InferenceConstraintKind::IntLike { res_ty } => {
                ctx.find(&res_ty.into()).listeners()
            }
            InferenceConstraintKind::IsInner { inner, ref_ty } => ctx
                .find(inner)
                .listeners()
                .into_iter()
                .chain(ctx.find(ref_ty).listeners())
                .collect(),
        }
    }
}
