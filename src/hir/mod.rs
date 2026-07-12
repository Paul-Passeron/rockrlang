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

#![allow(dead_code)]

use std::sync::Arc;

use crate::{
    Db,
    common::{
        arena,
        location::Span,
        symbols::{StrLit, Symbol},
    },
    hir::lower_fundef::{lower_fundef_body, lower_method_body},
    name_resolve::{
        implems::module_impls,
        interfaces::module_interfaces,
        module_items,
        type_expr::{get_templates_of_fun, resolve_type_expr},
    },
    parse_tree::{
        expr::BinaryOperator,
        top_level::{
            AstFundef, AstFundefArg, AstFunsig, AstImplItem, AstInterfaceItem,
            AstMethodDef, AstMethodsig, AstReceiver, AstTopLevelItemDesc,
        },
        type_expr::{AstAnyTypeExpr, AstTypeExpr},
    },
    ril::{
        EnumId, FunctionId, ImplSource, InterfaceId, InternedFunctionId,
        InternedImplId, InternedInterfaceId, ModuleId, ScopeOwnerId, StructId,
        TypeDefId, TypeRef,
    },
};

pub mod display;
pub mod lower_fundef;
pub mod utils;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HirId(pub usize);

impl From<usize> for HirId {
    fn from(value: usize) -> Self {
        Self(value)
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
        fields: Vec<HirStructFieldPattern>,
    },
    Constructor {
        resolution: EnumId,
        name: Symbol,
        fields: HirPatternConstructorArgs,
    },
    IntLit(i64),
    Error,
}

pub type LocalId = arena::Idx<LocalInfo>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HirPlace {
    pub id: HirId,
    pub kind: HirPlaceKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HirPlaceKind {
    Local(LocalId),
    Field { base: Box<HirPlace>, field: Symbol },
    TupleField { base: Box<HirPlace>, index: u32 },
    Deref(Box<HirPlace>),
    Index { base: Box<HirPlace>, index: Box<HirExpr> },
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
impl Mutability {
    pub fn is_mut(&self) -> bool {
        matches!(self, Self::Mutable)
    }
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
        // Warning, FunctionId is the generic definition of a function, not an
        // instance
        target: FunctionId,
        args: Vec<HirExpr>,
    },

    UnresolvedCallDirect {
        // The function id here is not valid
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

    Metadata(Box<HirExpr>),

    As {
        expr: Box<HirExpr>,
        ty: PartialTypeRef,
    },

    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PartialTypeRef {
    Resolved(TypeRef),
    WithHoles { def: TypeDefId, args: Vec<PartialTypeArg> },
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
    pub zelf: Option<LocalId>,
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
pub fn impl_sources<'db>(
    db: &'db dyn Db,
    impl_id: InternedImplId<'db>,
) -> Vec<ImplSource<'db>> {
    module_impls(db, impl_id.parent(db).interned())
        .iter()
        .filter(|impl_| *impl_.id(db) == impl_id.into())
        .copied()
        .collect()
}

#[salsa::tracked]
pub fn impl_items<'db>(
    db: &'db dyn Db,
    impl_id: InternedImplId<'db>,
) -> Vec<AstImplItem> {
    impl_sources(db, impl_id)
        .iter()
        .flat_map(|impl_| impl_.items(db).clone())
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
            .find(|interface| interface.name.data == *interface_id.name(db))
            .cloned()
            .into_iter()
            .flat_map(|interface| interface.items)
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq, Hash)]
pub enum FunctionLikeAst {
    ExternDef(Arc<AstFunsig>, bool),
    Fundef(Arc<AstFundef>),
    Method(Arc<AstMethodDef>),
    TraitMethod(Arc<AstMethodsig>),
}

impl FunctionLikeAst {
    pub fn get_span(&self) -> Span {
        match self {
            FunctionLikeAst::ExternDef(spanned, _) => spanned.span,
            FunctionLikeAst::Fundef(spanned) => spanned.span,
            FunctionLikeAst::Method(spanned) => spanned.span,
            FunctionLikeAst::TraitMethod(spanned) => spanned.span,
        }
    }

    pub fn body_span(&self) -> Option<Span> {
        match self {
            FunctionLikeAst::Fundef(spanned) => Some(spanned.data.body_span),
            FunctionLikeAst::Method(spanned) => Some(spanned.data.body_span),
            _ => None,
        }
    }

    pub fn get_args(&self) -> &[AstFundefArg] {
        match self {
            FunctionLikeAst::ExternDef(spanned, _) => &spanned.data.args,
            FunctionLikeAst::Fundef(spanned) => &spanned.data.args,
            FunctionLikeAst::Method(spanned) => &spanned.data.args,
            FunctionLikeAst::TraitMethod(spanned) => &spanned.data.args,
        }
    }

    pub fn receiver(&self) -> Option<AstReceiver> {
        match self {
            FunctionLikeAst::ExternDef(_, _) => None,
            FunctionLikeAst::Fundef(_) => None,
            FunctionLikeAst::Method(spanned) => {
                Some(spanned.data.receiver.clone())
            }
            FunctionLikeAst::TraitMethod(spanned) => {
                Some(spanned.data.receiver.clone())
            }
        }
    }

    pub fn get_ret(&self) -> &AstTypeExpr {
        match self {
            FunctionLikeAst::ExternDef(spanned, _) => &spanned.data.return_type,
            FunctionLikeAst::Fundef(spanned) => &spanned.data.return_type,
            FunctionLikeAst::Method(spanned) => &spanned.data.return_type,
            FunctionLikeAst::TraitMethod(spanned) => &spanned.data.return_type,
        }
    }

    pub fn has_body(&self) -> bool {
        !matches!(
            self,
            FunctionLikeAst::ExternDef(_, _) | FunctionLikeAst::TraitMethod(_)
        )
    }
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
            for item in module_items(db, module_id.interned())
                .as_ref()
                .into_iter()
                .flatten()
            {
                match &item.data {
                    AstTopLevelItemDesc::Fundef(fdef)
                        if fdef.data.name.data == *function.name(db) =>
                    {
                        return InternedFunctionLikeAst::new(
                            db,
                            FunctionLikeAst::Fundef(Arc::new(fdef.clone())),
                        );
                    }
                    AstTopLevelItemDesc::ExternDef(fsig, variadic)
                        if fsig.data.name.data == *function.name(db) =>
                    {
                        return InternedFunctionLikeAst::new(
                            db,
                            FunctionLikeAst::ExternDef(
                                Arc::new(fsig.clone()),
                                *variadic,
                            ),
                        );
                    }
                    _ => (),
                }
            }
        }
        ScopeOwnerId::Impl(impl_id) => {
            for item in impl_items(db, impl_id.interned()) {
                if let AstImplItem::Fundef(fdef) = item
                    && fdef.data.name.data == *function.name(db)
                {
                    return InternedFunctionLikeAst::new(
                        db,
                        FunctionLikeAst::Method(Arc::new(
                            fdef.as_ref().clone(),
                        )),
                    );
                }
            }
        }
        ScopeOwnerId::Interface(interface_ref) => {
            for item in
                interface_items(db, interface_ref.def(db).interned()).iter()
            {
                if let AstInterfaceItem::Sig(sig) = item {
                    return InternedFunctionLikeAst::new(
                        db,
                        FunctionLikeAst::TraitMethod(sig.clone()),
                    );
                }
            }
        }
    }
    panic!("[INTERNAL COMPILER ERROR] FunctionId's Ast not found")
}

pub fn hir_body<'db>(
    db: &'db dyn Db,
    function: FunctionId,
) -> Option<HirBody<'db>> {
    _hir_body(db, function.interned())
}

#[salsa::tracked(returns(copy))]
fn _hir_body<'db>(
    db: &'db dyn Db,
    function: InternedFunctionId<'db>,
) -> Option<HirBody<'db>> {
    let ast = function_ast(db, function);

    match ast.inner(db) {
        FunctionLikeAst::Fundef(fundef) => {
            Some(lower_fundef_body(db, function.into(), fundef))
        }
        FunctionLikeAst::Method(methoddef) => {
            Some(lower_method_body(db, function.into(), methoddef))
        }
        FunctionLikeAst::ExternDef(_, _) => None,
        FunctionLikeAst::TraitMethod(_) => None,
    }
}

pub fn owning_module(db: &dyn Db, owner: ScopeOwnerId) -> ModuleId {
    match owner {
        ScopeOwnerId::Module(module_id) => module_id,
        ScopeOwnerId::Impl(impl_id) => impl_id.parent(db),
        ScopeOwnerId::Interface(interface_ref) => {
            interface_ref.def(db).parent(db)
        }
    }
}

impl FunctionId {
    pub fn receiver(self, db: &dyn Db) -> AstReceiver {
        let ast = function_ast(db, self.interned());
        match ast.inner(db) {
            FunctionLikeAst::Fundef(_) | FunctionLikeAst::ExternDef(_, _) => {
                AstReceiver::None
            }
            FunctionLikeAst::Method(spanned) => spanned.data.receiver.clone(),
            FunctionLikeAst::TraitMethod(spanned) => {
                spanned.data.receiver.clone()
            }
        }
    }

    pub fn ret_ty<'db>(&'db self, db: &'db dyn Db) -> TypeRef {
        let templates = get_templates_of_fun(db, self.interned());
        let owning_module = owning_module(db, self.parent(db));
        let ast = function_ast(db, self.interned()).inner(db);
        let (type_expr, has_zelf) = match ast {
            FunctionLikeAst::ExternDef(spanned, _) => {
                (&spanned.data.return_type, false)
            }
            FunctionLikeAst::Fundef(spanned) => {
                (&spanned.data.return_type, false)
            }
            FunctionLikeAst::Method(spanned) => {
                (&spanned.data.return_type, true)
            }
            FunctionLikeAst::TraitMethod(spanned) => {
                (&spanned.data.return_type, true)
            }
        };
        match resolve_type_expr(
            db,
            type_expr,
            owning_module.interned(),
            templates,
            has_zelf,
        ) {
            crate::name_resolve::type_expr::TypeResolution::Type(type_ref) => {
                type_ref
            }
            _ => panic!(
                "{}: Unresolved type in function {}",
                type_expr.span.start().loc_info(db),
                self.name(db).display(db)
            ),
        }
    }

    pub fn args<'db>(
        &'db self,
        db: &'db dyn Db,
    ) -> (Option<TypeRef>, Vec<AstFundefArg>) {
        let ast = function_ast(db, self.interned()).inner(db);
        match ast {
            FunctionLikeAst::ExternDef(spanned, _) => {
                (None, spanned.data.args.clone())
            }
            FunctionLikeAst::Fundef(spanned) => {
                (None, spanned.data.args.clone())
            }
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

    pub fn has_body(self, db: &dyn Db) -> bool {
        !matches!(
            function_ast(db, self.interned()).inner(db),
            FunctionLikeAst::ExternDef(_, _) | FunctionLikeAst::TraitMethod(_)
        )
    }

    pub fn is_var_args(self, db: &dyn Db) -> bool {
        match function_ast(db, self.interned()).inner(db) {
            FunctionLikeAst::ExternDef(_, var_arg) => *var_arg,
            _ => false,
        }
    }
}
