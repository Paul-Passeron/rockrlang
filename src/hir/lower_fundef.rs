use std::{collections::HashMap, iter};

use crate::{
    Db,
    common::{location::Span, symbols::Symbol},
    hir::{
        HirBody, HirConstructorArgs, HirExpr, HirExprDesc, HirIdAlloc, HirMatchBranch, HirPattern,
        HirPatternConstructorArgs, HirPatternDesc, HirPlace, HirStmt, HirStmtKind,
        HirStructFieldPattern, LocalId, LocalInfo, Mutability, PartialTypeArg, PartialTypeRef,
    },
    name_resolve::{
        definition::{Definition, resolve_in_module},
        interfaces::{core_int_iter_struct, core_into_iterator_interface, core_iter_interface},
        type_expr::{templates_of_enum, templates_of_struct},
    },
    parse_tree::{
        expr::{AstExpr, AstExprDesc, AstStructField},
        pattern::{
            AstConstructFields, AstNamedPattern, AstPattern, AstPatternDesc, StructFieldPattern,
        },
        stmt::{AstMatchBranch, AstStmt, AstStmtDesc},
        top_level::{AstFundef, AstFundefArg, AstTemplateArg},
        type_expr::{AstAnyTypeExpr, AstAnyTypeExprDesc, AstTypeExpr, AstTypeExprDesc},
    },
    ril::{
        BuiltinTypeId, FunctionId, ModuleId, ScopeOwnerId, TypeDefId, TypeId, TypeParamId, TypeRef,
        get_template_param_count,
    },
};

struct LowerFundef<'db> {
    db: &'db dyn Db,
    function: FunctionId,
    module: ModuleId,
    template_args: Vec<AstTemplateArg>,
    next_local_id: u32,
    alloc: HirIdAlloc,
    locals: HashMap<LocalId, LocalInfo>,
}

#[derive(Debug, Clone)]
pub struct Scope {
    pub map: HashMap<Symbol, LocalId>,
}

impl Scope {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
}

impl PartialTypeRef {
    pub fn to_partial_arg(self) -> PartialTypeArg {
        match &self {
            PartialTypeRef::Resolved(type_ref) => PartialTypeArg::Known(*type_ref),
            _ => PartialTypeArg::Partial(Box::new(self)),
        }
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
            next_local_id: 0,
            alloc: HirIdAlloc::new(),
            locals: HashMap::new(),
        }
    }

    pub fn allocate_local(
        &mut self,
        scope: &mut Scope,
        name: Symbol,
        mutability: Mutability,
        ty_annotation: Option<AstAnyTypeExpr>,
        span: Span,
    ) -> LocalId {
        let id = LocalId(self.next_local_id);
        self.next_local_id += 1;
        let info = LocalInfo {
            id,
            name,
            mutability,
            ty_annotation,
            span,
        };
        self.locals.insert(id, info);
        scope.map.insert(name, id);
        id
    }

    fn expr_as_place(&mut self, expr: &AstExpr, scope: &Scope, module: ModuleId) -> HirPlace {
        println!("Calling place with {:?}", expr.data);

        match &expr.data {
            AstExprDesc::Name(symbol) => {
                if let Some(id) = scope.map.get(symbol) {
                    HirPlace::Local(*id)
                } else {
                    todo!(
                        "expr_as_place: Name `{}` not found in local scope",
                        symbol.interned().contents(self.db)
                    )
                }
            }
            AstExprDesc::NameResolved { from, to } => {
                match resolve_in_module(self.db, from.interned(), module.interned()) {
                    Some(Definition::Module(inner)) => self.expr_as_place(to, scope, inner),
                    _ => {
                        let temp = self.lower_expr(expr, scope, module);
                        HirPlace::Temporary(Box::new(temp))
                    }
                }
            }
            AstExprDesc::PostfixDeref(inner) => {
                HirPlace::Deref(Box::new(self.expr_as_place(inner, scope, module)))
            }
            AstExprDesc::FieldAccess { object, field } => {
                let base = self.expr_as_place(object, scope, module);
                HirPlace::Field {
                    base: Box::new(base),
                    field: *field,
                }
            }
            AstExprDesc::TupleAccess { object, index } => {
                let base = self.expr_as_place(object, scope, module);
                HirPlace::TupleField {
                    base: Box::new(base),
                    index: *index,
                }
            }
            AstExprDesc::Index { object, index } => {
                let base = self.expr_as_place(object, scope, module);
                let index = self.lower_expr(index, scope, self.module);
                HirPlace::Index {
                    base: Box::new(base),
                    index: Box::new(index),
                }
            }
            _ => {
                let temp = self.lower_expr(expr, scope, module);
                HirPlace::Temporary(Box::new(temp))
            }
        }
    }

    fn instantiate_holed(&self, ty: TypeDefId) -> PartialTypeRef {
        let n = get_template_param_count(self.db, ty);
        if n == 0 {
            PartialTypeRef::Resolved(TypeRef::Concrete(TypeId::new(self.db, ty, vec![])))
        } else {
            PartialTypeRef::WithHoles {
                def: ty,
                args: iter::repeat_n(PartialTypeArg::Infer, n).collect(),
            }
        }
    }

    fn resolve_any_holed_arg(&self, any_ty: &AstAnyTypeExpr, module: ModuleId) -> PartialTypeArg {
        match &any_ty.data {
            AstAnyTypeExprDesc::Any => PartialTypeArg::Infer,
            AstAnyTypeExprDesc::Known(desc) => {
                self.resolve_holed_desc(desc, module).to_partial_arg()
            }
        }
    }

    fn resolve_holed_desc(&self, desc: &AstTypeExprDesc, module: ModuleId) -> PartialTypeRef {
        match desc {
            AstTypeExprDesc::Named { name, args } => {
                if args.is_empty() {
                    if let Some(idx) = self.template_args.iter().position(|p| p.name == *name) {
                        return PartialTypeRef::Resolved(TypeRef::Param(TypeParamId(idx)));
                    }
                }

                let Some(Definition::Type(type_def_id)) =
                    resolve_in_module(self.db, name.interned(), module.interned())
                else {
                    panic!("todo")
                };

                if args.is_empty() {
                    return self.instantiate_holed(type_def_id);
                }

                let partial_args: Vec<PartialTypeArg> = args
                    .iter()
                    .map(|arg| self.resolve_any_holed_arg(arg, module))
                    .collect();

                let all_known: Option<Vec<TypeRef>> = partial_args
                    .iter()
                    .map(|a| match a {
                        PartialTypeArg::Known(t) => Some(*t),
                        _ => None,
                    })
                    .collect();

                match all_known {
                    Some(resolved) => PartialTypeRef::Resolved(TypeRef::Concrete(TypeId::new(
                        self.db,
                        type_def_id,
                        resolved,
                    ))),
                    None => PartialTypeRef::WithHoles {
                        def: type_def_id,
                        args: partial_args,
                    },
                }
            }

            AstTypeExprDesc::NameResolved { from, to } => {
                match resolve_in_module(self.db, from.interned(), module.interned()) {
                    Some(Definition::Module(inner_module)) => {
                        self.resolve_holed_ty(to, inner_module)
                    }
                    _ => todo!(""),
                }
            }

            AstTypeExprDesc::Pointer { mutable, pointee } => {
                let inner = self.resolve_holed_ty(pointee, module);
                self.wrap_builtin_unary(
                    if *mutable {
                        BuiltinTypeId::mut_ptr(self.db)
                    } else {
                        BuiltinTypeId::ptr(self.db)
                    },
                    inner,
                )
            }

            AstTypeExprDesc::Ref { mutable, pointee } => {
                let inner = self.resolve_holed_ty(pointee, module);
                self.wrap_builtin_unary(
                    if *mutable {
                        BuiltinTypeId::mut_ref(self.db)
                    } else {
                        BuiltinTypeId::ref_(self.db)
                    },
                    inner,
                )
            }

            AstTypeExprDesc::Slice { ty, len: _ } => {
                let inner = self.resolve_holed_ty(ty, module);
                self.wrap_builtin_unary(BuiltinTypeId::slice(self.db), inner)
            }

            AstTypeExprDesc::Tuple(tys) => {
                let partial_args: Vec<PartialTypeArg> = tys
                    .iter()
                    .map(|ty| self.resolve_holed_ty(ty, module).to_partial_arg())
                    .collect();

                let tuple_def: TypeDefId = BuiltinTypeId::tuple(self.db).into();

                let all_known: Option<Vec<TypeRef>> = partial_args
                    .iter()
                    .map(|a| match a {
                        PartialTypeArg::Known(t) => Some(*t),
                        _ => None,
                    })
                    .collect();

                match all_known {
                    Some(resolved) => PartialTypeRef::Resolved(TypeRef::Concrete(TypeId::new(
                        self.db, tuple_def, resolved,
                    ))),
                    None => PartialTypeRef::WithHoles {
                        def: tuple_def,
                        args: partial_args,
                    },
                }
            }
        }
    }

    fn resolve_holed_ty(&self, ty: &AstTypeExpr, module: ModuleId) -> PartialTypeRef {
        self.resolve_holed_desc(&ty.data, module)
    }

    fn resolve_holed(&self, ty: &AstTypeExpr) -> PartialTypeRef {
        self.resolve_holed_ty(ty, self.module)
    }

    fn wrap_builtin_unary(&self, builtin: BuiltinTypeId, inner: PartialTypeRef) -> PartialTypeRef {
        let def: TypeDefId = builtin.into();
        let arg = inner.to_partial_arg();
        match &arg {
            PartialTypeArg::Known(t) => {
                PartialTypeRef::Resolved(TypeRef::Concrete(TypeId::new(self.db, def, vec![*t])))
            }
            PartialTypeArg::Infer => PartialTypeRef::WithHoles {
                def,
                args: vec![PartialTypeArg::Infer],
            },
            PartialTypeArg::Partial(partial) => *partial.clone(),
        }
    }

    fn lower_expr(&mut self, expr: &AstExpr, scope: &Scope, module: ModuleId) -> HirExpr {
        println!("Calling with {:?}", expr.data);
        HirExpr {
            id: self.alloc.next(),
            data: match &expr.data {
                AstExprDesc::IntLit(intlit) => HirExprDesc::IntLit(*intlit as i64),
                AstExprDesc::CharLit(c) => HirExprDesc::CharLit(*c),
                AstExprDesc::StrLit(strlit) => HirExprDesc::StrLit(*strlit),
                AstExprDesc::CStrLit(strlit) => HirExprDesc::CStrLit(*strlit),
                AstExprDesc::BoolLit(boollit) => HirExprDesc::BoolLit(*boollit),

                AstExprDesc::Name(symbol) => {
                    if let Some(id) = scope.map.get(symbol) {
                        HirExprDesc::Use(HirPlace::Local(*id))
                    } else {
                        match resolve_in_module(self.db, symbol.interned(), module.interned()) {
                            Some(Definition::Function(_)) => {
                                todo!(
                                    "bare function name `{}` used as value expression",
                                    symbol.interned().contents(self.db)
                                )
                            }
                            _ => todo!(
                                "Name `{}` not found in scope or module",
                                symbol.interned().contents(self.db)
                            ),
                        }
                    }
                }

                AstExprDesc::NameResolved { from, to } => {
                    match resolve_in_module(self.db, from.interned(), module.interned()) {
                        Some(Definition::Module(module_id)) => {
                            self.lower_expr(to, scope, module_id).data
                        }
                        Some(Definition::Type(type_def_id)) => match &to.data {
                            AstExprDesc::Call { callee, args } => {
                                let ty = self.instantiate_holed(type_def_id);
                                let AstExprDesc::Name(method) = callee.data else {
                                    unreachable!()
                                };
                                let args = args
                                    .iter()
                                    .map(|a| self.lower_expr(a, scope, module))
                                    .collect();
                                HirExprDesc::CallStatic { ty, method, args }
                            }
                            AstExprDesc::StructLit {
                                ty,
                                variant,
                                fields,
                            } => match type_def_id {
                                TypeDefId::Builtin(_builtin_type_id) => todo!(),
                                TypeDefId::Struct(_struct_id) => todo!(),
                                TypeDefId::Enum(enum_def) => {
                                    let variant_name = if let Some(v) = variant {
                                        *v
                                    } else {
                                        match &ty.data {
                                            AstTypeExprDesc::Named { name, args }
                                                if args.is_empty() =>
                                            {
                                                *name
                                            }
                                            _ => unreachable!(
                                                "NameResolved+StructLit with no variant and complex ty"
                                            ),
                                        }
                                    };
                                    let mut lowered_fields = vec![];
                                    for AstStructField { name, value } in fields {
                                        let expr = self.lower_expr(value, scope, self.module);
                                        lowered_fields.push((*name, expr));
                                    }
                                    let args = HirConstructorArgs::StructLike {
                                        fields: lowered_fields,
                                    };
                                    let mut template_hints = vec![];
                                    for _ in
                                        0..templates_of_enum(self.db, enum_def.interned()).len()
                                    {
                                        template_hints.push(PartialTypeArg::Infer);
                                    }
                                    HirExprDesc::Constructor {
                                        enum_def,
                                        name: variant_name,
                                        args,
                                        template_hints,
                                    }
                                }
                            },
                            _ => todo!(),
                        },
                        _ => todo!(
                            "error: badly name-resolved item ({})",
                            from.interned().contents(self.db)
                        ),
                    }
                }

                AstExprDesc::StaticCall { ty, method, args } => {
                    let ty = self.resolve_holed(ty);
                    let args = args
                        .iter()
                        .map(|a| self.lower_expr(a, scope, module))
                        .collect();
                    HirExprDesc::CallStatic {
                        ty,
                        method: *method,
                        args,
                    }
                }

                AstExprDesc::BinOp { lhs, op, rhs } => {
                    let lhs = self.lower_expr(lhs, scope, self.module);
                    let rhs = self.lower_expr(rhs, scope, self.module);
                    HirExprDesc::BinOp {
                        lhs: Box::new(lhs),
                        op: *op,
                        rhs: Box::new(rhs),
                    }
                }

                AstExprDesc::Range { from, to } => {
                    let from = self.lower_expr(from, scope, self.module);
                    let to = self.lower_expr(to, scope, self.module);

                    let int_iter = core_int_iter_struct(self.db);
                    let type_ref = TypeRef::Concrete(TypeId::new(
                        self.db,
                        TypeDefId::Struct(int_iter),
                        vec![],
                    ));
                    HirExprDesc::StructLit {
                        ty: PartialTypeRef::Resolved(type_ref),
                        fields: vec![
                            (Symbol::new(self.db, "start"), from),
                            (Symbol::new(self.db, "end"), to),
                        ],
                    }
                }

                AstExprDesc::Neg(inner) => {
                    HirExprDesc::Neg(Box::new(self.lower_expr(inner, scope, self.module)))
                }
                AstExprDesc::Not(inner) => {
                    HirExprDesc::Not(Box::new(self.lower_expr(inner, scope, self.module)))
                }

                AstExprDesc::AddressOf(place) => HirExprDesc::AddressOf {
                    place: self.expr_as_place(place, scope, module),
                    mutability: Mutability::Immutable, // TODO: mut address-of
                },

                AstExprDesc::Call { callee, args } => {
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
                            // Module-level: must be a free function
                            match resolve_in_module(self.db, symbol.interned(), module.interned()) {
                                Some(Definition::Function(fid)) => {
                                    let args = args
                                        .iter()
                                        .map(|a| self.lower_expr(a, scope, module))
                                        .collect();
                                    HirExprDesc::CallDirect { target: fid, args }
                                }
                                other => todo!(
                                    "Call callee `{}` resolved to {:?}",
                                    symbol.interned().contents(self.db),
                                    other
                                ),
                            }
                        }
                        AstExprDesc::NameResolved { from, to } => {
                            let resolved_call = AstExpr::new(
                                AstExprDesc::NameResolved {
                                    from: *from,
                                    to: Box::new(AstExpr::new(
                                        AstExprDesc::Call {
                                            callee: to.clone(),
                                            args: args.clone(),
                                        },
                                        vec![],
                                        expr.span.clone(),
                                    )),
                                },
                                vec![],
                                expr.span.clone(),
                            );
                            self.lower_expr(&resolved_call, scope, module).data
                        }
                        _ => {
                            todo!("Call with complex callee expression")
                        }
                    }
                }

                AstExprDesc::MethodCall {
                    object,
                    method,
                    args,
                } => HirExprDesc::CallMethod {
                    receiver: Box::new(self.lower_expr(object, scope, module)),
                    method: *method,
                    args: args
                        .iter()
                        .map(|arg| self.lower_expr(arg, scope, module))
                        .collect(),
                    interface_hint: None,
                },

                AstExprDesc::StructLit {
                    ty,
                    variant,
                    fields,
                } => {
                    if let Some(variant_name) = variant {
                        let partial_ty = self.resolve_holed(ty);
                        let enum_def = match &partial_ty {
                            PartialTypeRef::Resolved(TypeRef::Concrete(type_id)) => {
                                match type_id.def(self.db) {
                                    TypeDefId::Enum(e) => e,
                                    _ => panic!("StructLit with variant on non-enum type"),
                                }
                            }
                            PartialTypeRef::WithHoles { def, .. } => match def {
                                TypeDefId::Enum(e) => *e,
                                _ => panic!("StructLit with variant on non-enum type"),
                            },
                            _ => panic!("StructLit with variant: could not resolve enum type"),
                        };
                        let template_hints = match &partial_ty {
                            PartialTypeRef::Resolved(TypeRef::Concrete(type_id)) => type_id
                                .args(self.db)
                                .iter()
                                .map(|a| PartialTypeArg::Known(*a))
                                .collect(),
                            PartialTypeRef::WithHoles { args, .. } => args.clone(),
                            _ => {
                                let n = templates_of_enum(self.db, enum_def.interned()).len();
                                iter::repeat_n(PartialTypeArg::Infer, n).collect()
                            }
                        };
                        let lowered_fields: Vec<(Symbol, HirExpr)> = fields
                            .iter()
                            .map(|field| {
                                (
                                    field.name,
                                    self.lower_expr(&field.value, scope, self.module),
                                )
                            })
                            .collect();
                        HirExprDesc::Constructor {
                            enum_def,
                            name: *variant_name,
                            args: HirConstructorArgs::StructLike {
                                fields: lowered_fields,
                            },
                            template_hints,
                        }
                    } else {
                        HirExprDesc::StructLit {
                            ty: self.resolve_holed(ty),
                            fields: fields
                                .iter()
                                .map(|field| {
                                    (
                                        field.name,
                                        self.lower_expr(&field.value, scope, self.module),
                                    )
                                })
                                .collect(),
                        }
                    }
                }
                AstExprDesc::Tuple(exprs) => HirExprDesc::Tuple(
                    exprs
                        .iter()
                        .map(|e| self.lower_expr(e, scope, self.module))
                        .collect(),
                ),
                AstExprDesc::SliceLit(exprs) => HirExprDesc::SliceLit(
                    exprs
                        .iter()
                        .map(|expr| self.lower_expr(expr, scope, self.module))
                        .collect(),
                ),
                AstExprDesc::SizeOf(ty) => HirExprDesc::SizeOf(self.resolve_holed(ty)),
                AstExprDesc::QualifiedPath { ty, name } => {
                    let partial_ty = self.resolve_holed(ty);
                    let enum_def = match &partial_ty {
                        PartialTypeRef::Resolved(TypeRef::Concrete(type_id)) => {
                            match type_id.def(self.db) {
                                TypeDefId::Enum(e) => e,
                                _ => panic!("QualifiedPath on non-enum type"),
                            }
                        }
                        PartialTypeRef::WithHoles { def, .. } => match def {
                            TypeDefId::Enum(e) => *e,
                            _ => panic!("QualifiedPath on non-enum type"),
                        },
                        _ => panic!("QualifiedPath: could not resolve enum type"),
                    };
                    let template_hints = match &partial_ty {
                        PartialTypeRef::Resolved(TypeRef::Concrete(type_id)) => type_id
                            .args(self.db)
                            .iter()
                            .map(|a| PartialTypeArg::Known(*a))
                            .collect(),
                        PartialTypeRef::WithHoles { args, .. } => args.clone(),
                        _ => {
                            let n = templates_of_enum(self.db, enum_def.interned()).len();
                            iter::repeat_n(PartialTypeArg::Infer, n).collect()
                        }
                    };
                    HirExprDesc::Constructor {
                        enum_def,
                        name: *name,
                        args: HirConstructorArgs::None,
                        template_hints,
                    }
                }
                _ => HirExprDesc::Use(self.expr_as_place(expr, scope, module)),
            },
            span: expr.span.clone(),
        }
    }

    fn lower_pattern(&mut self, pat: &AstPattern, scope: &mut Scope) -> (HirPattern, Vec<LocalId>) {
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
                            pat.span.clone(),
                        );
                        locals.push(local_id);
                        HirPattern {
                            id: this.alloc.next(),
                            data: HirPatternDesc::Bind {
                                id: local_id,
                                name: *name,
                                mutable: true,
                            },
                            span: pat.span.clone(),
                        }
                    }
                    AstNamedPattern::Constructor { name, args } => {
                        let resolution =
                            resolve_in_module(this.db, name.interned(), module.interned());
                        match (resolution, args) {
                            (
                                Some(Definition::Type(_type_def_id)),
                                AstConstructFields::StructFields(_fields),
                            ) => {
                                todo!("Constructor pattern with struct fields")
                            }
                            (_, AstConstructFields::None) => {
                                let local_id = this.allocate_local(
                                    scope,
                                    *name,
                                    Mutability::Immutable,
                                    None,
                                    pat.span.clone(),
                                );
                                locals.push(local_id);
                                HirPattern {
                                    id: this.alloc.next(),
                                    data: HirPatternDesc::Bind {
                                        id: local_id,
                                        name: *name,
                                        mutable: false,
                                    },
                                    span: pat.span.clone(),
                                }
                            }
                            (_, _) => {
                                todo!(
                                    "error: `{}` is not a type but used as constructor pattern",
                                    name.interned().contents(this.db)
                                )
                            }
                        }
                    }
                    AstNamedPattern::NameResolved { from, to } => {
                        match resolve_in_module(this.db, from.interned(), module.interned()) {
                            Some(Definition::Module(id)) => {
                                let inner_pat = AstPattern::new(
                                    AstPatternDesc::Named(*to.clone()),
                                    vec![],
                                    pat.span.clone(),
                                );
                                _lower(this, &inner_pat, scope, locals, id)
                            }
                            Some(Definition::Type(TypeDefId::Enum(resolution))) => {
                                let (name, fields) = this.lower_pattern_as_constructor_args(
                                    to.as_ref(),
                                    pat.span.clone(),
                                    scope,
                                    locals,
                                );

                                HirPattern {
                                    id: this.alloc.next(),
                                    data: HirPatternDesc::Constructor {
                                        resolution,
                                        name,
                                        fields,
                                    },
                                    span: pat.span.clone(),
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
                        id: this.alloc.next(),
                        data: HirPatternDesc::Tuple(
                            fields
                                .iter()
                                .map(|x| _lower(this, x, scope, locals, this.module))
                                .collect(),
                        ),
                        span: pat.span.clone(),
                    },
                },
                AstPatternDesc::Any => HirPattern {
                    id: this.alloc.next(),
                    data: HirPatternDesc::Any,
                    span: pat.span.clone(),
                },
            }
        }

        let mut v = vec![];
        let pat = _lower(self, pat, scope, &mut v, self.module);
        (pat, v)
    }

    fn lower_stmt(&mut self, stmt: &AstStmt, scope: &mut Scope) -> HirStmt {
        HirStmt {
            id: self.alloc.next(),
            kind: match &stmt.data {
                AstStmtDesc::Return { value } => {
                    let value = value
                        .as_ref()
                        .map(|expr| self.lower_expr(expr, scope, self.module));
                    HirStmtKind::Return(value)
                }
                AstStmtDesc::If { cond, then, else_ } => {
                    let cond = self.lower_expr(cond, scope, self.module);
                    let then = self.lower_stmt(then, scope);
                    let else_ = else_
                        .as_ref()
                        .map(|s| self.lower_stmt(s, scope))
                        .map(Box::new);
                    HirStmtKind::If {
                        cond,
                        then: Box::new(then),
                        else_,
                    }
                }
                AstStmtDesc::While { cond, body } => {
                    let cond = self.lower_expr(cond, scope, self.module);
                    let body = self.lower_stmt(body, scope);
                    HirStmtKind::While {
                        cond,
                        body: Box::new(body),
                    }
                }
                AstStmtDesc::For {
                    element,
                    iterator,
                    body,
                } => {
                    let iterator_span = iterator.span.clone();
                    let iter_interface = core_iter_interface(self.db);
                    let into_iter_interface = core_into_iterator_interface(self.db);

                    let iterator_candidate = self.lower_expr(iterator, scope, self.module);

                    let iterator = HirExpr {
                        id: self.alloc.next(),
                        data: HirExprDesc::CallMethod {
                            receiver: Box::new(iterator_candidate),
                            method: Symbol::new(self.db, "into_iter"),
                            args: vec![],
                            interface_hint: Some(into_iter_interface),
                        },
                        span: iterator_span.clone(),
                    };

                    let iterator_id = LocalId(self.next_local_id);
                    self.next_local_id += 1;
                    let iterator_var_name =
                        Symbol::new(self.db, format!("@iterator_{}", iterator_id.0));
                    self.locals.insert(
                        iterator_id,
                        LocalInfo {
                            id: iterator_id,
                            name: iterator_var_name,
                            mutability: Mutability::Mutable,
                            ty_annotation: None,
                            span: iterator_span.clone(),
                        },
                    );

                    let elem_id = LocalId(self.next_local_id);
                    self.next_local_id += 1;
                    let elem_var_name =
                        Symbol::new(self.db, format!("@iterator_elem_{}", elem_id.0));
                    self.locals.insert(
                        elem_id,
                        LocalInfo {
                            id: elem_id,
                            name: elem_var_name,
                            mutability: Mutability::Mutable,
                            ty_annotation: None,
                            span: iterator_span.clone(),
                        },
                    );

                    let mut stmts = vec![];

                    stmts.push(HirStmt {
                        id: self.alloc.next(),
                        kind: HirStmtKind::Let {
                            pattern: HirPattern {
                                id: self.alloc.next(),
                                data: HirPatternDesc::Bind {
                                    id: iterator_id,
                                    name: iterator_var_name,
                                    mutable: true,
                                },
                                span: iterator_span.clone(),
                            },
                            locals: vec![iterator_id],
                            ty_annotation: None,
                            init: iterator,
                        },
                        span: iterator_span.clone(),
                    });

                    let elem_init = HirExpr {
                        id: self.alloc.next(),
                        data: HirExprDesc::CallMethod {
                            receiver: Box::new(HirExpr {
                                id: self.alloc.next(),
                                data: HirExprDesc::Use(HirPlace::Local(iterator_id)),
                                span: iterator_span.clone(),
                            }),
                            method: Symbol::new(self.db, "next"),
                            args: vec![],
                            interface_hint: Some(iter_interface),
                        },
                        span: iterator_span.clone(),
                    };

                    stmts.push(HirStmt {
                        id: self.alloc.next(),
                        kind: HirStmtKind::Let {
                            pattern: HirPattern {
                                id: self.alloc.next(),
                                data: HirPatternDesc::Bind {
                                    id: elem_id,
                                    name: elem_var_name,
                                    mutable: true,
                                },
                                span: iterator_span.clone(),
                            },
                            locals: vec![elem_id],
                            ty_annotation: None,
                            init: elem_init,
                        },
                        span: iterator_span.clone(),
                    });

                    let mut inner_scope = scope.clone();

                    let (pat, locals) = self.lower_pattern(element, &mut inner_scope);

                    stmts.push(HirStmt {
                        id: self.alloc.next(),
                        kind: HirStmtKind::While {
                            cond: HirExpr {
                                id: self.alloc.next(),
                                data: HirExprDesc::CallMethod {
                                    receiver: Box::new(HirExpr {
                                        id: self.alloc.next(),
                                        data: HirExprDesc::Use(HirPlace::Local(elem_id)),
                                        span: iterator_span.clone(),
                                    }),
                                    method: Symbol::new(self.db, "is_some"),
                                    args: vec![],
                                    interface_hint: Some(iter_interface),
                                },
                                span: iterator_span.clone(),
                            },
                            body: Box::new(HirStmt {
                                id: self.alloc.next(),
                                kind: HirStmtKind::Block(vec![
                                    HirStmt {
                                        id: self.alloc.next(),
                                        kind: HirStmtKind::Let {
                                            pattern: pat,
                                            locals,
                                            ty_annotation: None,
                                            init: HirExpr {
                                                id: self.alloc.next(),
                                                data: HirExprDesc::CallMethod {
                                                    receiver: Box::new(HirExpr {
                                                        id: self.alloc.next(),
                                                        data: HirExprDesc::Use(HirPlace::Local(
                                                            elem_id,
                                                        )),
                                                        span: iterator_span.clone(),
                                                    }),
                                                    method: Symbol::new(self.db, "unwrap"),
                                                    args: vec![],
                                                    interface_hint: None,
                                                },
                                                span: iterator_span.clone(),
                                            },
                                        },
                                        span: element.span.clone(),
                                    },
                                    self.lower_stmt(body, &mut inner_scope),
                                    HirStmt {
                                        id: self.alloc.next(),
                                        kind: HirStmtKind::Assign {
                                            lhs: HirPlace::Local(elem_id),
                                            rhs: HirExpr {
                                                id: self.alloc.next(),
                                                data: HirExprDesc::CallMethod {
                                                    receiver: Box::new(HirExpr {
                                                        id: self.alloc.next(),
                                                        data: HirExprDesc::Use(HirPlace::Local(
                                                            iterator_id,
                                                        )),
                                                        span: iterator_span.clone(),
                                                    }),
                                                    method: Symbol::new(self.db, "next"),
                                                    args: vec![],
                                                    interface_hint: Some(iter_interface),
                                                },
                                                span: iterator_span.clone(),
                                            },
                                        },
                                        span: element.span.clone(),
                                    },
                                ]),
                                span: iterator_span.clone(),
                            }),
                        },
                        span: iterator_span.clone(),
                    });

                    HirStmtKind::Block(stmts)
                }
                AstStmtDesc::LetDecl {
                    pat,
                    type_constraint,
                    value,
                } => {
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
                        stmts
                            .iter()
                            .map(|s| self.lower_stmt(s, &mut block_scope))
                            .collect(),
                    )
                }
                AstStmtDesc::Assign { lhs, rhs } => {
                    let place = self.expr_as_place(lhs, scope, self.module);
                    let rhs = self.lower_expr(rhs, scope, self.module);
                    HirStmtKind::Assign { lhs: place, rhs }
                }
                AstStmtDesc::CompoundAssign { lhs, op, rhs } => {
                    let place = self.expr_as_place(lhs, scope, self.module);
                    let binop = op.to_binop();
                    let lhs_expr = self.lower_expr(lhs, scope, self.module);
                    let rhs_expr = self.lower_expr(rhs, scope, self.module);
                    let combined = HirExpr {
                        id: self.alloc.next(),
                        data: HirExprDesc::BinOp {
                            lhs: Box::new(lhs_expr),
                            op: binop,
                            rhs: Box::new(rhs_expr),
                        },
                        span: stmt.span.clone(),
                    };
                    HirStmtKind::Assign {
                        lhs: place,
                        rhs: combined,
                    }
                }
                AstStmtDesc::Expr(expr) => {
                    let expr = self.lower_expr(expr, scope, self.module);
                    HirStmtKind::Expr(expr)
                }
                AstStmtDesc::Match {
                    scrutinee,
                    branches,
                } => {
                    let scrutinee = self.lower_expr(scrutinee, scope, self.module);

                    let branches = branches
                        .iter()
                        .map(|AstMatchBranch { pat, guard, body }| {
                            let mut branch_scope = scope.clone();
                            let (pattern, locals) = self.lower_pattern(pat, &mut branch_scope);
                            println!("Locals: {:?}", locals);
                            let guard = guard
                                .as_ref()
                                .map(|expr| self.lower_expr(expr, &branch_scope, self.module));
                            let body = self.lower_stmt(body, &mut branch_scope);
                            HirMatchBranch {
                                pattern,
                                locals,
                                guard,
                                body: Box::new(body),
                            }
                        })
                        .collect();
                    HirStmtKind::Match {
                        scrutinee,
                        branches,
                    }
                }
                AstStmtDesc::Defer(stmt) => {
                    HirStmtKind::Defer(Box::new(self.lower_stmt(stmt, scope)))
                }
            },
            span: stmt.span.clone(),
        }
    }

    fn collect_args(&mut self, args: &[AstFundefArg], scope: &mut Scope) -> Vec<LocalId> {
        args.iter()
            .map(|arg| {
                self.allocate_local(
                    scope,
                    arg.name,
                    Mutability::Immutable, // TODO: be able to change that
                    Some(arg.ty.clone().into()),
                    arg.span.clone(),
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
            self.locals.drain().map(|(_, x)| x).collect::<Vec<_>>(),
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
                                StructFieldPattern::Rebind { name, pattern } => {
                                    let (pat, new_locals) = self.lower_pattern(pattern, scope);
                                    locals.extend(new_locals);
                                    HirStructFieldPattern::Rebind {
                                        name: *name,
                                        pattern: pat,
                                    }
                                }
                                StructFieldPattern::Name(symbol) => {
                                    let new_local = self.allocate_local(
                                        scope,
                                        *symbol,
                                        Mutability::Immutable,
                                        None,
                                        span.clone(),
                                    );
                                    locals.push(new_local);
                                    HirStructFieldPattern::Name {
                                        name: *symbol,
                                        id: new_local,
                                    }
                                }
                            };
                            lowered_pats.push(pat);
                        }
                        HirPatternConstructorArgs::StructFields(lowered_pats)
                    }
                    AstConstructFields::None => HirPatternConstructorArgs::None,
                };
                (*name, args)
            }
            AstNamedPattern::NameResolved { .. } => todo!(),
            AstNamedPattern::Tuple { .. } => {
                unreachable!()
            }
            AstNamedPattern::Mut { .. } => unreachable!(),
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
    };
    let template_args = ast.data.template_args.clone();
    let mut ctx = LowerFundef::new(db, function, module, template_args);
    ctx.lower(ast)
}
