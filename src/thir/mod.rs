#![allow(dead_code)]

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt, iter,
    sync::Mutex,
};

use crate::{
    Db,
    hir::{
        HirBody, HirExpr, HirExprDesc, HirId, HirPlace, HirStmt, HirStmtKind, LocalId, LocalInfo,
        Mutability, PartialTypeArg, PartialTypeRef, hir_body, owning_module,
    },
    name_resolve::{
        definition::{Definition, resolve_in_module},
        type_expr::{TypeResolution, get_templates_of_fun, resolve_type_expr},
    },
    parse_tree::type_expr::{AstAnyTypeExpr, AstAnyTypeExprDesc, AstTypeExpr, AstTypeExprDesc},
    ril::{
        BuiltinTypeId, FunctionId, InternedFunctionId, ModuleId, TypeDefId, TypeId, TypeParamId,
        TypeRef, bool_id, char_id, display::RilDisplay, get_template_param_count, never_id, ptr_of,
        str_id, void_id,
    },
};

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

    // Inference
    templates: Vec<TyVarId>,
    type_eq_constrs: Vec<(InferTy, InferTy)>,
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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct IntVarId(usize);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct FloatVarId(usize);

impl TyVarId {
    pub fn alloc() -> Self {
        static NEXT: Mutex<usize> = Mutex::new(0);
        let res = Self(*NEXT.lock().unwrap());
        *NEXT.lock().unwrap() += 1;
        res
    }
}

impl IntVarId {
    pub fn alloc() -> Self {
        static NEXT: Mutex<usize> = Mutex::new(0);
        let res = Self(*NEXT.lock().unwrap());
        *NEXT.lock().unwrap() += 1;
        res
    }
}

impl FloatVarId {
    pub fn alloc() -> Self {
        static NEXT: Mutex<usize> = Mutex::new(0);
        let res = Self(*NEXT.lock().unwrap());
        *NEXT.lock().unwrap() += 1;
        res
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum InferTy {
    Known(TypeRef),
    Var(TyVarId),
    IntVar(IntVarId),
    FloatVar(FloatVarId),
    Ptr(Mutability, Box<InferTy>),
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
            InferTy::Known(type_ref) => write!(f, "{}", type_ref.display(self.db)),
            InferTy::Var(ty_var_id) => write!(f, "'{}'", ty_var_id.0),
            InferTy::IntVar(int_var_id) => write!(f, "int({})", int_var_id.0),
            InferTy::FloatVar(float_var_id) => write!(f, "float({})", float_var_id.0),
            InferTy::Ptr(mutability, infer_ty) => write!(
                f,
                "*{}{}",
                match mutability {
                    Mutability::Mutable => "mut ",
                    Mutability::Immutable => "",
                },
                infer_ty.display(self.db)
            ),
            InferTy::Error => write!(f, "{{ERROR}}"),
        }
    }
}

impl<'db> TyCtx<'db> {
    pub fn new(
        db: &'db dyn Db,
        function: FunctionId,
        locals: &'db [LocalInfo],
        params: &'db [LocalId],
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
            type_eq_constrs: Vec::new(),
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
                this.type_eq_constrs
                    .push((InferTy::Var(var_id), inferred_annotation));
            }
        }

        this
    }

    fn allocate_partial(&mut self, ty: PartialTypeRef) -> InferTy {
        match ty {
            PartialTypeRef::Resolved(type_ref) => InferTy::Known(type_ref),
            PartialTypeRef::WithHoles { .. } => todo!(),
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

    fn equivalence_class(&self, ty: &InferTy) -> HashSet<InferTy> {
        let mut res = HashSet::new();
        for (a, b) in &self.type_eq_constrs {
            if a == ty || b == ty {
                res.insert(a.clone());
                res.insert(b.clone());
            }
        }
        res
    }

    fn equivalence_classes(&self) -> HashMap<InferTy, HashSet<InferTy>> {
        let mut res: HashMap<InferTy, HashSet<InferTy>> = HashMap::new();
        for (a, b) in &self.type_eq_constrs {
            res.insert(a.clone(), self.equivalence_class(a));
            res.insert(b.clone(), self.equivalence_class(b));
        }
        res
    }

    fn normalize_type(
        &self,
        ty: &InferTy,
        equivalences: &HashMap<InferTy, HashSet<InferTy>>,
    ) -> TypeRef {
        self.normalize_type_aux(ty, equivalences, &mut HashSet::new())
    }

    fn normalize_type_aux(
        &self,
        ty: &InferTy,
        equivalences: &HashMap<InferTy, HashSet<InferTy>>,
        seen: &mut HashSet<InferTy>,
    ) -> TypeRef {
        if !seen.insert(ty.clone()) {
            return TypeRef::Error;
        }
        match ty {
            InferTy::Known(type_ref) => *type_ref,
            InferTy::Ptr(mutability, pointee) => {
                let mutable = matches!(mutability, Mutability::Mutable);
                let normalized_pointee = self.normalize_type_aux(pointee, equivalences, seen);
                if let TypeRef::Error = normalized_pointee {
                    TypeRef::Error
                } else {
                    ptr_of(self.db, normalized_pointee, mutable).into()
                }
            }
            InferTy::Error => TypeRef::Error,
            _ => {
                let empty = &HashSet::new();
                // Must look into the candidates
                equivalences
                    .get(ty)
                    .unwrap_or(&empty)
                    .iter()
                    .filter_map(|infer_ty| {
                        match self.normalize_type_aux(infer_ty, equivalences, seen) {
                            TypeRef::Error => None,
                            ty_ref => Some(ty_ref),
                        }
                    })
                    .next()
                    .unwrap_or(TypeRef::Error)
            }
        }
    }

    fn print_equivalence_classes(&self, equivalence_classes: &HashMap<InferTy, HashSet<InferTy>>) {
        println!(
            "{}",
            equivalence_classes
                .iter()
                .map(|(key, val)| {
                    format!(
                        "{}: {{{}}}",
                        key.display(self.db),
                        val.iter()
                            .map(|ty| format!("{}", ty.display(self.db)))
                            .collect::<Box<[String]>>()
                            .join(", ")
                    )
                })
                .collect::<Box<[String]>>()
                .join("\n")
        );
    }

    fn solve_infer_ty(
        &self,
        ty: InferTy,
        equivalences: &HashMap<InferTy, HashSet<InferTy>>,
    ) -> TypeRef {
        let s = equivalences.get(&ty).unwrap();
        let candidates = s
            .iter()
            .filter_map(
                |infer_ty| match self.normalize_type(infer_ty, equivalences) {
                    TypeRef::Error => None,
                    it => Some(it),
                },
            )
            .collect::<HashSet<_>>();
        if candidates.len() == 0 {
            println!(
                "Could not find any candidates for infer type {}",
                ty.display(self.db)
            );
            TypeRef::Error
        } else if candidates.len() > 1 {
            println!(
                "Too many candidates for infer type {}: {{{}}}",
                ty.display(self.db),
                candidates
                    .iter()
                    .map(|ty| ty.display(self.db).to_string())
                    .collect::<Box<[_]>>()
                    .join(", ")
            );
            TypeRef::Error
        } else {
            candidates.into_iter().next().unwrap()
        }
    }

    fn concretize_infos(
        &self,
        infos: InferCallInfos,
        equivalences: &HashMap<InferTy, HashSet<InferTy>>,
    ) -> CallInfos {
        CallInfos {
            expr_id: infos.expr_id,
            callee: infos.callee,
            substitution: infos
                .substitution
                .into_iter()
                .map(|ty| self.normalize_type(&ty, equivalences))
                .collect(),
            variadic: infos.variadic,
        }
    }

    fn finalize(mut self) -> TypeCheckResults<'db> {
        // Solve constraints and populate result
        let equivalences = self.equivalence_classes();
        self.print_equivalence_classes(&equivalences);

        let drain = self.exprs.drain().collect::<Box<[_]>>();
        let node_types = drain
            .into_iter()
            .map(|(id, infer_ty)| (id, self.solve_infer_ty(infer_ty, &equivalences)))
            .collect();

        let call_infos = self
            .calls
            .drain()
            .collect::<Box<[_]>>()
            .into_iter()
            .map(|(id, infos)| (id, self.concretize_infos(infos, &equivalences)))
            .collect();

        TypeCheckResults::new(self.db, node_types, call_infos)
    }

    fn int_ty(&self) -> InferTy {
        InferTy::IntVar(IntVarId::alloc())
    }

    fn char_ty(&self) -> InferTy {
        InferTy::Known(char_id(self.db).into())
    }

    fn str_ty(&self) -> InferTy {
        InferTy::Known(str_id(self.db).into())
    }

    fn bool_ty(&self) -> InferTy {
        InferTy::Known(bool_id(self.db).into())
    }

    fn never_ty(&self) -> InferTy {
        InferTy::Known(never_id(self.db).into())
    }

    fn void_ty(&self) -> InferTy {
        InferTy::Known(void_id(self.db).into())
    }

    fn const_ptr_of(&self, ty: InferTy) -> InferTy {
        match ty {
            InferTy::Known(type_id) => {
                InferTy::Known(ptr_of(self.db, type_id.into(), false).into())
            }
            ty => InferTy::Ptr(Mutability::Immutable, Box::new(ty)),
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
            HirPlace::Deref(hir_place) => todo!(),
            HirPlace::Index { base, index } => todo!(),
            HirPlace::Temporary(hir_expr) => todo!(),
        }
    }

    fn fresh_var() -> InferTy {
        InferTy::Var(TyVarId::alloc())
    }

    fn allocate_ref(&self, ty_ref: TypeRef, templates: &[InferTy]) -> InferTy {
        match ty_ref {
            TypeRef::Concrete(type_id) => InferTy::Known(type_id.into()),
            TypeRef::Param(type_param_id) => templates[type_param_id.0].clone(),
            TypeRef::Error => InferTy::Error,
        }
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

                infered_args.into_iter().zip(ast_args).for_each(|pair| {
                    self.type_eq_constrs.push(pair);
                });

                let return_ty = InferTy::Var(TyVarId::alloc());
                let computed_return_ty =
                    self.allocate_ref(target.ret_ty(self.db), &infer_templates);

                self.type_eq_constrs
                    .push((return_ty.clone(), computed_return_ty));

                let infos = InferCallInfos::new(ExprId(expr.id), target, infer_templates, false);
                self.calls.insert(ExprId(expr.id), infos);
                return_ty
            }
            HirExprDesc::CallMethod { .. } => {
                let l = expr.span.start();
                println!("{:?}", l);
                todo!()
            }
            HirExprDesc::CallStatic { .. } => todo!(),
            HirExprDesc::BinOp { .. } => todo!(),
            HirExprDesc::StructLit { .. } => todo!(),
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
        InferTy::Known(self.function.ret_ty(self.db))
    }

    fn type_check_stmt(&mut self, stmt: &'db HirStmt) {
        match &stmt.kind {
            HirStmtKind::Let { .. } => todo!(),
            HirStmtKind::Match { .. } => todo!(),
            HirStmtKind::Assign { .. } => todo!(),
            HirStmtKind::Expr(expr) => {
                self.type_check_expr(expr);
            }
            HirStmtKind::Return(value) => {
                let ty = value.as_ref().map(|expr| self.type_check_expr(expr));
                if let Some(ty) = ty {
                    self.type_eq_constrs.push((ty, self.get_ret_ty()));
                } else {
                    self.type_eq_constrs
                        .push((self.void_ty(), self.get_ret_ty()));
                }
                // self.type_eq_constrs.push((ty, self.never_ty()));
            }
            HirStmtKind::If { .. } => {
                todo!()
            }
            HirStmtKind::While { .. } => todo!(),
            HirStmtKind::Block(_) => todo!(),
            HirStmtKind::Defer(_) => todo!(),
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

fn type_check_hir<'db>(db: &'db dyn Db, hir: HirBody<'db>) -> TypeCheckResults<'db> {
    TyCtx::new(db, hir.owner(db), hir.locals(db), hir.params(db)).type_check(hir.stmts(db))
}

#[salsa::tracked]
pub fn type_check_function<'db>(
    db: &'db dyn Db,
    function: InternedFunctionId<'db>,
) -> Option<TypeCheckResults<'db>> {
    hir_body(db, function).map(|hir| type_check_hir(db, hir))
}
