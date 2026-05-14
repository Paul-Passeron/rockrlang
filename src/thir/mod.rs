#![allow(dead_code)]

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt, iter,
    sync::Mutex,
};

use crate::{
    Db,
    hir::{
        HirBody, HirExpr, HirExprDesc, HirId, HirMatchBranch, HirPattern,
        HirPatternConstructorArgs, HirPatternDesc, HirPlace, HirStmt, HirStmtKind, LocalId,
        LocalInfo, PartialTypeArg, PartialTypeRef, hir_body, owning_module,
    },
    name_resolve::{
        definition::{Definition, resolve_in_module},
        type_expr::{
            TypeResolution, enum_item, get_templates_of_fun, resolve_type_expr, struct_item,
        },
    },
    parse_tree::{
        top_level::{AstEnumVariant, AstEnumVariantKind, AstStructDefField},
        type_expr::{AstAnyTypeExpr, AstAnyTypeExprDesc, AstTypeExpr, AstTypeExprDesc},
    },
    ril::{
        BuiltinTypeId, FunctionId, InternedFunctionId, ModuleId, Package, TypeDefId, TypeId,
        TypeParamId, TypeRef, display::RilDisplay, get_template_param_count, str_id,
    },
    thir::methods::{ImplMatchConstraint, ImplMatchConstraints, find_method_for_partial_ref},
};

pub mod methods;

#[derive(Clone)]
pub(self) struct InferCallInfos {
    pub(self) expr_id: ExprId,
    pub(self) callee: FunctionId,
    pub(self) substitution: Vec<InferTy>,
    pub(self) variadic: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CallInfos {
    pub expr_id: ExprId,
    pub callee: FunctionId,
    pub substitution: Vec<TypeRef>,
    pub variadic: bool,
}

impl InferCallInfos {
    fn new(
        expr_id: ExprId,
        callee: FunctionId,
        substitution: Vec<InferTy>,
        variadic: bool,
    ) -> Self {
        Self {
            expr_id,
            callee,
            substitution,
            variadic,
        }
    }
}

#[derive(Clone)]
struct TyCtx<'db> {
    db: &'db dyn Db,
    function: FunctionId,
    locals: &'db [LocalInfo],
    params: &'db [LocalId],
    packages: Vec<Package<'db>>,

    // Inference
    templates: Vec<TyVarId>,
    forwards: HashMap<InferTy, InferTy>,
    // type_eq_constrs: Vec<(InferTy, InferTy)>,
    calls: HashMap<ExprId, InferCallInfos>,
    exprs: HashMap<ExprId, InferTy>,
    infer_locals: HashMap<LocalId, InferTy>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExprId(HirId);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldId(usize);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatternId(HirId);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TyVarId(usize);

// #[derive(Clone, Copy, PartialEq, Eq, Hash)]
// struct IntVarId(usize);

// #[derive(Clone, Copy, PartialEq, Eq, Hash)]
// struct FloatVarId(usize);

impl TyVarId {
    pub fn alloc() -> Self {
        static NEXT: Mutex<usize> = Mutex::new(0);
        let res = Self(*NEXT.lock().unwrap());
        *NEXT.lock().unwrap() += 1;
        res
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(self) enum InferTy {
    Param(TypeParamId),
    Var(TyVarId),
    Adt {
        def: TypeDefId,
        args: Vec<InferTy>,
    },
    Deref(Box<InferTy>),
    RefOrDerefLike {
        base: Box<InferTy>, // Let's call this T
        like: Box<InferTy>, // if this is not a ref: T,
                            // if this is &mut ... => &mut T and if &... => &T
    },
    Error,
}

struct InferTyDisplay<'a> {
    ty: &'a InferTy,
    db: &'a dyn Db,
}

impl InferTy {
    pub fn display<'a>(&'a self, db: &'a dyn Db) -> InferTyDisplay<'a> {
        InferTyDisplay { db, ty: self }
    }
}

impl<'a> fmt::Display for InferTyDisplay<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.ty {
            InferTy::Var(ty_var_id) => write!(f, "'{}'", ty_var_id.0),
            InferTy::Param(id) => write!(f, "T{}", id.0),
            InferTy::Error => write!(f, "{{ERROR}}"),
            InferTy::Adt { def, args } => {
                write!(f, "{}", def.name(self.db).display(self.db))?;
                if !args.is_empty() {
                    write!(f, "<")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", arg.display(self.db))?;
                    }
                    write!(f, ">")?;
                }
                Ok(())
            }
            InferTy::Deref(infer_ty) => write!(f, "Deref{{{}}}", infer_ty.display(self.db)),
            InferTy::RefOrDerefLike { base, like } => {
                write!(
                    f,
                    "RefOrDerefLike({} as {})",
                    base.display(self.db),
                    like.display(self.db)
                )
            }
        }
    }
}

impl<'db> TyCtx<'db> {
    pub fn new(
        db: &'db dyn Db,
        function: FunctionId,
        locals: &'db [LocalInfo],
        params: &'db [LocalId],
        packages: Vec<Package<'db>>,
    ) -> Self {
        let templates = get_templates_of_fun(db, function.interned())
            .iter()
            .map(|_| TyVarId::alloc())
            .collect();
        let mut this = Self {
            db,
            function,
            locals,
            params,
            templates,
            packages,
            forwards: HashMap::new(),
            calls: HashMap::new(),
            exprs: HashMap::new(),
            infer_locals: HashMap::new(),
        };

        for local in locals {
            let var_id = TyVarId::alloc();
            this.infer_locals
                .insert(local.id.clone(), InferTy::Var(var_id));
            if let Some(annotation) = local.ty_annotation.as_ref()
                && let AstAnyTypeExprDesc::Known(desc) = &annotation.data
            {
                let partially_resolved =
                    this.resolve_holed_desc(desc, owning_module(db, function.parent(db)));
                let inferred_annotation = this.allocate_partial(partially_resolved);
                this.unify(InferTy::Var(var_id), inferred_annotation);
            }
        }
        this
    }

    fn find(&self, ty: InferTy) -> InferTy {
        let mut res = ty;
        while let Some(other) = self.forwards.get(&res)
            && other != &InferTy::Error
        {
            res = other.clone();
        }
        res
    }

    fn is_var_in_ty(var: TyVarId, ty: &InferTy) -> bool {
        match ty {
            InferTy::Var(ty_var_id) => *ty_var_id == var,
            InferTy::Adt { args, .. } => args.iter().any(|arg| Self::is_var_in_ty(var, arg)),
            _ => false,
        }
    }

    fn occurence_unify(&mut self, var: TyVarId, ty: InferTy) {
        if Self::is_var_in_ty(var, &ty) {
            // Recursive type def, error and breaking the forwards chain (see find)
            self.forwards
                .entry(InferTy::Var(var))
                .insert_entry(InferTy::Error);
        } else {
            // We know that var is the last in its forward chain
            let equates_to = self.find(ty);
            self.forwards.insert(InferTy::Var(var), equates_to);
        }
    }

    fn unify(&mut self, ty_a: InferTy, ty_b: InferTy) {
        let ty_a = self.find(ty_a);
        let ty_b = self.find(ty_b);
        println!(
            "Unifying {} and {}",
            ty_a.display(self.db),
            ty_b.display(self.db)
        );
        match (ty_a, ty_b) {
            (InferTy::Error, b) => {
                self.forwards.insert(b, InferTy::Error);
            }
            (InferTy::Param(x), InferTy::Param(y)) => {
                if x != y {
                    self.forwards.insert(InferTy::Param(x), InferTy::Error);
                    self.forwards.insert(InferTy::Param(y), InferTy::Error);
                }
            }
            (InferTy::Var(var), b) => {
                self.occurence_unify(var, b);
            }
            (
                InferTy::Adt {
                    def: def_a,
                    args: args_a,
                },
                InferTy::Adt {
                    def: def_b,
                    args: args_b,
                },
            ) => {
                if def_a != def_b {
                    // We error both out
                    println!("ERROR from != def");
                    self.forwards.insert(
                        InferTy::Adt {
                            def: def_a,
                            args: args_a,
                        },
                        InferTy::Error,
                    );
                    self.forwards.insert(
                        InferTy::Adt {
                            def: def_b,
                            args: args_b,
                        },
                        InferTy::Error,
                    );
                } else {
                    for (arg_a, arg_b) in args_a.clone().into_iter().zip(args_b.clone().into_iter())
                    {
                        self.unify(arg_a.clone(), arg_b.clone());
                        if self.find(arg_a) == InferTy::Error {
                            println!("ERROR bad arg");

                            // error out if it contains an error
                            self.forwards.insert(
                                InferTy::Adt {
                                    def: def_a,
                                    args: args_a,
                                },
                                InferTy::Error,
                            );
                            self.forwards.insert(
                                InferTy::Adt {
                                    def: def_b,
                                    args: args_b,
                                },
                                InferTy::Error,
                            );
                            break;
                        }
                    }
                }
            }
            (InferTy::Deref(pointee), InferTy::Adt { def, args }) => {
                let ptr_likes = [
                    BuiltinTypeId::mut_ptr(self.db),
                    BuiltinTypeId::ptr(self.db),
                    BuiltinTypeId::mut_ref(self.db),
                    BuiltinTypeId::ref_(self.db),
                ];
                for ptr_like in &ptr_likes {
                    if def == TypeDefId::Builtin(*ptr_like) {
                        self.unify(*pointee.clone(), args[0].clone());
                        return;
                    }
                }
                self.unify(InferTy::Deref(pointee), InferTy::Error);
                self.unify(InferTy::Adt { def, args }, InferTy::Error);
            }
            (InferTy::Deref(pointee), _) => todo!(),
            (a, b) => self.unify(b, a), // Switch the types
        }
    }

    fn allocate_partial(&mut self, ty: PartialTypeRef) -> InferTy {
        match ty {
            PartialTypeRef::Resolved(type_ref) => self.allocate_ref(
                type_ref,
                &self
                    .templates
                    .iter()
                    .copied()
                    .map(|x| InferTy::Var(x))
                    .collect::<Box<[_]>>(),
            ),
            PartialTypeRef::WithHoles { def, args } => InferTy::Adt {
                def,
                args: args
                    .into_iter()
                    .map(|arg| match arg {
                        PartialTypeArg::Known(type_ref) => {
                            self.allocate_partial(PartialTypeRef::Resolved(type_ref))
                        }
                        PartialTypeArg::Partial(type_ref) => self.allocate_partial(*type_ref),
                        PartialTypeArg::Infer => InferTy::Var(TyVarId::alloc()),
                    })
                    .collect(),
            },
        }
    }

    fn resolve_holed_ty(&self, ty: &AstTypeExpr, module: ModuleId) -> PartialTypeRef {
        self.resolve_holed_desc(&ty.data, module)
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

    fn resolve_holed_desc(&self, desc: &AstTypeExprDesc, module: ModuleId) -> PartialTypeRef {
        match desc {
            AstTypeExprDesc::Named { name, args } => {
                if args.is_empty() {
                    if let Some(idx) = get_templates_of_fun(self.db, self.function.interned())
                        .iter()
                        .position(|p| p.name == *name)
                    {
                        return PartialTypeRef::Resolved(TypeRef::Param(TypeParamId(idx)));
                    }
                }

                let Some(Definition::Type(type_def_id)) =
                    resolve_in_module(self.db, name.interned(), module.interned())
                else {
                    panic!("todo {}", name.display(self.db))
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

    fn resolve_any_holed_arg(&self, any_ty: &AstAnyTypeExpr, module: ModuleId) -> PartialTypeArg {
        match &any_ty.data {
            AstAnyTypeExprDesc::Any => PartialTypeArg::Infer,
            AstAnyTypeExprDesc::Known(desc) => {
                self.resolve_holed_desc(desc, module).to_partial_arg()
            }
        }
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

    fn solve(&self, ty: InferTy) -> TypeRef {
        let ty = self.find(ty);
        match ty {
            InferTy::Adt { def, args } => {
                let args = args
                    .into_iter()
                    .map(|arg| self.solve(arg))
                    .collect::<Vec<_>>();
                TypeRef::Concrete(TypeId::new(self.db, def, args))
            }
            _ => TypeRef::Error,
        }
    }

    fn concretize_infos(&self, infos: InferCallInfos) -> CallInfos {
        CallInfos {
            expr_id: infos.expr_id,
            callee: infos.callee,
            substitution: infos
                .substitution
                .into_iter()
                .map(|ty| self.solve(ty))
                .collect(),
            variadic: infos.variadic,
        }
    }

    fn finalize(mut self) -> TypeCheckResults<'db> {
        // Solve constraints and populate result
        let drain = self.exprs.drain().collect::<Box<[_]>>();
        let node_types = drain
            .into_iter()
            .map(|(id, infer_ty)| (id, self.solve(infer_ty)))
            .collect();

        let call_infos = self
            .calls
            .drain()
            .collect::<Box<[_]>>()
            .into_iter()
            .map(|(id, infos)| (id, self.concretize_infos(infos)))
            .collect();

        TypeCheckResults::new(self.db, node_types, call_infos)
    }

    fn int_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::int(self.db)),
            args: vec![],
        }
    }

    fn char_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::char(self.db)),
            args: vec![],
        }
    }

    fn bool_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::bool(self.db)),
            args: vec![],
        }
    }

    fn str_ty(&self) -> InferTy {
        InferTy::Adt {
            def: str_id(self.db).def(self.db),
            args: vec![],
        }
    }

    fn never_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::never(self.db)),
            args: vec![],
        }
    }

    fn void_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::void(self.db)),
            args: vec![],
        }
    }

    fn const_ptr_of(&self, ty: InferTy) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::ptr(self.db)),
            args: vec![ty],
        }
    }

    fn type_check_place(&mut self, place: &'db HirPlace) -> InferTy {
        match place {
            HirPlace::Local(local_id) => self
                .infer_locals
                .get(local_id)
                .cloned()
                .unwrap_or(InferTy::Error),
            HirPlace::Field { base, field } => todo!(),
            HirPlace::TupleField { base, index } => todo!(),
            HirPlace::Deref(hir_place) => {
                let place_ty = self.type_check_place(hir_place);
                let pointee = InferTy::Var(TyVarId::alloc());
                let ptr_ty = InferTy::Deref(Box::new(pointee.clone()));
                self.unify(place_ty, ptr_ty);
                pointee
            }
            HirPlace::Index { base, index } => todo!(),
            HirPlace::Temporary(hir_expr) => self.type_check_expr(hir_expr),
        }
    }

    fn fresh_var() -> InferTy {
        InferTy::Var(TyVarId::alloc())
    }

    fn allocate_ref(&self, ty_ref: TypeRef, templates: &[InferTy]) -> InferTy {
        fn _allocate_ref(
            db: &dyn Db,
            ty_ref: TypeRef,
            templates: &[InferTy],
            seen: &mut HashSet<TypeRef>,
        ) -> InferTy {
            if !seen.insert(ty_ref) {
                return InferTy::Error;
            }
            match ty_ref {
                TypeRef::Concrete(type_id) => {
                    let def = type_id.def(db);
                    let args = type_id.args(db);
                    let allocated_args = args
                        .iter()
                        .copied()
                        .map(|x| _allocate_ref(db, x, templates, seen))
                        .collect::<_>();
                    InferTy::Adt {
                        def,
                        args: allocated_args,
                    }
                }
                TypeRef::Param(type_param_id) => templates[type_param_id.0].clone(),
                TypeRef::Error => InferTy::Error,
            }
        }
        _allocate_ref(self.db, ty_ref, templates, &mut HashSet::new())
    }

    fn type_check_expr(&mut self, expr: &'db HirExpr) -> InferTy {
        let ty = match &expr.data {
            HirExprDesc::IntLit(_) => self.int_ty(),
            HirExprDesc::CharLit(_) => self.char_ty(),
            HirExprDesc::StrLit(_) => self.str_ty(),
            HirExprDesc::CStrLit(_) => self.const_ptr_of(self.char_ty()),
            HirExprDesc::BoolLit(_) => self.bool_ty(),
            HirExprDesc::Use(place) => self.type_check_place(place),
            HirExprDesc::AddressOf { .. } => todo!(),
            HirExprDesc::Ref { .. } => todo!(),
            HirExprDesc::CallDirect { target, args } => {
                let target = *target;
                let owning_module = owning_module(self.db, target.parent(self.db));
                let templates = get_templates_of_fun(self.db, target.interned());
                let (receiver, ast_args) = target.args(self.db);
                let infer_templates = templates
                    .iter()
                    .map(|_| Self::fresh_var())
                    .collect::<Vec<_>>();
                let infered_args = args
                    .iter()
                    .map(|arg| self.type_check_expr(arg))
                    .collect::<Vec<_>>();

                let ast_args = receiver
                    .into_iter()
                    .chain(ast_args.iter().map(|arg| {
                        match resolve_type_expr(
                            self.db,
                            &arg.ty,
                            owning_module.interned(),
                            &templates,
                        ) {
                            TypeResolution::Type(type_ref) => type_ref,
                            _ => unreachable!(),
                        }
                    }))
                    .map(|ty| self.allocate_ref(ty, &infer_templates))
                    .collect::<Box<[_]>>();

                infered_args.into_iter().zip(ast_args).for_each(|(a, b)| {
                    self.unify(a, b);
                });

                let return_ty = InferTy::Var(TyVarId::alloc());
                let computed_return_ty =
                    self.allocate_ref(target.ret_ty(self.db), &infer_templates);

                println!(
                    "Computed return type is {}",
                    computed_return_ty.display(self.db)
                );

                self.unify(return_ty.clone(), computed_return_ty);

                let infos = InferCallInfos::new(ExprId(expr.id), target, infer_templates, false);
                self.calls.insert(ExprId(expr.id), infos);
                return_ty
            }
            HirExprDesc::CallMethod {
                receiver,
                method,
                args,
                interface_hint,
            } => {
                let inferred_args = args
                    .iter()
                    .map(|arg| self.type_check_expr(arg))
                    .collect::<Vec<_>>();
                let inferred_receiver = self.type_check_expr(receiver);
                let candidates = find_method_for_partial_ref(
                    self.db,
                    inferred_receiver.clone(),
                    *method,
                    self.packages.clone(),
                )
                .into_iter()
                .filter(|(id, _)| id.args(self.db).1.len() == args.len())
                .collect::<Box<[_]>>();

                for package in &self.packages {
                    println!(
                        "Package {}",
                        package.root(self.db).name(self.db).display(self.db)
                    );
                }

                if candidates.is_empty() {
                    println!("No candidates found for method {}", method.display(self.db));
                    return InferTy::Error;
                }

                if candidates.len() > 1 {
                    println!(
                        "Too many candidates found for method {}",
                        method.display(self.db)
                    );
                    return InferTy::Error;
                }

                let (
                    id,
                    ImplMatchConstraints {
                        substitution,
                        constraints,
                    },
                ) = candidates.into_iter().next().unwrap();

                for constraint in constraints {
                    match constraint {
                        ImplMatchConstraint::Unify(ty_a, ty_b) => {
                            println!(
                                "With constraints: Unifying {} and {}",
                                ty_a.display(self.db),
                                ty_b.display(self.db)
                            );
                            self.unify(ty_a, ty_b);
                        }
                        ImplMatchConstraint::Implements(infer_ty, interface_ref) => todo!(),
                    }
                }

                let templates = get_templates_of_fun(self.db, id.interned());

                let infer_templates = templates
                    .iter()
                    .map(|_| Self::fresh_var())
                    .collect::<Vec<_>>();

                for (template, infer_template) in substitution.iter().zip(infer_templates.iter()) {
                    if let Some(infer) = template {
                        self.unify(infer.clone(), infer_template.clone());
                    }
                }

                let owning_module = owning_module(self.db, id.parent(self.db));

                let (receiver, ast_args) = id.args(self.db);

                if let Some(ty) = receiver
                    .as_ref()
                    .map(|ty| self.allocate_ref(*ty, &infer_templates))
                {
                    self.unify(inferred_receiver, ty);
                }

                println!("ast_args: {}", ast_args.len());

                for (a, b) in inferred_args.into_iter().zip(
                    ast_args
                        .iter()
                        .map(|arg| {
                            match resolve_type_expr(
                                self.db,
                                &arg.ty,
                                owning_module.interned(),
                                &templates,
                            ) {
                                TypeResolution::Type(type_ref) => {
                                    println!("Resolved arg type: {}", type_ref.display(self.db));
                                    type_ref
                                }
                                _ => unreachable!(),
                            }
                        })
                        .map(|ty| self.allocate_ref(ty, &infer_templates))
                        .collect::<Box<[_]>>(),
                ) {
                    println!("Unifying method call args");
                    self.unify(a, b);
                }

                let computed_return_ty = self.allocate_ref(id.ret_ty(self.db), &infer_templates);
                computed_return_ty
            }
            HirExprDesc::CallStatic { ty, method, args } => {
                let partial = self.allocate_partial(ty.clone());
                let candidates =
                    find_method_for_partial_ref(self.db, partial, *method, self.packages.clone())
                        .into_iter()
                        .filter(|(id, _)| id.args(self.db).1.len() == args.len())
                        .collect::<Box<[_]>>();

                let inferred_args = args
                    .iter()
                    .map(|arg| self.type_check_expr(arg))
                    .collect::<Vec<_>>();

                if candidates.is_empty() {
                    println!("No candidates found for method {}", method.display(self.db));
                    return InferTy::Error;
                }

                if candidates.len() > 1 {
                    println!(
                        "Too many candidates found for method {}",
                        method.display(self.db)
                    );
                    return InferTy::Error;
                }

                let (
                    id,
                    ImplMatchConstraints {
                        substitution,
                        constraints,
                    },
                ) = candidates.into_iter().next().unwrap();

                for constraint in constraints {
                    match constraint {
                        ImplMatchConstraint::Unify(a, b) => {
                            self.unify(a, b);
                        }
                        ImplMatchConstraint::Implements(infer_ty, interface_ref) => {
                            todo!()
                        }
                    }
                }

                let templates = get_templates_of_fun(self.db, id.interned());

                let infer_templates = templates
                    .iter()
                    .map(|_| Self::fresh_var())
                    .collect::<Vec<_>>();

                for (template, infer_template) in substitution.iter().zip(infer_templates.iter()) {
                    println!(
                        "Unifying template arg: {:?} with {}",
                        template.as_ref().map(|x| x.display(self.db).to_string()),
                        infer_template.display(self.db)
                    );
                    if let Some(infer) = template {
                        println!("Unifying template args");
                        self.unify(infer.clone(), infer_template.clone());
                    }
                }

                let owning_module = owning_module(self.db, id.parent(self.db));

                let (receiver, ast_args) = id.args(self.db);

                for (a, b) in inferred_args.into_iter().zip(
                    receiver
                        .into_iter()
                        .chain(ast_args.iter().map(|arg| {
                            match resolve_type_expr(
                                self.db,
                                &arg.ty,
                                owning_module.interned(),
                                &templates,
                            ) {
                                TypeResolution::Type(type_ref) => type_ref,
                                _ => unreachable!(),
                            }
                        }))
                        .map(|ty| self.allocate_ref(ty, &infer_templates))
                        .collect::<Box<[_]>>(),
                ) {
                    self.unify(a, b);
                }

                let computed_return_ty = self.allocate_ref(id.ret_ty(self.db), &infer_templates);
                computed_return_ty
            }
            HirExprDesc::BinOp { .. } => todo!(),
            HirExprDesc::StructLit { ty, fields } => {
                let struct_def = match ty {
                    PartialTypeRef::Resolved(type_ref) => match type_ref {
                        TypeRef::Concrete(type_id) => match type_id.def(self.db) {
                            TypeDefId::Struct(struct_id) => Some(struct_id),
                            _ => None,
                        },
                        TypeRef::Param(type_param_id) => None,
                        TypeRef::Error => None,
                    },
                    PartialTypeRef::WithHoles { def, args } => match def {
                        TypeDefId::Struct(struct_id) => Some(*struct_id),
                        _ => None,
                    },
                };
                let mut inferred_exprs = vec![];
                for (sym, expr) in fields {
                    let expr_ty = self.type_check_expr(expr);
                    inferred_exprs.push((*sym, expr_ty));
                }
                if let Some(struct_id) = struct_def {
                    let struct_item = struct_item(self.db, struct_id.interned());
                    let templates_inferred = struct_item
                        .template_args
                        .iter()
                        .map(|_| InferTy::Var(TyVarId::alloc()))
                        .collect::<Vec<_>>();

                    let field_types: HashMap<_, _> =
                        HashMap::from_iter(struct_item.fields.iter().map(
                            |AstStructDefField { name, ty }| match resolve_type_expr(
                                self.db,
                                ty,
                                struct_id.parent(self.db).interned(),
                                &struct_item.template_args,
                            ) {
                                TypeResolution::Type(type_ref) => {
                                    let allocated =
                                        self.allocate_ref(type_ref, &templates_inferred);
                                    (*name, allocated)
                                }
                                _ => (*name, InferTy::Error),
                            },
                        ));

                    let mut seen_fields = HashSet::new();
                    for (field_name, ty) in inferred_exprs {
                        if !seen_fields.insert(field_name) {
                            todo!("Same field in struct lit twice")
                        }
                        if let Some(field_ty) = field_types.get(&field_name) {
                            self.unify(ty, field_ty.clone());
                        } else {
                            todo!(
                                "No field named {} in struct {}",
                                field_name.display(self.db),
                                struct_id.display(self.db)
                            )
                        }
                    }

                    InferTy::Adt {
                        def: TypeDefId::Struct(struct_id),
                        args: templates_inferred,
                    }
                } else {
                    todo!()
                }
            }
            HirExprDesc::Neg(_) => todo!(),
            HirExprDesc::Not(_) => todo!(),
            HirExprDesc::Tuple(_) => todo!(),
            HirExprDesc::SliceLit(_) => todo!(),
            HirExprDesc::SizeOf(_) => todo!(),
            HirExprDesc::Constructor { .. } => todo!(),
        };
        self.exprs.insert(ExprId(expr.id), ty.clone());
        ty
    }

    fn get_ret_ty(&self) -> InferTy {
        let ret = self.function.ret_ty(self.db);
        self.allocate_ref(
            ret,
            &self
                .templates
                .iter()
                .copied()
                .map(InferTy::Var)
                .collect::<Box<[_]>>(),
        )
    }

    fn type_check_patt(&mut self, pattern: &'db HirPattern) -> InferTy {
        match &pattern.data {
            HirPatternDesc::Bind { id, name, mutable } => {
                self.infer_locals.get(id).cloned().unwrap_or(InferTy::Error)
            }
            HirPatternDesc::Any => InferTy::Var(TyVarId::alloc()),
            HirPatternDesc::Tuple(hir_patterns) => todo!(),
            HirPatternDesc::DestructureBinding { resolution, fields } => todo!(),
            HirPatternDesc::Constructor {
                resolution,
                name,
                fields,
            } => {
                let enum_item = enum_item(self.db, resolution.interned());
                let module = resolution.parent(self.db);
                let inferred_templates = enum_item
                    .template_args
                    .iter()
                    .map(|_| InferTy::Var(TyVarId::alloc()))
                    .collect::<Vec<_>>();
                let infer_ty = InferTy::Adt {
                    def: TypeDefId::Enum(*resolution),
                    args: inferred_templates.clone(),
                };

                if let Some(variant) = enum_item.variants.iter().find(|v| v.name == *name) {
                    match fields {
                        HirPatternConstructorArgs::None => {
                            let AstEnumVariant {
                                kind: AstEnumVariantKind::Unit,
                                ..
                            } = variant
                            else {
                                panic!(
                                    "Expected unit variant for pattern constructor, found struct or tuple variant"
                                )
                            };
                        }
                        HirPatternConstructorArgs::StructFields(structs) => {
                            let AstEnumVariant {
                                kind: AstEnumVariantKind::StructLike(ast_structs),
                                ..
                            } = variant
                            else {
                                panic!(
                                    "Expected unit variant for pattern constructor, found struct or tuple variant"
                                )
                            };
                            todo!()
                        }
                        HirPatternConstructorArgs::TupleFields(pats) => {
                            let AstEnumVariant {
                                kind: AstEnumVariantKind::TupleLike(ast_pats),
                                ..
                            } = variant
                            else {
                                panic!(
                                    "Expected unit variant for pattern constructor, found struct or tuple variant"
                                )
                            };

                            assert!(pats.len() == ast_pats.len());

                            for (pat, ast_pat) in pats.iter().zip(ast_pats) {
                                match resolve_type_expr(
                                    self.db,
                                    ast_pat,
                                    module.interned(),
                                    &enum_item.template_args,
                                ) {
                                    TypeResolution::Type(type_ref) => {
                                        let allocated =
                                            self.allocate_ref(type_ref, &inferred_templates);
                                        let pat_ty = self.type_check_patt(pat);
                                        self.unify(pat_ty, allocated);
                                    }
                                    _ => todo!(),
                                };
                            }
                        }
                    };
                    infer_ty
                } else {
                    InferTy::Error
                }
            }
        }
    }

    fn type_check_stmt(&mut self, stmt: &'db HirStmt) {
        match &stmt.kind {
            HirStmtKind::Let {
                pattern,
                locals,
                ty_annotation,
                init,
            } => {
                for local in locals {
                    self.infer_locals
                        .insert(*local, InferTy::Var(TyVarId::alloc()));
                }
                let init_ty = self.type_check_expr(init);
                let patt_ty = self.type_check_patt(pattern);
                self.unify(init_ty, patt_ty.clone());
                if let Some(annotation) = ty_annotation.as_ref()
                    && let AstAnyTypeExprDesc::Known(desc) = &annotation.data
                {
                    let resolved = self.resolve_holed_desc(
                        desc,
                        owning_module(self.db, self.function.parent(self.db)),
                    );
                    let allocated = self.allocate_partial(resolved);
                    self.unify(patt_ty, allocated);
                }
            }
            HirStmtKind::Match {
                scrutinee,
                branches,
            } => {
                // TODO: Implement matching a ref with patterns where
                // the fields are references as well
                let scrut_ty = self.type_check_expr(scrutinee);
                for HirMatchBranch {
                    pattern,
                    locals: _,
                    guard,
                    body,
                } in branches
                {
                    let pattern_ty = self.type_check_patt(pattern);
                    self.unify(pattern_ty, scrut_ty.clone());
                    if let Some(guard_ty) = guard.as_ref().map(|expr| self.type_check_expr(expr)) {
                        self.unify(guard_ty, self.bool_ty());
                    }
                    self.type_check_stmt(body);
                }
                // todo!()
            }
            HirStmtKind::Assign { .. } => todo!(),
            HirStmtKind::Expr(expr) => {
                self.type_check_expr(expr);
            }
            HirStmtKind::Return(value) => {
                let ty = value.as_ref().map(|expr| self.type_check_expr(expr));
                if let Some(ty) = ty {
                    self.unify(ty, self.get_ret_ty());
                } else {
                    self.unify(self.void_ty(), self.get_ret_ty());
                }
                // self.type_eq_constrs.push((ty, self.never_ty()));
            }
            HirStmtKind::If { .. } => {
                todo!()
            }
            HirStmtKind::While { cond, body } => {
                let cond_ty = self.type_check_expr(cond);
                self.unify(cond_ty, self.bool_ty());
                self.type_check_stmt(body);
            }
            HirStmtKind::Block(block) => {
                block.iter().for_each(|stmt| self.type_check_stmt(stmt));
            }
            HirStmtKind::Defer(deferred) => {
                self.type_check_stmt(deferred);
            }
            HirStmtKind::Break => (),
        }
    }

    fn type_check(mut self, stmts: &'db [HirStmt]) -> TypeCheckResults<'db> {
        stmts.iter().for_each(|stmt| self.type_check_stmt(stmt));
        self.finalize()
    }
}

#[salsa::tracked]
pub struct TypeCheckResults<'db> {
    pub node_types: BTreeMap<ExprId, TypeRef>,
    pub call_infos: BTreeMap<ExprId, CallInfos>,
}

fn type_check_hir<'db>(
    db: &'db dyn Db,
    hir: HirBody<'db>,
    packages: Vec<Package<'db>>,
) -> TypeCheckResults<'db> {
    TyCtx::new(db, hir.owner(db), hir.locals(db), hir.params(db), packages)
        .type_check(hir.stmts(db))
}

#[salsa::tracked]
pub fn type_check_function<'db>(
    db: &'db dyn Db,
    function: InternedFunctionId<'db>,
    packages: Vec<Package<'db>>,
) -> Option<TypeCheckResults<'db>> {
    hir_body(db, function).map(|hir| type_check_hir(db, hir, packages))
}
