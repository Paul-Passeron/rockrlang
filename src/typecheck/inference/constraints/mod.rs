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
    common::{location::Span, symbols::Symbol},
    hir::interface_items,
    parse_tree::{
        expr::BinaryOperator,
        top_level::{AstInterfaceItem, AstMethodsig},
    },
    resolved::{
        BuiltinTypeId, FunctionId, ImplSource, InterfaceId, InterfaceRef, ScopeOwnerId,
        TypeDefId, TypeId, TypeRef,
    },
    typecheck::{
        CallKind, ExprId, InferCallInfos,
        inference::{
            InferTy, InferenceCtx, InterfaceImplem, UnificationError,
            implems::PotentialBlockRes, implicit::ImplicitContext, var::InferVar,
        },
    },
};

pub mod emit;
pub mod solve;

const MAX_IMPL_DEPTH: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InferenceConstraintId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InferenceConstraint {
    pub id: InferenceConstraintId,
    pub kind: InferenceConstraintKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InferenceConstraintKind {
    Deref { var: InferVar, target: InferTy },
    BindsLike { ty: InferVar, inner: InferTy, like: InferVar },
    IndexedBy { elem_var: InferVar, base_ty: InferTy, index_ty: InferTy },
    Tuple { elem_var: InferVar, tuple_ty: InferTy, has_index: u32 },
    StructField { elem_var: InferVar, struct_ty: InferTy, field: Symbol },
    Method(MethodConstraint),
    Implements { ty: InferTy, id: InterfaceId, args: Box<[InferTy]> },
    Unify { a: InferTy, b: InferTy },
    Binop { res_ty: InferVar, lhs_ty: InferTy, rhs_ty: InferTy, op: BinaryOperator },
    IntLike { res_ty: InferVar },
    IsInner { inner: InferTy, ref_ty: InferTy },
    FatPtr { fat_ptr_var: InferVar },
    MetadataOfFatPtr { fat_ptr_var: InferVar, metadata_var: InferVar },
    Cast { from: InferTy, to: InferTy },
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
        matches!(self, Self::Deref { .. } | Self::IntLike { .. })
    }
}

#[derive(Debug)]
enum ConstraintSolveResult {
    Solved,
    Pending,
    Error(UnificationError),
}

impl InferenceCtx<'_> {
    fn is_builtin_indexed_by_int(&self, ty: &InferTy) -> Option<InferTy> {
        self.is_slice(ty).or_else(|| self.is_ref_to_slice(ty)).or_else(|| self.is_ptr(ty))
    }
}

impl<'db> InferenceCtx<'db> {
    fn infer_to_ref(&mut self, ty: &InferTy) -> TypeRef {
        let ty = self.find(ty);
        match ty {
            InferTy::Var(_) => TypeRef::Error,
            InferTy::Adt { def, fields } => {
                let args =
                    fields.into_iter().map(|f| self.infer_to_ref(&f)).collect::<Vec<_>>();
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
        method_constraint: &MethodConstraint,
    ) -> Option<ConstraintSolveResult> {
        let mut cur = self.find(&method_constraint.ty);
        let mut depth = 0;
        loop {
            if let Some((iface_id, implem, sig)) =
                self.find_known_impl_method(&cur, method_constraint)
            {
                return Some(self.apply_interface_method(
                    method_constraint,
                    depth,
                    iface_id,
                    &implem,
                    &sig,
                ));
            }
            let (_, inner) = cur.ptr_like(self.db)?;
            cur = inner.clone();
            depth += 1;
        }
    }

    fn find_known_impl_method(
        &mut self,
        cur: &InferTy,
        method_constraint: &MethodConstraint,
    ) -> Option<(InterfaceId, InterfaceImplem, AstMethodsig)> {
        let MethodConstraint { method, interface_hint, args, is_static, .. } =
            method_constraint;
        let candidates = self
            .implements
            .iter()
            .filter(|(iface_id, _)| interface_hint.is_none_or(|hint| hint == **iface_id))
            .flat_map(|(iface_id, implems)| {
                implems.iter().map(|implem| (*iface_id, implem.clone()))
            })
            .collect_vec();

        for (iface_id, implem) in candidates {
            if self.find(&implem.ty) != *cur {
                continue;
            }

            let sig =
                interface_items(self.db, iface_id.interned()).iter().find_map(|item| {
                    match item {
                        AstInterfaceItem::Sig(sig)
                            if sig.data.name.data == *method
                                && sig.data.receiver.is_static() == *is_static
                                && sig.data.args.len() == args.len() =>
                        {
                            Some(sig.clone())
                        }
                        _ => None,
                    }
                });

            if let Some(sig) = sig {
                return Some((iface_id, implem, sig.as_ref().clone()));
            }
        }

        None
    }

    fn apply_interface_method(
        &mut self,
        method_constraint: &MethodConstraint,
        depth: usize,
        iface_id: InterfaceId,
        implem: &InterfaceImplem,
        sig: &AstMethodsig,
    ) -> ConstraintSolveResult {
        let MethodConstraint {
            ret_var,
            ty: receiver,
            id: expr_id,
            method,
            args,
            is_static,
            ..
        } = method_constraint;
        assert_eq!(sig.data.receiver.is_static(), *is_static);
        let ref_args =
            implem.templates.iter().map(|a| self.infer_to_ref(a)).collect::<Vec<_>>();
        if ref_args.iter().any(|a| matches!(a, TypeRef::Error)) {
            return ConstraintSolveResult::Pending;
        }
        let iface_ref = InterfaceRef::new(self.db, iface_id, ref_args);

        let zelf_ty = self.peel_receiver(receiver, depth);

        if let Err(e) = self.unify(&zelf_ty, &implem.ty) {
            return ConstraintSolveResult::Error(e);
        }

        let method_id =
            FunctionId::new(self.db, *method, ScopeOwnerId::Interface(iface_ref));

        let method_templates = implem
            .templates
            .iter()
            .cloned()
            .chain(sig.data.template_args.iter().map(|t| {
                if !t.constraints.is_empty() {
                    todo!()
                }
                self.fresh_var().into()
            }))
            .collect::<Arc<[_]>>();

        let ctx = ImplicitContext::new(
            self.db,
            ScopeOwnerId::Interface(iface_ref),
            &[],
            method_templates.clone(),
            Some(zelf_ty.clone()),
        );

        let ret_ty = self.allocate_ast_type_expr(&sig.data.return_type.data, &ctx);
        if let Err(e) = self.unify(&ret_var.into(), &ret_ty) {
            return ConstraintSolveResult::Error(e);
        }

        for (ast_arg, call_arg) in sig.data.args.iter().zip(args) {
            let expected = self.allocate_ast_type_expr(&ast_arg.ty.data, &ctx);
            if let Err(e) = self.unify(&expected, call_arg) {
                return ConstraintSolveResult::Error(e);
            }
        }

        let call_infos = InferCallInfos {
            expr_id: *expr_id,
            callee: method_id,
            substitution: method_templates.iter().cloned().collect_vec(),
            call_kind: if *is_static {
                CallKind::Static
            } else {
                CallKind::Method {
                    adjustment: self.get_adjustments_for(method_id, depth),
                }
            },
            zelf_ty: Some(zelf_ty),
        };

        self.call_infos.insert(*expr_id, call_infos);
        ConstraintSolveResult::Solved
    }

    fn get_working_impls(
        &self,
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
        for implem in &self
            .implements
            .get(&id)
            .iter()
            .flat_map(|the_impl| the_impl.iter().cloned())
            .collect::<Box<[_]>>()
        {
            let implem_ty = self.find(&implem.ty);
            let implem_templates =
                implem.templates.iter().map(|t| self.find(t)).collect::<Box<[_]>>();
            if ty == implem_ty && templates == implem_templates {
                return true;
            }
        }
        false
    }

    pub(super) fn add_implementation(
        &mut self,
        id: InterfaceId,
        ty: &InferTy,
        templates: &[InferTy],
    ) {
        let ty = self.find(ty);
        let templates = templates.iter().map(|t| self.find(t)).collect::<Box<[_]>>();
        if let Entry::Vacant(e) = self.implements.entry(id) {
            e.insert(once(InterfaceImplem { interface: id, ty, templates }).collect());
        } else {
            for implem in self.implements.get(&id).iter().flat_map(|implem| implem.iter())
            {
                let implem_ty = self.find_const(&implem.ty);
                let implem_templates = implem
                    .templates
                    .iter()
                    .map(|t| self.find_const(t))
                    .collect::<Box<[_]>>();
                if ty == implem_ty && templates == implem_templates {
                    return;
                }
            }
            self.implements.entry(id).or_default().insert(InterfaceImplem {
                interface: id,
                ty,
                templates,
            });
        }
    }

    fn try_default_constraint(
        &mut self,
        constraint: &InferenceConstraint,
    ) -> ConstraintSolveResult {
        assert!(
            constraint.kind.has_default_behaviour(),
            "Cannot call `try_default_constraint` method on constraint that has no default behaviour"
        );
        match &constraint.kind {
            InferenceConstraintKind::IntLike { res_ty } => {
                match self.unify(&res_ty.into(), &self.int_ty()) {
                    Ok(()) => ConstraintSolveResult::Solved,
                    Err(err) => ConstraintSolveResult::Error(err),
                }
            }
            InferenceConstraintKind::Deref { var, target } => {
                let adt = InferTy::Adt {
                    def: TypeDefId::Builtin(BuiltinTypeId::const_ref(self.db)),
                    fields: vec![target.clone()],
                };
                match self.unify(&var.into(), &adt) {
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

    fn fresh_constraint(
        &mut self,
        constraint: InferenceConstraintKind,
    ) -> InferenceConstraint {
        let res = InferenceConstraint {
            id: InferenceConstraintId(self.next_constraint_id),
            kind: constraint,
            span: self.current_span,
        };
        self.next_constraint_id += 1;
        res
    }

    pub fn register_listeners(&mut self, constraint: &InferenceConstraint) {
        for listener in constraint.listeners(self) {
            self.record_listeners(listener);
            self.listeners.entry(listener).or_default().push(constraint.id);
        }
    }
}

pub struct ICKDisplay<'db, 'a, T> {
    pub value: T,
    pub db: &'db dyn Db,
    pub ctx: &'a InferenceCtx<'db>,
}

impl InferenceConstraintKind {
    pub fn display<'a, 'b, 'db>(
        &'a self,
        ctx: &'b InferenceCtx<'db>,
    ) -> ICKDisplay<'db, 'b, &'a Self> {
        ICKDisplay { value: self, db: ctx.db, ctx }
    }
}

impl fmt::Display for ICKDisplay<'_, '_, &InferenceConstraintKind> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            InferenceConstraintKind::Deref { var, target } => {
                write!(
                    f,
                    "Deref {{var: {}, target: {}}}",
                    self.ctx.find_const(&InferTy::Var(*var)).to_string(self.db),
                    target.to_string(self.db)
                )
            }
            InferenceConstraintKind::BindsLike { ty, inner, like } => {
                write!(
                    f,
                    "BindsLike {{ty: {}, inner: {}, like: {}}}",
                    self.ctx.find_const(&InferTy::Var(*ty)).to_string(self.db),
                    inner.to_string(self.db),
                    self.ctx.find_const(&InferTy::Var(*like)).to_string(self.db)
                )
            }
            InferenceConstraintKind::IndexedBy { elem_var, base_ty, index_ty } => {
                write!(
                    f,
                    "IndexedBy {{elem_var: {}, base_ty: {}, index_ty: {}}}",
                    self.ctx.find_const(&InferTy::Var(*elem_var)).to_string(self.db),
                    self.ctx.find_const(base_ty).to_string(self.db),
                    self.ctx.find_const(index_ty).to_string(self.db)
                )
            }
            InferenceConstraintKind::Tuple { elem_var, tuple_ty, has_index } => {
                write!(
                    f,
                    "Tuple {{elem_var: {}, tuple_ty: {}, has_index: {has_index}}}",
                    self.ctx.find_const(&InferTy::Var(*elem_var)).to_string(self.db),
                    self.ctx.find_const(tuple_ty).to_string(self.db),
                )
            }
            InferenceConstraintKind::StructField { elem_var, struct_ty, field } => {
                write!(
                    f,
                    "StructField {{elem_var: {}, struct_ty: {}, field: {}}}",
                    self.ctx.find_const(&InferTy::Var(*elem_var)).to_string(self.db),
                    self.ctx.find_const(struct_ty).to_string(self.db),
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
                    self.ctx.find_const(&InferTy::Var(*ret_var)).to_string(self.db),
                    self.ctx.find_const(ty).to_string(self.db),
                    id.0,
                    method.display(self.db),
                    args.iter()
                        .map(|a| self.ctx.find_const(a).to_string(self.db))
                        .join(", "),
                    match interface_hint {
                        Some(hint) => hint.to_string(self.db),
                        None => String::new(),
                    }
                )
            }
            InferenceConstraintKind::Implements { ty, id, args } => {
                write!(
                    f,
                    "Implements {{ty: {}, id: {}, args: [{}]}}",
                    self.ctx.find_const(ty).to_string(self.db),
                    id.to_string(self.db),
                    args.iter()
                        .map(|a| self.ctx.find_const(a).to_string(self.db))
                        .join(", "),
                )
            }
            InferenceConstraintKind::Unify { a, b } => {
                write!(
                    f,
                    "Unify {{a: {}, b: {}}}",
                    self.ctx.find_const(a).to_string(self.db),
                    self.ctx.find_const(b).to_string(self.db),
                )
            }
            InferenceConstraintKind::Binop { res_ty, lhs_ty, rhs_ty, op } => {
                write!(
                    f,
                    "Binop<{op}> {{lhs: {}, rhs: {}, res_ty: {res_ty}}}",
                    self.ctx.find_const(lhs_ty).to_string(self.db),
                    self.ctx.find_const(rhs_ty).to_string(self.db),
                )
            }
            InferenceConstraintKind::IntLike { res_ty } => {
                write!(f, "IntLike {{ res_ty: {res_ty} }}")
            }
            InferenceConstraintKind::IsInner { inner, ref_ty } => write!(
                f,
                "IsInner {{ inner: {}, ref_ty: {} }}",
                self.ctx.find_const(inner).to_string(self.db),
                self.ctx.find_const(ref_ty).to_string(self.db)
            ),
            InferenceConstraintKind::FatPtr { fat_ptr_var } => {
                write!(f, "FatPtr {{ fat_ptr_var: {fat_ptr_var} }}")
            }
            InferenceConstraintKind::MetadataOfFatPtr { fat_ptr_var, metadata_var } => {
                write!(
                    f,
                    "MetadataOfFatPtr {{ fat_ptr_var: {fat_ptr_var}, metadata_var: {metadata_var} }}"
                )
            }
            InferenceConstraintKind::Cast { from, to } => {
                write!(
                    f,
                    "Cast {{from: {}, to: {}}}",
                    self.ctx.find_const(from).to_string(self.db),
                    self.ctx.find_const(to).to_string(self.db)
                )
            }
        }
    }
}

impl InferenceConstraint {
    pub fn listeners(&self, ctx: &mut InferenceCtx) -> HashSet<InferVar> {
        self.kind.listeners(ctx)
    }
}

impl InferTy {
    pub fn listeners(&self) -> HashSet<InferVar> {
        match self {
            Self::Var(var) => [*var].into(),
            Self::Adt { fields, .. } => fields.iter().flat_map(Self::listeners).collect(),
            Self::Param(_) => HashSet::new(),
        }
    }
}

impl InferenceConstraintKind {
    pub fn listeners(&self, ctx: &mut InferenceCtx) -> HashSet<InferVar> {
        match self {
            Self::Deref { var, target } => ctx
                .find(target)
                .listeners()
                .into_iter()
                .chain(ctx.find(&var.into()).listeners())
                .collect(),
            Self::BindsLike { ty, inner, like } => ctx
                .find(inner)
                .listeners()
                .into_iter()
                .chain(ctx.find(&ty.into()).listeners())
                .chain(ctx.find(&like.into()).listeners())
                .collect(),
            Self::IndexedBy { elem_var, base_ty, index_ty } => ctx
                .find(base_ty)
                .listeners()
                .into_iter()
                .chain(ctx.find(index_ty).listeners())
                .chain(ctx.find(&elem_var.into()).listeners())
                .collect(),
            Self::Tuple { elem_var, tuple_ty, .. } => ctx
                .find(tuple_ty)
                .listeners()
                .into_iter()
                .chain(ctx.find(&elem_var.into()).listeners())
                .collect(),
            Self::StructField { elem_var, struct_ty, .. } => ctx
                .find(struct_ty)
                .listeners()
                .into_iter()
                .chain(ctx.find(&elem_var.into()).listeners())
                .collect(),
            Self::Method(MethodConstraint { ret_var, ty, args, .. }) => args
                .iter()
                .flat_map(|t| ctx.find(t).listeners())
                .collect::<Box<_>>()
                .into_iter()
                .chain(ty.listeners())
                .chain(ctx.find(&ret_var.into()).listeners())
                .collect(),
            Self::Implements { ty, args, .. } => args
                .iter()
                .flat_map(|t| ctx.find(t).listeners())
                .collect::<Box<_>>()
                .into_iter()
                .chain(ctx.find(ty).listeners())
                .collect(),
            Self::Unify { a, b } => ctx
                .find(a)
                .listeners()
                .into_iter()
                .chain(ctx.find(b).listeners())
                .collect(),
            Self::Binop { res_ty, lhs_ty, rhs_ty, .. } => lhs_ty
                .listeners()
                .into_iter()
                .chain(rhs_ty.listeners())
                .chain(ctx.find(&res_ty.into()).listeners())
                .collect(),
            Self::IntLike { res_ty } => ctx.find(&res_ty.into()).listeners(),
            Self::IsInner { inner, ref_ty } => ctx
                .find(inner)
                .listeners()
                .into_iter()
                .chain(ctx.find(ref_ty).listeners())
                .collect(),
            Self::FatPtr { fat_ptr_var } => ctx.find(&fat_ptr_var.into()).listeners(),
            Self::MetadataOfFatPtr { fat_ptr_var, metadata_var } => ctx
                .find(&fat_ptr_var.into())
                .listeners()
                .into_iter()
                .chain(ctx.find(&metadata_var.into()).listeners())
                .collect(),
            Self::Cast { from, to } => ctx
                .find(from)
                .listeners()
                .into_iter()
                .chain(ctx.find(to).listeners())
                .collect(),
        }
    }
}
