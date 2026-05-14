#![allow(dead_code)]

use crate::{
    common::{location::Span, symbols::Symbol},
    hir::{HirId, LocalId},
    ril::{EnumId, StructId, TypeId},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct THirPattern {
    pub id: HirId,
    pub desc: THirPatternDesc,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum THirPatternDesc {
    Any,
    Bind {
        id: LocalId,
        name: Symbol,
        mutable: bool,
    },
    Tuple(Vec<THirPattern>),
    DestructureStructBinding {
        def: StructId,
        template_args: Vec<TypeId>, // flat map of the fully resolved template args
        fields: Vec<(Symbol, THirPattern)>, // The resolved ith field is bound to the ith pattern
    },
    Constructor {
        def: EnumId,
        template_args: Vec<TypeId>, // flat map of the fully resolved template args
        variant: Symbol,            // resolved variant
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct THirExpr {
    pub id: HirId,
    pub desc: THirExprDesc,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum THirExprDesc {}
