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

use crate::{
    common::{
        arena::{Arena, Idx},
        location::Span,
        symbols::{StrLit, Symbol},
    },
    hir::{self, Mutability},
    parse_tree::expr::BinaryOperator,
    ril::{EnumId, FunctionId, InterfaceRef, StructId, TypeRef},
};

pub type ExprId = Idx<ThirExpr>;
pub type LocalId = Idx<ThirLocal>;
pub type ScopeId = Idx<ThirScope>;

pub struct Thir {
    pub places: Arena<ThirPlace>,
    pub exprs: Arena<ThirExpr>,
    pub locals: Arena<ThirLocal>,
    pub params: Vec<LocalId>,
    pub scopes: Arena<ThirScope>,
    pub zelf: Option<LocalId>,
    pub root: Vec<ThirStmt>,
}

pub struct ThirLocal {
    pub ty: TypeRef,
    pub mutability: Mutability,
    pub span: Span,
    pub source: Option<(hir::LocalId, Symbol)>,
}

pub enum PlaceBase {
    Local(LocalId),
}

pub struct ThirPlace {
    pub base: PlaceBase,
    pub projections: Vec<Projection>,
    pub ty: TypeRef,
    pub span: Span,
}

pub enum Projection {
    Deref,
    Field(Symbol, TypeRef),
    TupleField(u32, TypeRef),
    Index(ExprId),
}

pub struct ThirExpr {
    pub kind: ExprKind,
    pub ty: TypeRef,
    pub span: Span,
}

pub struct ThirStmt {
    pub kind: StmtKind,
    pub span: Span,
}

// TODO: figure out the right way to do this
pub struct FunctionRef {
    pub id: FunctionId,
    pub args: Vec<TypeRef>,
    pub self_ty: Option<TypeRef>,
    pub dispatch: Dispatch,
}

pub enum Dispatch {
    Direct,
    Interface(InterfaceRef),
}

pub struct StructRef {
    pub def: StructId,
    pub args: Vec<TypeRef>,
}

pub struct EnumRef {
    pub def: EnumId,
    pub args: Vec<TypeRef>,
}

pub enum ExprKind {
    // Literals
    IntLit(i64),
    Charlit(char),
    StrLit(StrLit),
    CStrLit(StrLit),
    BoolLit(bool),

    // Places
    Use(Idx<ThirPlace>),
    AddressOf {
        place: Idx<ThirPlace>,
        mutability: Mutability,
    },
    Ref {
        place: Idx<ThirPlace>,
        mutability: Mutability,
    },
    Call {
        called: FunctionRef,
        args: Vec<ExprId>,
    },

    BinOp {
        op: BinaryOperator,
        lhs: ExprId,
        rhs: ExprId,
    },

    StructLit {
        struct_def: StructRef,
        fields: Vec<(Symbol, ExprId)>,
    },

    Neg(ExprId),
    Not(ExprId),

    Tuple(Vec<ExprId>),
    SliceLit(Vec<ExprId>),
    Sizeof(TypeRef),

    Constructor {
        enum_def: EnumRef,
        idx: usize,
        args: ThirConstructorArgs,
    },

    Error, // Todo: Add metadata maybe
}

pub enum ThirConstructorArgs {
    Tuple(Vec<ExprId>),
    Struct(Vec<(Symbol, ExprId)>),
    None,
}

pub enum StmtKind {
    Block {
        scope: ScopeId,
        stmts: Vec<ThirStmt>,
    },
    If {
        cond: ExprId,
        then: Vec<ThirStmt>,
        then_scope: ScopeId,
        else_: Option<Vec<ThirStmt>>,
        else_scope: Option<ScopeId>,
    },
    While {
        scope: ScopeId,
        cond: ExprId,
        body: Vec<ThirStmt>,
    },
    // TODO: ALl the other statements
}

pub struct ThirPattern {
    pub kind: ThirPatternKind,
    pub ty: TypeRef,
    pub span: Span,
}

pub enum ThirPatternKind {
    Any,
    Bind {
        local: LocalId,
        mutable: bool,
    },
    Tuple(Vec<ThirPattern>),
    Struct {
        def: StructRef,
        fields: Vec<(Symbol, ThirPattern)>,
    },
    Constructor {
        def: EnumRef,
        idx: usize,
        args: ThirConstructorArgs,
    },
    IntLit(i64),
}

pub struct ThirScope {
    pub kind: ScopeKind,
    pub span: Span,
}

pub enum ScopeKind {
    Loop,
    Block,
}
