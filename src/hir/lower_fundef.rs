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
    collections::{BTreeMap, HashSet},
    iter,
};

use itertools::Itertools;
use salsa::Accumulator;

use crate::{
    Db,
    common::{
        arena::Arena,
        ids::{IdGen, IdWrapper},
        location::Span,
        symbols::Symbol,
    },
    compiler::diagnostic::Diag,
    hir::{
        HirBody, HirConstructorArgs, HirExpr, HirExprDesc, HirId, HirMatchBranch,
        HirPattern, HirPatternConstructorArgs, HirPatternDesc, HirPlace, HirPlaceKind,
        HirStmt, HirStmtKind, HirStructFieldPattern, LocalId, LocalInfo, Mutability,
    },
    name_resolve::{
        definition::{Definition, get_module_pretty_name, resolve_in_module},
        interfaces::{
            core_int_iter_struct, core_into_iterator_interface, core_iter_interface,
        },
        type_expr::{
            enum_item, get_template_param_count, get_templates_of_fun, templates_of_enum,
        },
    },
    parse_tree::{
        expr::{AstExpr, AstExprDesc, AstStructField, BinaryOperator},
        pattern::{
            AstConstructFields, AstNamedPattern, AstPattern, AstPatternDesc,
            StructFieldPattern,
        },
        stmt::{AstMatchBranch, AstStmt, AstStmtDesc},
        top_level::{
            AstEnumVariantKind, AstFundef, AstFundefArg, AstMethodDef, AstReceiver,
            AstTemplateArg,
        },
        type_expr::{AstAnyTypeExpr, AstAnyTypeExprDesc, AstTypeExpr, AstTypeExprDesc},
    },
    ril::{
        BuiltinTypeId, FunctionId, ModuleId, ScopeOwnerId, TypeDefId, TypeId,
        TypeParamId, TypeRef, compute_template_hints, rehole, tuple_of,
    },
    thir::EnumRef,
    typecheck::inference::expr::diagnose_bad_struct_fields,
};

pub struct LowerFundef<'db> {
    pub db: &'db dyn Db,
    pub function: FunctionId,
    pub module: ModuleId,
    pub template_args: Vec<AstTemplateArg>,
    pub alloc: IdWrapper<HirId>,
    pub locals: Arena<LocalInfo>,
    pub iterator_id: IdGen,
}

#[derive(Debug, Clone)]
pub struct Scope {
    pub map: BTreeMap<Symbol, LocalId>,
}

impl Scope {
    fn new() -> Self {
        Self { map: BTreeMap::new() }
    }
}

impl<'db> LowerFundef<'db> {
    fn new(
        db: &'db dyn Db,
        function: FunctionId,
        module: ModuleId,
        template_args: Vec<AstTemplateArg>,
    ) -> Self {
        Self {
            db,
            function,
            module,
            template_args,
            alloc: IdWrapper::new(),
            locals: Arena::new(),
            iterator_id: IdGen::new(),
        }
    }

    pub fn allocate_local(
        &mut self,
        scope: &mut Scope,
        name: Symbol,
        mutability: Mutability,
        ty_annotation: Option<AstAnyTypeExpr>,
        span: Span,
        is_synthetic: bool,
    ) -> LocalId {
        let id = self.locals.next_id();
        let info = LocalInfo { id, name, mutability, ty_annotation, span, is_synthetic };
        let new_id = self.locals.insert(info);
        debug_assert_eq!(id, new_id);
        scope.map.insert(name, id);
        id
    }

    fn expr_as_place(
        &mut self,
        expr: &AstExpr,
        scope: &Scope,
        module: ModuleId,
    ) -> HirPlace {
        match &expr.data {
            AstExprDesc::Name(symbol) => {
                if let Some(id) = scope.map.get(symbol) {
                    self.new_place(HirPlaceKind::Local(*id), expr.span, false)
                } else {
                    todo!(
                        "expr_as_place: Name `{}` not found in local scope",
                        symbol.interned().contents(self.db)
                    )
                }
            }
            AstExprDesc::NameResolved { from, to } => {
                match resolve_in_module(self.db, from.data, module) {
                    Some(Definition::Module(inner)) => {
                        self.expr_as_place(to, scope, inner)
                    }
                    _ => {
                        let temp = self.lower_expr(expr, scope, module);
                        self.new_place(
                            HirPlaceKind::Temporary(temp.boxed()),
                            expr.span,
                            false,
                        )
                    }
                }
            }
            AstExprDesc::PostfixDeref(inner) | AstExprDesc::PrefixDeref(inner) => {
                let temp = self.expr_as_place(inner, scope, module);
                self.new_place(HirPlaceKind::Deref(temp.boxed()), expr.span, false)
            }

            AstExprDesc::FieldAccess { object, field } => {
                let base = self.expr_as_place(object, scope, module);
                self.new_place(
                    HirPlaceKind::Field { base: base.boxed(), field: *field },
                    expr.span,
                    false,
                )
            }
            AstExprDesc::TupleAccess { object, index } => {
                let base = self.expr_as_place(object, scope, module);
                self.new_place(
                    HirPlaceKind::TupleField { base: base.boxed(), index: *index },
                    expr.span,
                    false,
                )
            }
            AstExprDesc::Index { object, index } => {
                let base = self.expr_as_place(object, scope, module);
                let index = self.lower_expr(index, scope, self.module);
                self.new_place(
                    HirPlaceKind::Index { base: base.boxed(), index: index.boxed() },
                    expr.span,
                    false,
                )
            }
            _ => {
                let temp = self.lower_expr(expr, scope, module);
                self.new_place(HirPlaceKind::Temporary(temp.boxed()), expr.span, false)
            }
        }
    }

    fn instantiate_holed(&self, ty: TypeDefId) -> TypeRef {
        let n = get_template_param_count(self.db, ty);
        TypeId::new(self.db, ty, iter::repeat_n(TypeRef::Unknown, n).collect()).into()
    }

    fn resolve_any_holed_arg(
        &self,
        any_ty: &AstAnyTypeExpr,
        module: ModuleId,
    ) -> TypeRef {
        match &any_ty.data {
            AstAnyTypeExprDesc::Any => TypeRef::Unknown,
            AstAnyTypeExprDesc::Known(desc) => self.resolve_holed_desc(
                &AstTypeExpr {
                    annotations: Vec::new(),
                    data: desc.clone(),
                    span: any_ty.span,
                },
                module,
            ),
        }
    }

    fn resolve_holed_desc(&self, desc: &AstTypeExpr, module: ModuleId) -> TypeRef {
        match &desc.data {
            AstTypeExprDesc::Named { name, args } => {
                if args.is_empty() {
                    if *name == Symbol::new(self.db, "Self") {
                        return TypeRef::Zelf;
                    }
                    if let Some(idx) =
                        self.template_args.iter().position(|p| p.name == *name)
                    {
                        return TypeRef::Param(TypeParamId(idx));
                    }
                }

                match resolve_in_module(self.db, *name, module) {
                    Some(resolution) => {
                        let type_def_id = match resolution {
                            Definition::Type(type_def_id) => type_def_id,
                            Definition::Interface(id) => {
                                todo!("Interface {} here", id.to_string(self.db))
                            }
                            other_def => {
                                todo!("{}", other_def.to_string(self.db))
                            }
                        };

                        let partial_args: Vec<_> = args
                            .iter()
                            .map(|arg| self.resolve_any_holed_arg(arg, module))
                            .collect();

                        TypeId::new(self.db, type_def_id, partial_args).into()
                    }
                    None => {
                        Diag::generic_error(
                            format!(
                                "{}: Could not resolve name {} in scope",
                                desc.span.start().loc_info(self.db),
                                name.display(self.db),
                            ),
                            desc.span,
                        )
                        .accumulate(self.db);
                        TypeRef::Error
                    }
                }
            }

            AstTypeExprDesc::NameResolved { from, to } => {
                match resolve_in_module(self.db, *from, module) {
                    Some(Definition::Module(inner_module)) => {
                        self.resolve_holed_ty(to, inner_module)
                    }
                    _ => todo!(""),
                }
            }

            AstTypeExprDesc::Pointer { mutable, pointee } => {
                let inner = self.resolve_any(pointee);
                self.wrap_builtin_unary(
                    if *mutable {
                        BuiltinTypeId::mut_ptr(self.db)
                    } else {
                        BuiltinTypeId::const_ptr(self.db)
                    },
                    inner,
                )
            }

            AstTypeExprDesc::Ref { mutable, pointee } => {
                let inner = self.resolve_any(pointee);
                self.wrap_builtin_unary(
                    if *mutable {
                        BuiltinTypeId::mut_ref(self.db)
                    } else {
                        BuiltinTypeId::const_ref(self.db)
                    },
                    inner,
                )
            }

            AstTypeExprDesc::Slice { ty, len: _ } => {
                let inner = self.resolve_any(ty);
                self.wrap_builtin_unary(BuiltinTypeId::slice(self.db), inner)
            }

            AstTypeExprDesc::Tuple(tys) => {
                // TODO: Maybe ?
                // if tys.len() == 0 {
                //     void_id(self.db).into()
                // }
                if tys.len() == 1 {
                    self.resolve_any(&tys[0])
                } else {
                    let partial_args: Vec<TypeRef> = tys
                        .iter()
                        .map(|ty| self.resolve_any_holed_arg(ty, module))
                        .collect();

                    tuple_of(self.db, partial_args).into()
                }
            }
            AstTypeExprDesc::Error(_) => TypeRef::Error,
        }
    }

    fn resolve_holed_ty(&self, ty: &AstTypeExpr, module: ModuleId) -> TypeRef {
        self.resolve_holed_desc(ty, module)
    }

    fn resolve_holed(&self, ty: &AstTypeExpr) -> TypeRef {
        self.resolve_holed_ty(ty, self.module)
    }

    fn resolve_any(&self, ty: &AstAnyTypeExpr) -> TypeRef {
        ty.as_known().map_or(TypeRef::Unknown, |ty| self.resolve_holed(&ty))
    }

    fn wrap_builtin_unary(&self, builtin: BuiltinTypeId, inner: TypeRef) -> TypeRef {
        let def: TypeDefId = builtin.into();
        TypeId::new(self.db, def, vec![inner]).into()
    }

    fn lower_name(
        &mut self,
        symbol: Symbol,
        span: Span,
        scope: &Scope,
        module: ModuleId,
    ) -> HirExprDesc {
        if let Some(id) = scope.map.get(&symbol) {
            HirExprDesc::Use(self.new_place(HirPlaceKind::Local(*id), span, false))
        } else {
            match resolve_in_module(self.db, symbol, module) {
                Some(Definition::Function(_)) => {
                    todo!(
                        "bare function name `{}` used as value expression",
                        symbol.interned().contents(self.db)
                    )
                }
                _ => {
                    Diag::generic_error(
                        format!(
                            "Name `{}` not found in the current scope.",
                            symbol.display(self.db)
                        ),
                        span,
                    )
                    .accumulate(self.db);
                    HirExprDesc::Error
                }
            }
        }
    }

    fn lower_name_resolved(
        &mut self,
        from: Symbol,
        from_span: Span,
        to: &AstExpr,
        scope: &Scope,
        module: ModuleId,
    ) -> HirExprDesc {
        // TODO: compute once
        let template_names = get_templates_of_fun(self.db, self.function.into())
            .iter()
            .map(|ast| ast.name)
            .collect_vec();
        if let Some(pos) = template_names.iter().position(|temp| from == *temp) {
            let tref = TypeRef::Param(TypeParamId(pos));
            match &to.data {
                AstExprDesc::Call { callee, args, type_args } => {
                    let AstExprDesc::Name(callee) = &callee.data else { todo!() };
                    let args = args
                        .iter()
                        .map(|arg| self.lower_expr(arg, scope, self.module))
                        .collect_vec();
                    let type_args =
                        type_args.iter().map(|arg| self.resolve_any(arg)).collect_vec();
                    HirExprDesc::CallStatic { ty: tref, method: *callee, args, type_args }
                }
                _ => todo!(),
            }
        } else if from == Symbol::new(self.db, "Self") {
            // TODO: we're losing template info here
            let zelf = self
                .function
                .parent(self.db)
                .get_canonical_zelf(self.db)
                .expect("TODO: report that");
            match zelf {
                TypeRef::Concrete(type_id) => self.lower_name_resolved_expr_from_type(
                    scope,
                    module,
                    to,
                    type_id.def(self.db),
                ),
                _ => todo!("Handle bad cases"),
            }
        } else {
            match resolve_in_module(self.db, from, module) {
                Some(Definition::Module(module_id)) => {
                    self.lower_expr(to, scope, module_id).data
                }
                Some(Definition::Type(type_def_id)) => self
                    .lower_name_resolved_expr_from_type(scope, module, to, type_def_id),
                _ => {
                    Diag::generic_error(
                        format!(
                            "badly name-resolved item ({})",
                            from.interned().contents(self.db)
                        ),
                        from_span,
                    )
                    .accumulate(self.db);
                    HirExprDesc::Error
                }
            }
        }
    }

    fn lower_static_call(
        &mut self,
        ty: &AstTypeExpr,
        method: Symbol,
        type_args: &[AstAnyTypeExpr],
        args: &[AstExpr],
        scope: &Scope,
        module: ModuleId,
    ) -> HirExprDesc {
        let ty = self.resolve_holed(ty);
        let args = args.iter().map(|a| self.lower_expr(a, scope, module)).collect();
        let type_args = type_args.iter().map(|arg| self.resolve_any(arg)).collect_vec();
        HirExprDesc::CallStatic { ty, method, args, type_args }
    }

    fn lower_binop(
        &mut self,
        lhs: &AstExpr,
        rhs: &AstExpr,
        op: BinaryOperator,
        scope: &Scope,
    ) -> HirExprDesc {
        let lhs = self.lower_expr(lhs, scope, self.module);
        let rhs = self.lower_expr(rhs, scope, self.module);
        HirExprDesc::BinOp { lhs: lhs.boxed(), op, rhs: rhs.boxed() }
    }

    fn lower_range(
        &mut self,
        from: &AstExpr,
        to: &AstExpr,
        scope: &Scope,
    ) -> HirExprDesc {
        let from = self.lower_expr(from, scope, self.module);
        let to = self.lower_expr(to, scope, self.module);

        let int_iter = core_int_iter_struct(self.db);
        let type_ref =
            TypeRef::Concrete(TypeId::new(self.db, TypeDefId::Struct(int_iter), vec![]));
        HirExprDesc::StructLit {
            ty: type_ref,
            fields: vec![
                (Symbol::new(self.db, "start"), from),
                (Symbol::new(self.db, "end"), to),
            ],
        }
    }

    fn lower_call(
        &mut self,
        callee: &AstExpr,
        type_args: &[AstAnyTypeExpr],
        args: &[AstExpr],
        scope: &Scope,
        module: ModuleId,
        span: Span,
    ) -> HirExprDesc {
        // The callee might be a Name that resolves to a function,
        // or a more complex expression (function pointer call, etc.).
        match &callee.data {
            AstExprDesc::Name(symbol) => {
                // Check local scope first (function pointer in a local)
                if let Some(_id) = scope.map.get(symbol) {
                    todo!(
                        "call through local function pointer `{}`",
                        symbol.interned().contents(self.db)
                    )
                }
                let args =
                    args.iter().map(|a| self.lower_expr(a, scope, module)).collect();

                let type_args =
                    type_args.iter().map(|arg| self.resolve_any(arg)).collect_vec();

                // Module-level: must be a free function
                match resolve_in_module(self.db, *symbol, module) {
                    Some(Definition::Function(fid)) => {
                        if !type_args.is_empty() {
                            let temps = get_templates_of_fun(self.db, fid.into());
                            if temps.len() != type_args.len() {
                                todo!("Diag type args mismatch")
                            }
                        }
                        HirExprDesc::CallDirect { target: fid, args, type_args }
                    }
                    _ => HirExprDesc::UnresolvedCallDirect {
                        target: FunctionId::new(
                            self.db,
                            *symbol,
                            ScopeOwnerId::Module(module),
                        ),
                        type_args,
                        args,
                    },
                }
            }
            AstExprDesc::NameResolved { from, to } => {
                let resolved_call = AstExpr::new(
                    AstExprDesc::NameResolved {
                        from: from.clone(),
                        to: Box::new(AstExpr::new(
                            AstExprDesc::Call {
                                callee: to.clone(),
                                args: args.to_vec(),
                                type_args: vec![], // TODO
                            },
                            vec![],
                            span,
                        )),
                    },
                    vec![],
                    span,
                );
                self.lower_expr(&resolved_call, scope, module).data
            }
            _ => {
                todo!("Call with complex callee expression")
            }
        }
    }

    fn lower_method_call(
        &mut self,
        object: &AstExpr,
        method: Symbol,
        type_args: &[AstAnyTypeExpr],
        args: &[AstExpr],
        scope: &Scope,
        module: ModuleId,
    ) -> HirExprDesc {
        let type_args = type_args.iter().map(|arg| self.resolve_any(arg)).collect_vec();
        HirExprDesc::CallMethod {
            receiver: self.lower_expr(object, scope, module).boxed(),
            method,
            args: args.iter().map(|arg| self.lower_expr(arg, scope, module)).collect(),
            interface_hint: None,
            type_args,
        }
    }

    fn as_enum_with_filled_holes(&mut self, ty: &AstTypeExpr) -> Option<EnumRef> {
        let partial_ty = self.resolve_holed(ty).as_type_id()?;
        let enum_def = match partial_ty.def(self.db) {
            TypeDefId::Enum(enum_id) => Some(enum_id),
            _ => None,
        }?;
        let n = templates_of_enum(self.db, enum_def.interned()).len();

        let args = partial_ty.args(self.db);

        if n < partial_ty.args(self.db).len() {
            // TODO: error recovery, ignore them ?
            // Diagnose ?
            return None;
        }

        let template_hints = (0..n)
            .into_iter()
            .map(|i| args.get(i).copied().unwrap_or(TypeRef::Unknown))
            .collect();

        Some(EnumRef { def: enum_def, args: template_hints })
    }

    fn lower_structlit(
        &mut self,
        ty: &AstTypeExpr,
        variant: Option<Symbol>,
        fields: &[AstStructField],
        scope: &Scope,
    ) -> HirExprDesc {
        if let Some(variant_name) = variant {
            let EnumRef { def: enum_def, args: template_hints } = self
                .as_enum_with_filled_holes(ty)
                .expect("StructLit with variant: could not resolve enum type");

            let lowered_fields: Vec<(Symbol, HirExpr)> = fields
                .iter()
                .map(|field| {
                    let value = self.lower_expr(&field.value, scope, self.module);
                    (field.name, value)
                })
                .collect();
            HirExprDesc::Constructor {
                enum_def,
                name: variant_name,
                args: HirConstructorArgs::StructLike { fields: lowered_fields },
                template_hints,
            }
        } else {
            HirExprDesc::StructLit {
                ty: self.resolve_holed(ty),
                fields: fields
                    .iter()
                    .map(|field| {
                        (field.name, self.lower_expr(&field.value, scope, self.module))
                    })
                    .collect(),
            }
        }
    }

    fn lower_tuple(&mut self, exprs: &[AstExpr], scope: &Scope) -> HirExprDesc {
        if exprs.len() == 1 {
            self.lower_expr(&exprs[0], scope, self.module).data
        } else {
            HirExprDesc::Tuple(
                exprs.iter().map(|e| self.lower_expr(e, scope, self.module)).collect(),
            )
        }
    }

    fn lower_qualified(&mut self, name: Symbol, ty: &AstTypeExpr) -> HirExprDesc {
        let EnumRef { def: enum_def, args: template_hints } = self
            .as_enum_with_filled_holes(ty)
            .expect("QualifiedPath with variant: could not resolve enum type");

        HirExprDesc::Constructor {
            enum_def,
            name,
            args: HirConstructorArgs::None,
            template_hints,
        }
    }

    fn lower_ref(
        &mut self,
        place: &AstExpr,
        mutable: bool,
        scope: &Scope,
    ) -> HirExprDesc {
        let place = self.expr_as_place(place, scope, self.module);
        HirExprDesc::Ref {
            place,
            mutability: if mutable { Mutability::Mutable } else { Mutability::Const },
        }
    }

    fn lower_expr(&mut self, expr: &AstExpr, scope: &Scope, module: ModuleId) -> HirExpr {
        let data = match &expr.data {
            AstExprDesc::IntLit(intlit) => HirExprDesc::IntLit(*intlit),
            AstExprDesc::CharLit(c) => HirExprDesc::CharLit(*c),
            AstExprDesc::StrLit(strlit) => HirExprDesc::StrLit(*strlit),
            AstExprDesc::CStrLit(strlit) => HirExprDesc::CStrLit(*strlit),
            AstExprDesc::BoolLit(boollit) => HirExprDesc::BoolLit(*boollit),
            AstExprDesc::Name(symbol) => {
                self.lower_name(*symbol, expr.span, scope, module)
            }
            AstExprDesc::NameResolved { from, to } => {
                self.lower_name_resolved(from.data, from.span, to, scope, module)
            }
            AstExprDesc::StaticCall { ty, method, args, type_args } => {
                self.lower_static_call(ty, *method, type_args, args, scope, module)
            }
            AstExprDesc::BinOp { lhs, op, rhs } => self.lower_binop(lhs, rhs, *op, scope),
            AstExprDesc::Range { from, to } => self.lower_range(from, to, scope),
            AstExprDesc::Neg(inner) => {
                HirExprDesc::Neg(self.lower_expr(inner, scope, self.module).boxed())
            }
            AstExprDesc::Not(inner) => {
                HirExprDesc::Not(self.lower_expr(inner, scope, self.module).boxed())
            }
            AstExprDesc::AddressOf(place) => HirExprDesc::AddressOf {
                place: self.expr_as_place(place, scope, module),
                mutability: Mutability::Const, // TODO: mut address-of
            },
            AstExprDesc::Call { callee, args, type_args } => {
                self.lower_call(callee, type_args, args, scope, module, expr.span)
            }
            AstExprDesc::MethodCall { object, method, args, type_args } => {
                self.lower_method_call(object, *method, type_args, args, scope, module)
            }
            AstExprDesc::StructLit { ty, variant, fields } => {
                self.lower_structlit(ty, *variant, fields, scope)
            }
            AstExprDesc::Tuple(exprs) => self.lower_tuple(exprs, scope),
            AstExprDesc::SliceLit(exprs) => HirExprDesc::SliceLit(
                exprs
                    .iter()
                    .map(|expr| self.lower_expr(expr, scope, self.module))
                    .collect(),
            ),
            AstExprDesc::SizeOf(ty) => HirExprDesc::SizeOf(self.resolve_holed(ty)),
            AstExprDesc::TypeName(ty) => HirExprDesc::TypeName(self.resolve_holed(ty)),
            AstExprDesc::QualifiedPath { ty, name } => self.lower_qualified(*name, ty),
            AstExprDesc::Ref(mutable, place) => self.lower_ref(place, *mutable, scope),
            AstExprDesc::Metadata(fat_ptr) => {
                let expr = self.lower_expr(fat_ptr, scope, self.module);
                HirExprDesc::Metadata(expr.boxed())
            }
            AstExprDesc::As { expr, ty } => {
                let expr = self.lower_expr(expr, scope, self.module);
                let ty = self.resolve_holed(ty);
                HirExprDesc::As { expr: expr.boxed(), ty }
            }
            _ => HirExprDesc::Use(self.expr_as_place(expr, scope, module)),
        };

        self.new_expr(data, expr.span, false)
    }

    fn lower_constructor_or_static_call(
        &mut self,
        type_def_id: TypeDefId,
        symbol: Symbol,
        type_args: Vec<TypeRef>,
        args: Vec<HirExpr>,
    ) -> HirExprDesc {
        let ty = self.instantiate_holed(type_def_id);
        if let TypeDefId::Enum(enum_id) = type_def_id {
            let items = enum_item(self.db, enum_id.interned());
            if items.variants.iter().any(|variant| {
                variant.name == symbol
                    && matches!(&variant.kind, AstEnumVariantKind::TupleLike(_))
            }) {
                return HirExprDesc::Constructor {
                    enum_def: enum_id,
                    name: symbol,
                    args: HirConstructorArgs::TupleLike(args),
                    template_hints: compute_template_hints(self.db, ty, type_args),
                };
            }
        }
        HirExprDesc::CallStatic { ty, method: symbol, args, type_args }
    }

    fn lower_name_resolved_from_type_call(
        &mut self,
        type_def_id: TypeDefId,
        callee: &AstExpr,
        type_args: &[AstAnyTypeExpr],
        args: &[AstExpr],
        scope: &Scope,
        module: ModuleId,
    ) -> HirExprDesc {
        let AstExprDesc::Name(method) = callee.data else { unreachable!() };
        let args = args.iter().map(|a| self.lower_expr(a, scope, module)).collect_vec();
        let type_args = type_args.iter().map(|arg| self.resolve_any(arg)).collect_vec();
        self.lower_constructor_or_static_call(type_def_id, method, type_args, args)
    }

    fn lower_structlit_that_is_actually_enum_variant(
        &mut self,
        type_def_id: TypeDefId,
        ty: &AstTypeExpr,
        variant: Option<Symbol>,
        fields: &[AstStructField],
        scope: &Scope,
    ) -> HirExprDesc {
        let Some(EnumRef { def: enum_def, args: template_hints }) =
            rehole(self.db, TypeRef::Concrete(TypeId::new(self.db, type_def_id, vec![])))
                .as_enum_ref(self.db)
        else {
            todo!("Push diagnostic for bad type here")
        };

        let variant_name = if let Some(v) = variant {
            v
        } else {
            match &ty.data {
                AstTypeExprDesc::Named { name, args } if args.is_empty() => *name,
                _ => unreachable!(
                    "NameResolved+StructLit with no variant and complex
        ty"
                ),
            }
        };
        let mut lowered_fields = vec![];
        for AstStructField { name, value } in fields {
            let expr = self.lower_expr(value, scope, self.module);
            lowered_fields.push((*name, expr));
        }
        let args = HirConstructorArgs::StructLike { fields: lowered_fields };

        HirExprDesc::Constructor { enum_def, name: variant_name, args, template_hints }
    }

    fn lower_name_resolved_expr_from_type(
        &mut self,
        scope: &Scope,
        module: ModuleId,
        to: &AstExpr,
        type_def_id: TypeDefId,
    ) -> HirExprDesc {
        match &to.data {
            AstExprDesc::Call { callee, args, type_args } => self
                .lower_name_resolved_from_type_call(
                    type_def_id,
                    callee,
                    type_args,
                    args,
                    scope,
                    module,
                ),
            AstExprDesc::StructLit { ty, variant, fields } => self
                .lower_structlit_that_is_actually_enum_variant(
                    type_def_id,
                    ty,
                    *variant,
                    fields,
                    scope,
                ),
            AstExprDesc::Name(variant) => match type_def_id {
                TypeDefId::Builtin(_builtin_type_id) => todo!(),
                TypeDefId::Struct(_struct_id) => todo!(),
                TypeDefId::Enum(enum_def) => {
                    let mut template_hints = vec![];
                    for _ in 0..templates_of_enum(self.db, enum_def.interned()).len() {
                        template_hints.push(TypeRef::Unknown);
                    }
                    HirExprDesc::Constructor {
                        enum_def,
                        name: *variant,
                        args: HirConstructorArgs::None,
                        template_hints,
                    }
                }
            },
            unhandled => {
                todo!("{}: {:?}", to.span.start().loc_info(self.db), unhandled)
            }
        }
    }

    /// TODO: see what's the difference with the similarly named
    ///  `lower_pattern_as_constructor_args`
    fn lower_constructor_pattern(
        &mut self,
        pat: &AstPattern,
        scope: &mut Scope,
        locals: &mut Vec<LocalId>,
        module: ModuleId,
        name: &Symbol,
        args: &AstConstructFields,
    ) -> HirPattern {
        let Some(resolution) = resolve_in_module(self.db, *name, module) else {
            Diag::generic_error(
                format!(
                    "Unresolved name {} in module {}",
                    name.to_string(self.db),
                    get_module_pretty_name(self.db, module.interned())
                ),
                pat.span,
            )
            .accumulate(self.db);
            return self.new_pattern(HirPatternDesc::Error, pat.span, false);
        };

        let Definition::Type(type_def) = resolution else { todo!() };

        match type_def {
            TypeDefId::Struct(struct_id) => match args {
                AstConstructFields::StructFields(struct_field_patterns) => {
                    let expected: HashSet<Symbol> =
                        struct_id.field_names(self.db).iter().copied().collect();
                    let bound: HashSet<Symbol> = struct_field_patterns
                        .iter()
                        .map(|pat| match pat {
                            StructFieldPattern::Rebind { name, .. } => *name,
                            StructFieldPattern::Name(symbol, _) => *symbol,
                        })
                        .collect();

                    if expected != bound {
                        let _ = diagnose_bad_struct_fields(
                            self.db, pat.span, struct_id, &bound,
                        );
                    }

                    let hir_fields = struct_field_patterns
                        .iter()
                        .map(|pat| match pat {
                            StructFieldPattern::Rebind { name, pattern, name_span } => {
                                let (lowered, new_locals) =
                                    self.lower_pattern(pattern, scope);
                                locals.extend(new_locals);

                                HirStructFieldPattern::Rebind {
                                    name: *name,
                                    pattern: lowered,
                                    name_span: *name_span,
                                }
                            }
                            StructFieldPattern::Name(name, name_span) => {
                                let local = self.allocate_local(
                                    scope,
                                    *name,
                                    Mutability::Const,
                                    None,
                                    *name_span,
                                    false,
                                );
                                locals.push(local);
                                HirStructFieldPattern::Name {
                                    id: local,
                                    name: *name,
                                    span: *name_span,
                                }
                            }
                        })
                        .collect_vec();
                    self.new_pattern(
                        HirPatternDesc::DestructureBinding {
                            resolution: struct_id,
                            fields: hir_fields,
                        },
                        pat.span,
                        false,
                    )
                }
                _ => todo!(),
            },
            TypeDefId::Enum(_enum_id) => match args {
                AstConstructFields::TupleFields(_spanneds) => todo!(),
                AstConstructFields::StructFields(_struct_field_patterns) => {
                    todo!()
                }
            },
            _ => todo!(),
        }
    }

    fn lower_pattern(
        &mut self,
        pat: &AstPattern,
        scope: &mut Scope,
    ) -> (HirPattern, Vec<LocalId>) {
        fn _lower(
            this: &mut LowerFundef<'_>,
            pat: &AstPattern,
            scope: &mut Scope,
            locals: &mut Vec<LocalId>,
            module: ModuleId,
        ) -> HirPattern {
            match &pat.data {
                AstPatternDesc::Named(ast) => match ast {
                    AstNamedPattern::Mut { name } => {
                        let local_id = this.allocate_local(
                            scope,
                            *name,
                            Mutability::Mutable,
                            None,
                            pat.span,
                            false,
                        );
                        locals.push(local_id);
                        HirPattern {
                            id: this.alloc.fresh(),
                            data: HirPatternDesc::Bind {
                                id: local_id,
                                name: *name,
                                mutable: true,
                            },
                            span: pat.span,
                            is_synthetic: false,
                        }
                    }
                    AstNamedPattern::Bare(name) => {
                        let local_id = this.allocate_local(
                            scope,
                            *name,
                            Mutability::Const,
                            None,
                            pat.span,
                            false,
                        );
                        locals.push(local_id);
                        HirPattern {
                            id: this.alloc.fresh(),
                            data: HirPatternDesc::Bind {
                                id: local_id,
                                name: *name,
                                mutable: false,
                            },
                            span: pat.span,
                            is_synthetic: false,
                        }
                    }
                    AstNamedPattern::Constructor { name, args } => this
                        .lower_constructor_pattern(
                            pat, scope, locals, module, name, args,
                        ),
                    AstNamedPattern::NameResolved { from, to } => {
                        match resolve_in_module(this.db, *from, module) {
                            Some(Definition::Module(id)) => {
                                let inner_pat = AstPattern::new(
                                    AstPatternDesc::Named(*to.clone()),
                                    vec![],
                                    pat.span,
                                );
                                _lower(this, &inner_pat, scope, locals, id)
                            }
                            Some(Definition::Type(TypeDefId::Enum(resolution))) => {
                                let (name, fields) = this
                                    .lower_pattern_as_constructor_args(
                                        to.as_ref(),
                                        pat.span,
                                        scope,
                                        locals,
                                    );

                                HirPattern {
                                    id: this.alloc.fresh(),
                                    data: HirPatternDesc::Constructor {
                                        resolution,
                                        name,
                                        fields,
                                    },
                                    span: pat.span,
                                    is_synthetic: false,
                                }
                            }
                            Some(_) => todo!(
                                "error: `{}` in pattern is not a module",
                                from.interned().contents(this.db)
                            ),
                            None => todo!(
                                "error: `{}` not found in pattern path",
                                from.interned().contents(this.db)
                            ),
                        }
                    }
                    AstNamedPattern::Tuple { fields } => HirPattern {
                        id: this.alloc.fresh(),
                        data: HirPatternDesc::Tuple(
                            fields
                                .iter()
                                .map(|x| _lower(this, x, scope, locals, this.module))
                                .collect(),
                        ),
                        span: pat.span,
                        is_synthetic: false,
                    },
                },
                AstPatternDesc::Any => HirPattern {
                    id: this.alloc.fresh(),
                    data: HirPatternDesc::Any,
                    span: pat.span,
                    is_synthetic: false,
                },
                AstPatternDesc::IntLiteral(x) => HirPattern {
                    id: this.alloc.fresh(),
                    data: HirPatternDesc::IntLit(*x),
                    span: pat.span,
                    is_synthetic: false,
                },
                AstPatternDesc::Error(_) => HirPattern {
                    id: this.alloc.fresh(),
                    data: HirPatternDesc::Error,
                    span: pat.span,
                    is_synthetic: false,
                },
            }
        }

        let mut v = vec![];
        let pat = _lower(self, pat, scope, &mut v, self.module);
        (pat, v)
    }

    fn lower_stmt(&mut self, stmt: &AstStmt, scope: &mut Scope) -> HirStmt {
        let kind = match &stmt.data {
            AstStmtDesc::Return { value } => {
                let value =
                    value.as_ref().map(|expr| self.lower_expr(expr, scope, self.module));
                HirStmtKind::Return(value)
            }
            AstStmtDesc::If { cond, then, else_ } => {
                let cond = self.lower_expr(cond, scope, self.module);
                let then = self.lower_stmt(then, scope);
                let else_ = else_.as_ref().map(|s| self.lower_stmt(s, scope).boxed());
                HirStmtKind::If { cond, then: then.boxed(), else_ }
            }
            AstStmtDesc::While { cond, body } => {
                let cond = self.lower_expr(cond, scope, self.module);
                let body = self.lower_stmt(body, scope);
                HirStmtKind::While { cond, body: body.boxed() }
            }
            AstStmtDesc::For { element, iterator, body } => {
                self.desugar_for_loop(scope, element, iterator, body)
            }
            AstStmtDesc::LetDecl { pat, type_constraint, value } => {
                let init = self.lower_expr(value, scope, self.module);
                let (pat, locals) = self.lower_pattern(pat, scope);
                HirStmtKind::Let {
                    pattern: pat,
                    locals,
                    ty_annotation: type_constraint.clone(),
                    init,
                }
            }
            AstStmtDesc::Block { stmts } => {
                let mut block_scope = scope.clone();
                HirStmtKind::Block(
                    stmts.iter().map(|s| self.lower_stmt(s, &mut block_scope)).collect(),
                )
            }
            AstStmtDesc::Assign { lhs, rhs } => {
                let place = self.expr_as_place(lhs, scope, self.module);
                let rhs = self.lower_expr(rhs, scope, self.module);
                HirStmtKind::Assign { lhs: place, rhs }
            }
            AstStmtDesc::CompoundAssign { lhs, op, rhs } => {
                // Make sure not to lower the lhs twice !
                let place = self.expr_as_place(lhs, scope, self.module);
                let binop = op.to_binop();
                let lhs_expr =
                    self.new_expr(HirExprDesc::Use(place.clone()), lhs.span, true);
                let rhs_expr = self.lower_expr(rhs, scope, self.module);
                let combined = self.new_expr(
                    HirExprDesc::BinOp {
                        lhs: lhs_expr.boxed(),
                        op: binop,
                        rhs: rhs_expr.boxed(),
                    },
                    stmt.span,
                    true,
                );
                HirStmtKind::Assign { lhs: place, rhs: combined }
            }
            AstStmtDesc::Expr(expr) => {
                let expr = self.lower_expr(expr, scope, self.module);
                HirStmtKind::Expr(expr)
            }
            AstStmtDesc::Match { scrutinee, branches } => {
                let scrutinee = self.lower_expr(scrutinee, scope, self.module);

                let branches = branches
                    .iter()
                    .map(|AstMatchBranch { pat, guard, body }| {
                        let mut branch_scope = scope.clone();
                        let (pattern, locals) =
                            self.lower_pattern(pat, &mut branch_scope);
                        let guard = guard.as_ref().map(|expr| {
                            self.lower_expr(expr, &branch_scope, self.module)
                        });
                        let body = self.lower_stmt(body, &mut branch_scope);
                        HirMatchBranch { pattern, locals, guard, body: body.boxed() }
                    })
                    .collect();
                HirStmtKind::Match { scrutinee, branches }
            }
            AstStmtDesc::Defer(stmt) => {
                HirStmtKind::Defer(self.lower_stmt(stmt, scope).boxed())
            }
            AstStmtDesc::Break => HirStmtKind::Break,
            AstStmtDesc::Error(_) => HirStmtKind::Error,
        };
        self.new_stmt(kind, stmt.span, false)
    }

    fn desugar_for_loop(
        &mut self,
        scope: &mut Scope,
        element: &AstPattern,
        iterator: &AstExpr,
        body: &AstStmt,
    ) -> HirStmtKind {
        // for pat in iterator {...}
        // becomes
        // let mut iterator = IntoIterator::into_iter(iterator);
        // while true {
        //     match iterator.next() {
        //         Some(pat) => {...}
        //         _ => { break; }
        //     }
        // }

        let iterator_span = iterator.span;
        let iter_interface = core_iter_interface(self.db);
        let into_iter_interface = core_into_iterator_interface(self.db);
        let iterator_candidate = self.lower_expr(iterator, scope, self.module);
        let iterator = self.new_expr(
            HirExprDesc::CallMethod {
                receiver: iterator_candidate.boxed(),
                method: Symbol::new(self.db, "into_iter"),
                args: vec![],
                interface_hint: Some(*into_iter_interface),
                type_args: vec![],
            },
            iterator_span,
            true,
        );
        let iterator_id = self.allocate_local(
            scope,
            Symbol::new(self.db, format!("@iterator_{}", self.next_iterator_id())),
            Mutability::Mutable,
            None,
            iterator_span,
            true,
        );
        let let_iter = self.declare_single_var(iterator_id, iterator, iterator_span);
        let iterator_place =
            self.new_place(HirPlaceKind::Local(iterator_id), iterator_span, true);
        let next_expr = self.new_expr(
            HirExprDesc::CallMethod {
                receiver: self
                    .new_expr(HirExprDesc::Use(iterator_place), iterator_span, true)
                    .boxed(),

                method: Symbol::new(self.db, "next"),
                args: vec![],
                interface_hint: Some(*iter_interface),
                type_args: vec![],
            },
            iterator_span,
            true,
        );

        let mut iterator_scope = scope.clone();

        let pat = self.lower_pattern(element, &mut iterator_scope);

        let iterator_body = self.lower_stmt(body, &mut iterator_scope);

        let while_body =
            self.match_some_do_or_break(next_expr, pat.0, pat.1, iterator_body);

        let while_true_loop = self.while_true_do(while_body, iterator_span, true);
        HirStmtKind::Block(vec![let_iter, while_true_loop])
    }

    fn next_iterator_id(&self) -> usize {
        self.iterator_id.fresh()
    }

    fn collect_args(&mut self, args: &[AstFundefArg], scope: &mut Scope) -> Vec<LocalId> {
        args.iter()
            .map(|arg| {
                self.allocate_local(
                    scope,
                    arg.name,
                    Mutability::Const, // TODO: be able to change that
                    Some(arg.ty.clone().into()),
                    arg.span,
                    false,
                )
            })
            .collect()
    }

    fn lower(&mut self, ast: &AstFundef) -> HirBody<'db> {
        let mut s = Scope::new();
        let params = self.collect_args(&ast.data.args, &mut s);
        let stmts = ast
            .data
            .body
            .iter()
            .map(|stmt| self.lower_stmt(stmt, &mut s))
            .collect::<Vec<_>>();
        HirBody::new(
            self.db,
            self.function,
            params,
            std::mem::take(&mut self.locals).into_values().collect::<Vec<_>>(),
            None,
            stmts,
        )
    }

    fn lower_method(&mut self, ast: &AstMethodDef) -> HirBody<'db> {
        let mut s = Scope::new();
        let mut zelf = None;
        if let Some((mutability, span)) = match &ast.data.receiver {
            AstReceiver::None => None,
            AstReceiver::Zelf(span)
            | AstReceiver::RefZelf(span)
            | AstReceiver::PtrZelf(span) => Some((Mutability::Const, span)),
            AstReceiver::MutZelf(span)
            | AstReceiver::MutRefZelf(span)
            | AstReceiver::MutPtrZelf(span) => Some((Mutability::Mutable, span)),
        } {
            zelf = Some(self.allocate_local(
                &mut s,
                Symbol::new(self.db, "self"),
                mutability,
                None,
                *span,
                false,
            ));
        }
        let params = self.collect_args(&ast.data.args, &mut s);
        let stmts = ast
            .data
            .body
            .iter()
            .map(|stmt| self.lower_stmt(stmt, &mut s))
            .collect::<Vec<_>>();

        HirBody::new(
            self.db,
            self.function,
            params,
            std::mem::take(&mut self.locals).into_values().collect::<Vec<_>>(),
            zelf,
            stmts,
        )
    }

    fn lower_pattern_as_constructor_args(
        &mut self,
        pat: &AstNamedPattern,
        span: Span,
        scope: &mut Scope,
        locals: &mut Vec<LocalId>,
    ) -> (Symbol, HirPatternConstructorArgs) {
        match pat {
            AstNamedPattern::Constructor { name, args } => {
                let args = match args {
                    AstConstructFields::TupleFields(pats) => {
                        let mut lowered_pats = vec![];
                        for pat in pats {
                            let (pat, new_locals) = self.lower_pattern(pat, scope);
                            lowered_pats.push(pat);
                            locals.extend(new_locals);
                        }
                        HirPatternConstructorArgs::TupleFields(lowered_pats)
                    }
                    AstConstructFields::StructFields(pats) => {
                        let mut lowered_pats = vec![];
                        for pat in pats {
                            let pat = match pat {
                                StructFieldPattern::Rebind {
                                    name,
                                    pattern,
                                    name_span,
                                } => {
                                    let (pat, new_locals) =
                                        self.lower_pattern(pattern, scope);
                                    locals.extend(new_locals);
                                    HirStructFieldPattern::Rebind {
                                        name: *name,
                                        pattern: pat,
                                        name_span: *name_span,
                                    }
                                }
                                StructFieldPattern::Name(symbol, name_span) => {
                                    let new_local = self.allocate_local(
                                        scope,
                                        *symbol,
                                        Mutability::Const,
                                        None,
                                        span,
                                        false,
                                    );
                                    locals.push(new_local);
                                    HirStructFieldPattern::Name {
                                        name: *symbol,
                                        id: new_local,
                                        span: *name_span,
                                    }
                                }
                            };
                            lowered_pats.push(pat);
                        }
                        HirPatternConstructorArgs::StructFields(lowered_pats)
                    }
                };
                (*name, args)
            }
            AstNamedPattern::NameResolved { .. } => todo!(),
            AstNamedPattern::Tuple { .. } => {
                unreachable!()
            }
            AstNamedPattern::Mut { .. } => unreachable!(),
            AstNamedPattern::Bare(name) => (*name, HirPatternConstructorArgs::None),
        }
    }
}

pub(super) fn lower_fundef_body<'db>(
    db: &'db dyn Db,
    function: FunctionId,
    ast: &'db AstFundef,
) -> HirBody<'db> {
    let module = match function.parent(db) {
        ScopeOwnerId::Module(m) => m,
        ScopeOwnerId::Impl(impl_id) => impl_id.parent(db),
        ScopeOwnerId::Interface(interface_ref) => interface_ref.def(db).parent(db),
    };
    let template_args = get_templates_of_fun(db, function.into()).to_vec();
    let mut ctx = LowerFundef::new(db, function, module, template_args);
    ctx.lower(ast)
}

pub(super) fn lower_method_body<'db>(
    db: &'db dyn Db,
    function: FunctionId,
    ast: &'db AstMethodDef,
) -> HirBody<'db> {
    let module = match function.parent(db) {
        ScopeOwnerId::Module(m) => m,
        ScopeOwnerId::Impl(impl_id) => impl_id.parent(db),
        ScopeOwnerId::Interface(interface_ref) => interface_ref.def(db).parent(db),
    };
    let template_args = get_templates_of_fun(db, function.into()).to_vec();
    let mut ctx = LowerFundef::new(db, function, module, template_args);
    ctx.lower_method(ast)
}
