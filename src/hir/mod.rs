#![allow(dead_code)]

use std::sync::Arc;

use crate::{
    Db,
    common::{
        location::Span,
        symbols::{StrLit, Symbol},
    },
    hir::lower_fundef::lower_fundef_body,
    name_resolve::{
        implems::module_impls,
        interfaces::module_interfaces,
        module_items,
        type_expr::{get_templates_of_fun, resolve_type_expr},
    },
    parse_tree::{
        expr::BinaryOperator,
        top_level::{
            AstFundef, AstFundefArg, AstFunsig, AstImplItem, AstInterfaceItem, AstMethodDef,
            AstMethodsig, AstTopLevelItemDesc,
        },
        type_expr::AstAnyTypeExpr,
    },
    ril::{
        EnumId, FunctionId, ImplSource, InterfaceId, InternedFunctionId, InternedImplId,
        InternedInterfaceId, ModuleId, ScopeOwnerId, StructId, TypeDefId, TypeRef,
    },
};

mod display;
mod lower_fundef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HirId(pub u32);

pub struct HirIdAlloc {
    next: u32,
}

impl HirIdAlloc {
    pub fn new() -> Self {
        Self { next: 0 }
    }

    pub fn next(&mut self) -> HirId {
        let id = HirId(self.next);
        self.next += 1;
        id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirPattern {
    pub id: HirId,
    pub data: HirPatternDesc,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirPatternDesc {
    Bind {
        id: LocalId,
        name: Symbol,
        mutable: bool,
    },
    Any,
    Tuple(Vec<HirPattern>),
    DestructureBinding {
        resolution: StructId,
        fields: Vec<(Symbol, Option<HirPattern>)>,
    },
    Constructor {
        resolution: EnumId,
        name: Symbol,
        fields: HirPatternConstructorArgs,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirPlace {
    Local(LocalId),
    Field {
        base: Box<HirPlace>,
        field: Symbol,
    },
    TupleField {
        base: Box<HirPlace>,
        index: u32,
    },
    Deref(Box<HirPlace>),
    Index {
        base: Box<HirPlace>,
        index: Box<HirExpr>,
    },
    Temporary(Box<HirExpr>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirExpr {
    pub id: HirId,
    pub data: HirExprDesc,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mutability {
    Const,
    Mutable,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirExprDesc {
    // Literals
    IntLit(i64),
    CharLit(char),
    StrLit(StrLit),
    CStrLit(StrLit),
    BoolLit(bool),

    // Place-y things
    Use(HirPlace),
    AddressOf {
        place: HirPlace,
        mutability: Mutability,
    },
    Ref {
        place: HirPlace,
        mutability: Mutability,
    },
    // Call-y things
    // Function call which FunctionId is known at lowering time
    CallDirect {
        // Warning, FunctionId is the generic definition of a function, not an instance
        target: FunctionId,
        args: Vec<HirExpr>,
    },

    CallMethod {
        receiver: Box<HirExpr>,
        method: Symbol,
        args: Vec<HirExpr>,

        // None: regular method call    : expr.method(...)
        // Some(Trait)                  : Trait::method(expr, ...)
        interface_hint: Option<InterfaceId>,
    },

    CallStatic {
        ty: PartialTypeRef,
        method: Symbol,
        args: Vec<HirExpr>,
    },

    BinOp {
        lhs: Box<HirExpr>,
        op: BinaryOperator,
        rhs: Box<HirExpr>,
    },

    StructLit {
        ty: PartialTypeRef,
        fields: Vec<(Symbol, HirExpr)>,
    },

    Neg(Box<HirExpr>),
    Not(Box<HirExpr>),
    Tuple(Vec<HirExpr>),
    SliceLit(Vec<HirExpr>),
    SizeOf(PartialTypeRef),
    Constructor {
        enum_def: EnumId,
        name: Symbol,
        args: HirConstructorArgs,
        template_hints: Vec<PartialTypeArg>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PartialTypeRef {
    Resolved(TypeRef),
    WithHoles {
        def: TypeDefId,
        args: Vec<PartialTypeArg>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PartialTypeArg {
    Known(TypeRef),
    Partial(Box<PartialTypeRef>),
    Infer, // Hole
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirStmt {
    pub id: HirId,
    pub kind: HirStmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirMatchBranch {
    pub pattern: HirPattern,
    pub locals: Vec<LocalId>,
    pub guard: Option<HirExpr>,
    pub body: Box<HirStmt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirStmtKind {
    Let {
        pattern: HirPattern,
        locals: Vec<LocalId>,
        ty_annotation: Option<AstAnyTypeExpr>,
        init: HirExpr,
    },
    Match {
        scrutinee: HirExpr,
        branches: Vec<HirMatchBranch>,
    },
    Assign {
        lhs: HirPlace,
        rhs: HirExpr,
    },
    Expr(HirExpr),
    Return(Option<HirExpr>),
    If {
        cond: HirExpr,
        then: Box<HirStmt>,
        else_: Option<Box<HirStmt>>,
    },
    While {
        cond: HirExpr,
        body: Box<HirStmt>,
    },
    Block(Vec<HirStmt>),
    Defer(Box<HirStmt>),
    Break,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalInfo {
    pub id: LocalId,
    pub name: Symbol,
    pub mutability: Mutability,
    pub ty_annotation: Option<AstAnyTypeExpr>,
    pub span: Span,
}

// #[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[salsa::tracked]
pub struct HirBody<'db> {
    pub owner: FunctionId,
    #[returns(ref)]
    pub params: Vec<LocalId>,
    #[returns(ref)]
    pub locals: Vec<LocalInfo>,
    #[returns(ref)]
    pub stmts: Vec<HirStmt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirConstructorArgs {
    TupleLike(Vec<HirExpr>),
    StructLike { fields: Vec<(Symbol, HirExpr)> },
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirPatternConstructorArgs {
    None,
    StructFields(Vec<HirStructFieldPattern>),
    TupleFields(Vec<HirPattern>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HirStructFieldPattern {
    Rebind { name: Symbol, pattern: HirPattern },
    Name { id: LocalId, name: Symbol },
}

#[salsa::tracked]
pub fn impl_sources<'db>(db: &'db dyn Db, impl_id: InternedImplId<'db>) -> Vec<ImplSource<'db>> {
    module_impls(db, impl_id.parent(db).interned())
        .into_iter()
        .filter(|impl_| impl_.id(db) == impl_id.into())
        .collect()
}

#[salsa::tracked]
pub fn impl_items<'db>(db: &'db dyn Db, impl_id: InternedImplId<'db>) -> Vec<AstImplItem> {
    impl_sources(db, impl_id)
        .into_iter()
        .flat_map(|impl_| impl_.items(db))
        .collect()
}

#[salsa::tracked]
pub fn interface_items<'db>(
    db: &'db dyn Db,
    interface_id: InternedInterfaceId<'db>,
) -> Arc<Vec<AstInterfaceItem>> {
    Arc::new(
        module_interfaces(db, interface_id.parent(db).interned())
            .iter()
            .find(|interface| interface.name == interface_id.name(db))
            .cloned()
            .into_iter()
            .flat_map(|interface| interface.items)
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub enum FunctionLikeAst {
    ExternDef(AstFunsig, bool),
    Fundef(AstFundef),
    Method(AstMethodDef),
    TraitMethod(AstMethodsig),
}

#[salsa::tracked]
pub struct InternedFunctionLikeAst<'db> {
    #[returns(ref)]
    pub inner: FunctionLikeAst,
}

#[salsa::tracked]
pub fn function_ast<'db>(
    db: &'db dyn Db,
    function: InternedFunctionId<'db>,
) -> InternedFunctionLikeAst<'db> {
    let parent = function.parent(db);
    match parent {
        ScopeOwnerId::Module(module_id) => {
            let module_items = module_items(db, module_id.interned()).unwrap_or_default();
            for item in module_items {
                match item.data {
                    AstTopLevelItemDesc::Fundef(fdef) if fdef.data.name == function.name(db) => {
                        return InternedFunctionLikeAst::new(db, FunctionLikeAst::Fundef(fdef));
                    }
                    AstTopLevelItemDesc::ExternDef(fsig, variadic)
                        if fsig.data.name == function.name(db) =>
                    {
                        return InternedFunctionLikeAst::new(
                            db,
                            FunctionLikeAst::ExternDef(fsig, variadic),
                        );
                    }
                    _ => (),
                }
            }
        }
        ScopeOwnerId::Impl(impl_id) => {
            for item in impl_items(db, impl_id.interned()) {
                if let AstImplItem::Fundef(fdef) = item
                    && fdef.data.name == function.name(db)
                {
                    return InternedFunctionLikeAst::new(db, FunctionLikeAst::Method(fdef));
                }
            }
        }
        ScopeOwnerId::Interface(interface_ref) => {
            for item in interface_items(db, interface_ref.def(db).interned()).iter() {
                match item {
                    AstInterfaceItem::Sig(sig) => {
                        return InternedFunctionLikeAst::new(
                            db,
                            FunctionLikeAst::TraitMethod(sig.clone()),
                        );
                    }
                    _ => (),
                }
            }
        }
    }
    panic!("[INTERNAL COMPILER ERROR] FunctionId's Ast not found")
}

#[salsa::tracked]
pub fn hir_body<'db>(db: &'db dyn Db, function: InternedFunctionId<'db>) -> Option<HirBody<'db>> {
    let ast = function_ast(db, function);

    match ast.inner(db) {
        FunctionLikeAst::Fundef(fundef) => Some(lower_fundef_body(db, function.into(), fundef)),
        FunctionLikeAst::Method(_) => todo!(),
        FunctionLikeAst::ExternDef(_, _) => None,
        FunctionLikeAst::TraitMethod(_) => None,
    }
}

pub fn owning_module(db: &dyn Db, owner: ScopeOwnerId) -> ModuleId {
    match owner {
        ScopeOwnerId::Module(module_id) => module_id,
        ScopeOwnerId::Impl(impl_id) => impl_id.parent(db),
        ScopeOwnerId::Interface(interface_ref) => interface_ref.def(db).parent(db),
    }
}

impl FunctionId {
    pub fn ret_ty<'db>(&'db self, db: &'db dyn Db) -> TypeRef {
        let templates = get_templates_of_fun(db, self.interned());
        let owning_module = owning_module(db, self.parent(db));
        let ast = function_ast(db, self.interned()).inner(db);
        let (type_expr, has_zelf) = match ast {
            FunctionLikeAst::ExternDef(spanned, _) => (&spanned.data.return_type, false),
            FunctionLikeAst::Fundef(spanned) => (&spanned.data.return_type, false),
            FunctionLikeAst::Method(spanned) => (&spanned.data.return_type, true),
            FunctionLikeAst::TraitMethod(spanned) => (&spanned.data.return_type, true),
        };
        match resolve_type_expr(
            db,
            type_expr,
            owning_module.interned(),
            &templates,
            has_zelf,
        ) {
            crate::name_resolve::type_expr::TypeResolution::Type(type_ref) => type_ref,
            _ => panic!("Unresolved type in function {}", self.name(db).display(db)),
        }
    }

    pub fn args<'db>(&'db self, db: &'db dyn Db) -> (Option<TypeRef>, Vec<AstFundefArg>) {
        let ast = function_ast(db, self.interned()).inner(db);
        match ast {
            FunctionLikeAst::ExternDef(spanned, _) => (None, spanned.data.args.clone()),
            FunctionLikeAst::Fundef(spanned) => (None, spanned.data.args.clone()),
            FunctionLikeAst::Method(spanned) => {
                let receiver = match self.parent(db) {
                    ScopeOwnerId::Impl(impl_id) => impl_id.implemented(db),
                    _ => unreachable!(),
                };

                (Some(receiver), spanned.data.args.clone())
            }
            FunctionLikeAst::TraitMethod(spanned) => {
                // TODO: receiver is supposed to be Self type
                (None, spanned.data.args.clone())
            }
        }
    }
}
