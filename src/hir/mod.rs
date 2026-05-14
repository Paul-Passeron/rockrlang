#![allow(dead_code)]

use crate::{
    Db,
    common::{
        location::Span,
        symbols::{StrLit, Symbol},
    },
    hir::lower_fundef::lower_fundef_body,
    name_resolve::{implems::module_impls, module_items},
    parse_tree::{
        expr::BinaryOperator,
        top_level::{AstFundef, AstImplItem, AstMethodDef, AstTopLevelItemDesc},
        type_expr::AstAnyTypeExpr,
    },
    ril::{
        EnumId, FunctionId, InterfaceId, InternedFunctionId, InternedImplId, ScopeOwnerId,
        StructId, TypeDefId, TypeRef,
    },
};

mod display;
mod lower_fundef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    Mutable,
    Immutable,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirExprDesc {
    // Literals
    IntLit(i64),
    CharLit(char),
    StrLit(StrLit),
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
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocalInfo {
    pub id: LocalId,
    pub name: Symbol,
    pub mutability: Mutability,
    pub ty_annotation: Option<AstAnyTypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirBody {
    pub owner: FunctionId,
    pub params: Vec<LocalId>,
    pub locals: Vec<LocalInfo>,
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

impl HirBody {
    pub fn new(
        owner: FunctionId,
        params: Vec<LocalId>,
        locals: Vec<LocalInfo>,
        stmts: Vec<HirStmt>,
    ) -> Self {
        Self {
            owner,
            params,
            locals,
            stmts,
        }
    }
}

#[salsa::tracked]
pub fn impl_items<'db>(db: &'db dyn Db, impl_id: InternedImplId<'db>) -> Vec<AstImplItem> {
    module_impls(db, impl_id.parent(db).interned())
        .into_iter()
        .filter(|impl_| impl_.id(db) == impl_id.into())
        .map(|impl_| impl_.items(db))
        .flatten()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub enum FunctionLikeAst {
    Fundef(AstFundef),
    Method(AstMethodDef),
}

#[salsa::tracked]
pub fn function_ast<'db>(db: &'db dyn Db, function: InternedFunctionId<'db>) -> FunctionLikeAst {
    let parent = function.parent(db);
    match parent {
        ScopeOwnerId::Module(module_id) => {
            let module_items = module_items(db, module_id.interned()).unwrap_or_default();
            for item in module_items {
                if let AstTopLevelItemDesc::Fundef(fdef) = item.data
                    && fdef.data.name == function.name(db)
                {
                    return FunctionLikeAst::Fundef(fdef);
                }
            }
        }
        ScopeOwnerId::Impl(impl_id) => {
            for item in impl_items(db, impl_id.interned()) {
                if let AstImplItem::Fundef(fdef) = item
                    && fdef.data.name == function.name(db)
                {
                    return FunctionLikeAst::Method(fdef);
                }
            }
        }
    }
    panic!("[INTERNAL COMPILER ERROR] FunctionId's Ast not found")
}

#[salsa::tracked]
pub fn hir_body<'db>(db: &'db dyn Db, function: InternedFunctionId<'db>) -> HirBody {
    let ast = function_ast(db, function);

    match ast {
        FunctionLikeAst::Fundef(fundef) => lower_fundef_body(db, function.into(), &fundef),
        FunctionLikeAst::Method(_) => todo!(),
    }
}
