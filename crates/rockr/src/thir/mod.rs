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

use crate::common::mutability::Mutability;
use crate::resolved::type_ref::{EnumRef, StructRef};
use crate::{
    Db,
    common::{
        arena::{Arena, Idx},
        location::Span,
        symbols::{StrLit, Symbol},
    },
    hir::{self, function_ast},
    parse_tree::expr::BinaryOperator,
    resolved::{FunctionId, InterfaceRef, TypeRef},
    thir::stmt::ThirStmt,
};

pub mod display;
pub mod stmt;

pub type ExprId = Idx<ThirExpr>;
pub type LocalId = Idx<ThirLocal>;
pub type ScopeId = Idx<ThirScope>;
pub type PlaceId = Idx<ThirPlace>;

pub struct Thir {
    pub id: FunctionId,
    pub places: Arena<ThirPlace>,
    pub exprs: Arena<ThirExpr>,
    pub locals: Arena<ThirLocal>,
    pub params: Vec<LocalId>,
    pub scopes: Arena<ThirScope>,
    pub zelf: Option<LocalId>,
    pub root: Vec<ThirStmt>,
    pub root_scope: ScopeId,
}

pub struct ThirLocal {
    pub ty: TypeRef,
    pub mutability: Mutability,
    pub span: Span,
    pub source: Option<(hir::LocalId, Symbol)>,
    pub is_synthetic: bool,
}

#[derive(Copy, Clone)]
pub enum PlaceBase {
    Local(LocalId),
}

#[derive(Clone)]
pub struct ThirPlace {
    pub base: PlaceBase,
    pub projections: Vec<Projection>,
    pub ty: TypeRef,
    pub span: Span,
    pub is_synthetic: bool,
}

#[derive(Clone, Copy)]
pub struct Projection {
    pub span: Span,
    pub kind: ProjectionKind,
}

#[derive(Clone, Copy)]
pub enum ProjectionKind {
    Deref,
    Field(Symbol, TypeRef),
    TupleField(u32, TypeRef),
    Index(ExprId),
}

pub struct ThirExpr {
    pub kind: ExprKind,
    pub ty: TypeRef,
    pub span: Span,
    pub is_synthetic: bool,
}

// TODO: figure out the right way to do this
#[derive(PartialEq, Eq, Clone, Hash)]
pub struct FunctionRef {
    pub id: FunctionId,
    pub args: Vec<TypeRef>,
    pub self_ty: Option<TypeRef>,
    pub dispatch: Dispatch,
}

#[derive(PartialEq, Eq, Clone, Copy, Hash)]
pub enum Dispatch {
    Direct,
    Interface(InterfaceRef),
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ThirStructField<T> {
    pub field: Symbol,
    pub field_span: Span,
    pub expr: T,
}

pub enum ExprKind {
    // Literals
    IntLit(i64),
    Charlit(char),
    StrLit(StrLit),
    CStrLit(StrLit),
    BoolLit(bool),

    // Places
    Use(PlaceId),
    AddressOf { place: PlaceId, mutability: Mutability },
    Ref { place: PlaceId, mutability: Mutability },
    Call { called: FunctionRef, args: Vec<ExprId> },

    BinOp { op: BinaryOperator, lhs: ExprId, rhs: ExprId },

    StructLit { struct_def: StructRef, fields: Vec<ThirStructField<ExprId>> },

    Neg(ExprId),
    Not(ExprId),

    Tuple(Vec<ExprId>),
    ArrayLit(Vec<ExprId>),
    SizeOf(TypeRef),
    TypeName(TypeRef),

    Constructor { enum_def: EnumRef, idx: usize, args: ThirConstructorArgs<ExprId> },

    Metadata(ExprId),

    Cast(ExprId, TypeRef),

    Error,
    // Todo: Add metadata maybe
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ThirExprWithSetup {
    pub stmts: Vec<ThirStmt>,
    pub expr: ExprId,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum ThirConstructorArgs<T> {
    Tuple(Vec<T>),
    Struct(Vec<ThirStructField<T>>),
    None,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ThirPattern {
    pub kind: ThirPatternKind,
    pub ty: TypeRef,
    pub span: Span,
    pub is_synthetic: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum ThirPatternKind {
    Any,
    Bind { local: LocalId, mutable: bool },
    Tuple(Vec<ThirPattern>),
    Struct { def: StructRef, fields: Vec<(Symbol, ThirPattern)> },
    Constructor { def: EnumRef, idx: usize, args: ThirConstructorArgs<ThirPattern> },
    IntLit(i64),

    Error,
}

pub struct ThirScope {
    pub kind: ScopeKind,
    pub span: Span,
    pub drops: Vec<LocalId>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum ScopeKind {
    Loop,
    Block,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ThirMatchBranch {
    pub pattern: ThirPattern,
    pub guard: Option<ThirExprWithSetup>,
    pub body_scope: ScopeId,
    pub body: Vec<ThirStmt>,
    pub is_synthetic: bool,
}

// Does this match the old behaviour ?

impl PartialEq for Thir {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

fn get_thir_body_span(db: &dyn Db, thir: &Thir) -> Span {
    // We return the body span of the thir without the braces if possible.
    if let Some(fst) = thir.root.first()
        && let Some(lst) = thir.root.last()
    {
        fst.span.start().span(lst.span.end())
    } else {
        function_ast(db, thir.id.interned())
            .inner(db)
            .body_span()
            .unwrap_or_else(|| thir.id.span(db))
    }
}

impl Thir {
    pub fn body_span(&self, db: &dyn Db) -> Span {
        get_thir_body_span(db, self)
    }
}
