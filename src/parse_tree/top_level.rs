use nonempty::NonEmpty;

use crate::{
    common::symbols::Symbol,
    parse_tree::{Spanned, expr::Expr, pattern::Pattern, stmt::Stmt, type_expr::TypeExpr},
};

pub type TopLevelItem = Spanned<TopLevelItemDesc>;
pub type AnyTopLevelItem = Spanned<AnyTopLevelItemDesc>;

#[derive(PartialEq, Eq, Hash, Debug)]
pub enum AnyTopLevelItemDesc {
    Include(IncludePath),
    Item(TopLevelItemDesc),
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum TopLevelItemDesc {
    Module(Module),
    Fundef(Fundef),
    Interface(Interface),
    Const(ConstDecl),
    Impl(ImplBlock),
}

pub type Module = Spanned<ModuleDesc>;

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ModuleDesc {
    pub name: Symbol,
    pub items: Vec<TopLevelItem>,
}

pub type Fundef = Spanned<FundefDesc>;

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct FundefDesc {
    pub name: Symbol,
    pub args: Vec<FundefArg>,
    pub template_args: Vec<TemplateArg>,
    pub return_type: TypeExpr,
    pub body: Vec<Stmt>,
}

pub type Funsig = Spanned<FunsigDesc>;

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct FunsigDesc {
    pub name: Symbol,
    pub args: Vec<FundefArg>,
    pub template_args: Vec<TemplateArg>,
    pub return_type: TypeExpr,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct TemplateArg {
    pub name: Symbol,
    pub constraints: Vec<TypeExpr>,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct FundefArg {
    pub name: Symbol,
    pub ty: TypeExpr,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Interface {
    pub name: Symbol,
    pub template_args: Vec<TemplateArg>,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ConstDecl {
    pub pat: Pattern,
    pub ty: TypeExpr,
    pub value: Expr,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ImplBlock {
    template_args: Vec<TemplateArg>,
    interface: Option<TypeExpr>, // Interface being implemented
    implemented: TypeExpr,       // Type being implemented for
    items: Vec<ImplItem>,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum ImplItem {
    Type { name: Symbol, ty: TypeExpr },
    Fundef(Fundef),
}

#[salsa::tracked]
pub struct Ast<'db> {
    #[returns(ref)]
    pub items: Vec<AnyTopLevelItem>,
}

pub type IncludePath = Spanned<IncludePathDesc>;

#[derive(PartialEq, Eq, Hash, Debug)]
pub enum IncludePathDesc {
    Symbol(Symbol),
    NameResolved { from: Symbol, to: Box<IncludePath> },
}

impl From<NonEmpty<Spanned<Symbol>>> for IncludePath {
    fn from(value: NonEmpty<Spanned<Symbol>>) -> Self {
        let mut symbols = value.into_iter().collect::<Vec<_>>();
        let Spanned { data: symbol, span } = symbols.pop().unwrap();
        let start_loc = span.start();
        symbols.reverse();

        symbols.into_iter().fold(
            IncludePath::new(IncludePathDesc::Symbol(symbol), span),
            |acc, symb| {
                let total_span = start_loc.span(&symb.span.end());
                IncludePath::new(
                    IncludePathDesc::NameResolved {
                        from: symb.data,
                        to: Box::new(acc),
                    },
                    total_span,
                )
            },
        )
    }
}
