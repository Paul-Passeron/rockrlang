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

use std::{fmt, sync::Arc};

use itertools::Itertools;
use nonempty::NonEmpty;

use crate::{
    Db,
    common::{location::Span, symbols::Symbol},
    parse_tree::{
        Spanned,
        expr::AstExpr,
        pattern::AstPattern,
        stmt::AstStmt,
        type_expr::{AstTypeExpr, AstTypeExprDesc},
    },
    ril::display::Display,
};

pub type AstTopLevelItem = Spanned<AstTopLevelItemDesc>;
pub type AstAnyTopLevelItem = Spanned<AstAnyTopLevelItemDesc>;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum AstAnyTopLevelItemDesc {
    Include(AstIncludePath),
    Item(Box<AstTopLevelItemDesc>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AstTopLevelItemDesc {
    Module(AstModule),
    Fundef(AstFundef),
    Interface(AstInterface),
    // Const(AstConstDecl),
    Impl(AstImplBlock),
    StructDef(AstStructDef),
    EnumDef(AstEnumDef),
    ExternDef(AstFunsig, bool), // true means variadic
}

pub type AstModule = Spanned<AstModuleDesc>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstModuleDesc {
    pub name: Spanned<Symbol>,
    pub items: Vec<AstTopLevelItem>,
    pub includes: Vec<AstIncludePath>,
}

pub type AstFundef = Spanned<AstFundefDesc>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstFundefDesc {
    pub name: Spanned<Symbol>,
    pub args: Vec<AstFundefArg>,
    pub template_args: Vec<AstTemplateArg>,
    pub return_type: AstTypeExpr,
    pub body_span: Span,
    pub body: Vec<AstStmt>,
}

pub type AstMethodDef = Spanned<AstMethodDefDesc>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstMethodDefDesc {
    pub name: Spanned<Symbol>,
    pub receiver: AstReceiver,
    pub args: Vec<AstFundefArg>,
    pub template_args: Vec<AstTemplateArg>,
    pub return_type: AstTypeExpr,
    pub body_span: Span,
    pub body: Vec<AstStmt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AstReceiver {
    None,             // No receiver, static method
    Zelf(Span),       // self
    MutZelf(Span),    // mut self
    RefZelf(Span),    // &self
    MutRefZelf(Span), // &mut self
    PtrZelf(Span),    // *self
    MutPtrZelf(Span), // *mut self
}

impl AstReceiver {
    pub fn is_static(&self) -> bool {
        matches!(self, AstReceiver::None)
    }
}

impl fmt::Display for AstReceiver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AstReceiver::None => Ok(()),
            AstReceiver::Zelf(_) => write!(f, "self"),
            AstReceiver::MutZelf(_) => write!(f, "mut self"),
            AstReceiver::RefZelf(_) => write!(f, "&self"),
            AstReceiver::MutRefZelf(_) => write!(f, "&mut self"),
            AstReceiver::PtrZelf(_) => write!(f, "*self"),
            AstReceiver::MutPtrZelf(_) => write!(f, "*mut self"),
        }
    }
}

pub type AstFunsig = Spanned<AstFunsigDesc>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstFunsigDesc {
    pub name: Spanned<Symbol>,
    pub args: Vec<AstFundefArg>,
    pub template_args: Vec<AstTemplateArg>,
    pub return_type: AstTypeExpr,
}

pub type AstMethodsig = Spanned<AstMethodsigDesc>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstMethodsigDesc {
    pub name: Spanned<Symbol>,
    pub receiver: AstReceiver,
    pub args: Vec<AstFundefArg>,
    pub template_args: Vec<AstTemplateArg>,
    pub return_type: AstTypeExpr,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstTemplateArg {
    pub name: Symbol,
    pub constraints: Vec<AstTypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstFundefArg {
    pub name: Symbol,
    pub ty: AstTypeExpr,
    pub span: Span,
}

impl AstTypeExprDesc {
    pub fn display<'a, 'b>(&'a self, db: &'b dyn Db) -> Display<'b, &'a Self> {
        Display { value: self, db }
    }
}

impl<'a, 'b> fmt::Display for Display<'b, &'a AstFundefArg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}",
            self.value.name.display(self.db),
            self.value.ty.data.display(self.db)
        )
    }
}

impl<'a, 'b> fmt::Display for Display<'b, &'a AstTypeExprDesc> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            AstTypeExprDesc::Named { name, args } => {
                write!(f, "{}", name.display(self.db))?;
                if !args.is_empty() {
                    write!(
                        f,
                        "<{}>",
                        args.iter()
                            .map(|arg| {
                                arg.as_known().map_or(String::from("_"), |ty| {
                                    ty.data.display(self.db).to_string()
                                })
                            })
                            .collect_vec()
                            .join(", ")
                    )?;
                }
                Ok(())
            }
            AstTypeExprDesc::NameResolved { from, to } => {
                write!(
                    f,
                    "{}::{}",
                    from.display(self.db),
                    to.data.display(self.db)
                )
            }
            AstTypeExprDesc::Ref { mutable, pointee } => {
                write!(
                    f,
                    "&{}{}",
                    if *mutable { "mut " } else { "" },
                    pointee.data.display(self.db)
                )
            }
            AstTypeExprDesc::Pointer { mutable, pointee } => {
                write!(
                    f,
                    "*{}{}",
                    if *mutable { "mut " } else { "" },
                    pointee.data.display(self.db)
                )
            }
            AstTypeExprDesc::Slice { ty, len } => {
                write!(
                    f,
                    "[{}{}]",
                    ty.data.display(self.db),
                    if let Some(len) = len {
                        format!("; {len}")
                    } else {
                        String::new()
                    }
                )
            }
            AstTypeExprDesc::Tuple(spanneds) => {
                write!(
                    f,
                    "({})",
                    spanneds
                        .iter()
                        .map(|ty| ty.data.display(self.db).to_string())
                        .collect_vec()
                        .join(", ")
                )
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AstInterface {
    pub name: Spanned<Symbol>,
    pub supers: Vec<AstTypeExpr>,
    pub template_args: Vec<AstTemplateArg>,
    pub items: Vec<AstInterfaceItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AstInterfaceItem {
    Type(AstTemplateArg),
    Sig(Arc<AstMethodsig>),
}

#[allow(dead_code)]
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
    Type { name: Symbol, name_span: Span, ty: AstTypeExpr },
    Fundef(Box<AstMethodDef>),
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
    NameResolved { from: Symbol, to: Box<AstIncludePath> },
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct AstStructDef {
    pub name: Spanned<Symbol>,
    pub template_args: Vec<AstTemplateArg>,
    pub fields: Vec<AstStructDefField>,
    pub span: Span,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct AstEnumDef {
    pub name: Spanned<Symbol>,
    pub template_args: Vec<AstTemplateArg>,
    pub variants: Vec<AstEnumVariant>,
    pub span: Span,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct AstEnumVariant {
    pub name: Symbol,
    pub kind: AstEnumVariantKind,
    pub span: Span,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum AstEnumVariantKind {
    Unit,
    StructLike(Vec<AstStructDefField>),
    TupleLike(Vec<AstTypeExpr>),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct AstStructDefField {
    pub name: Symbol,
    pub ty: AstTypeExpr,
    pub span: Span,
}

impl From<NonEmpty<Spanned<Symbol>>> for AstIncludePath {
    fn from(value: NonEmpty<Spanned<Symbol>>) -> Self {
        let mut symbols = value.into_iter().collect::<Vec<_>>();
        let Spanned { data: symbol, span, .. } = symbols.pop().unwrap();
        let start_loc = span.start();
        symbols.reverse();

        symbols.into_iter().fold(
            AstIncludePath::new(
                AstIncludePathDesc::Symbol(symbol),
                vec![],
                span,
            ),
            |acc, symb| {
                let total_span = start_loc.span(symb.span.end());
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
