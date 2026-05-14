use nonempty::NonEmpty;

use crate::{
    common::{location::Span, symbols::Symbol},
    parse_tree::{
        Spanned, expr::AstExpr, pattern::AstPattern, stmt::AstStmt, type_expr::AstTypeExpr,
    },
};

pub type AstTopLevelItem = Spanned<AstTopLevelItemDesc>;
pub type AstAnyTopLevelItem = Spanned<AstAnyTopLevelItemDesc>;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum AstAnyTopLevelItemDesc {
    Include(AstIncludePath),
    Item(AstTopLevelItemDesc),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AstTopLevelItemDesc {
    Module(AstModule),
    Fundef(AstFundef),
    Interface(AstInterface),
    Const(AstConstDecl),
    Impl(AstImplBlock),
    StructDef(AstStructDef),
}

pub type AstModule = Spanned<AstModuleDesc>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstModuleDesc {
    pub name: Symbol,
    pub items: Vec<AstTopLevelItem>,
    pub includes: Vec<AstIncludePath>,
}

pub type AstFundef = Spanned<AstFundefDesc>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstFundefDesc {
    pub name: Symbol,
    pub args: Vec<AstFundefArg>,
    pub template_args: Vec<AstTemplateArg>,
    pub return_type: AstTypeExpr,
    pub body: Vec<AstStmt>,
}

pub type AstMethodDef = Spanned<AstMethodDefDesc>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstMethodDefDesc {
    pub name: Symbol,
    pub receiver: AstReceiver,
    pub args: Vec<AstFundefArg>,
    pub template_args: Vec<AstTemplateArg>,
    pub return_type: AstTypeExpr,
    pub body: Vec<AstStmt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstReceiver {
    None,       // No receiver, static method
    Zelf,       // self
    RefZelf,    // &self
    MutRefZelf, // &mut self
    PtrZelf,    // *self
    MutPtrZelf, // *mut self
}

pub type AstFunsig = Spanned<AstFunsigDesc>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstFunsigDesc {
    pub name: Symbol,
    pub args: Vec<AstFundefArg>,
    pub template_args: Vec<AstTemplateArg>,
    pub return_type: AstTypeExpr,
}

pub type AstMethodsig = Spanned<AstMethodsigDesc>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstMethodsigDesc {
    pub name: Symbol,
    pub receiver: AstReceiver,
    pub args: Vec<AstFundefArg>,
    pub template_args: Vec<AstTemplateArg>,
    pub return_type: AstTypeExpr,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstTemplateArg {
    pub name: Symbol,
    pub constraints: Vec<AstTypeExpr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstFundefArg {
    pub name: Symbol,
    pub ty: AstTypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstInterface {
    pub name: Symbol,
    pub supers: Vec<AstTypeExpr>,
    pub template_args: Vec<AstTemplateArg>,
    pub items: Vec<AstInterfaceItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AstInterfaceItem {
    Type(AstTemplateArg),
    Sig(AstMethodsig),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstConstDecl {
    pub pat: AstPattern,
    pub ty: AstTypeExpr,
    pub value: AstExpr,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstImplBlock {
    pub template_args: Vec<AstTemplateArg>,

    pub interface: Option<AstTypeExpr>, // Interface being implemented
    pub implemented: AstTypeExpr,       // Type being implemented for
    pub items: Vec<AstImplItem>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AstImplItem {
    Type { name: Symbol, ty: AstTypeExpr },
    Fundef(AstMethodDef),
}

#[salsa::tracked]
pub struct Ast<'db> {
    #[returns(ref)]
    pub items: Vec<AstTopLevelItem>,
    #[returns(ref)]
    pub includes: Vec<AstIncludePath>,
}

pub type AstIncludePath = Spanned<AstIncludePathDesc>;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum AstIncludePathDesc {
    Symbol(Symbol),
    NameResolved {
        from: Symbol,
        to: Box<AstIncludePath>,
    },
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct AstStructDef {
    pub name: Symbol,
    pub template_args: Vec<AstTemplateArg>,
    pub fields: Vec<AstStructDefField>,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct AstStructDefField {
    pub name: Symbol,
    pub ty: AstTypeExpr,
}

impl From<NonEmpty<Spanned<Symbol>>> for AstIncludePath {
    fn from(value: NonEmpty<Spanned<Symbol>>) -> Self {
        let mut symbols = value.into_iter().collect::<Vec<_>>();
        let Spanned {
            data: symbol, span, ..
        } = symbols.pop().unwrap();
        let start_loc = span.start();
        symbols.reverse();

        symbols.into_iter().fold(
            AstIncludePath::new(AstIncludePathDesc::Symbol(symbol), vec![], span),
            |acc, symb| {
                let total_span = start_loc.span(&symb.span.end());
                AstIncludePath::new(
                    AstIncludePathDesc::NameResolved {
                        from: symb.data,
                        to: Box::new(acc),
                    },
                    vec![],
                    total_span,
                )
            },
        )
    }
}
