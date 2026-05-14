mod constraints;
mod display;
pub mod expr;
mod types;
mod unify;
mod var;

use std::{collections::HashMap, sync::Arc};

use crate::{
    Db,
    common::symbols::Symbol,
    hir::{LocalId, PartialTypeRef},
    parse_tree::top_level::AstTemplateArg,
    ril::{StructId, TypeDefId},
};
use ena::unify::{InPlace, UnificationTable, UnifyValue};
use var::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InferTy {
    Var(InferVar),
    Adt {
        def: TypeDefId,
        fields: Box<[InferTy]>,
    },
}

#[derive(Clone)]
pub struct InferenceCtx<'a> {
    db: &'a dyn Db,
    table: UnificationTable<InPlace<InferVar>>,
    local_map: HashMap<LocalId, InferVar>,
    constraints: Vec<InferenceConstraint>,
    templates: Box<[InferVar]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InferenceConstraint {
    Deref {
        var: InferVar,
        target: InferTy,
    },
    BindsLike {
        ty: InferVar,
        inner: InferTy,
        like: InferVar,
    },
    IndexedBy {
        elem_var: InferVar,
        base_ty: InferTy,
        index_ty: InferTy,
    },
    Tuple {
        elem_var: InferVar,
        tuple_ty: InferTy,
        has_index: u32,
    },
    StructField {
        elem_var: InferVar,
        struct_ty: InferTy,
        field: Symbol,
    },
}

impl<'db> InferenceCtx<'db> {
    fn create_local_map(
        table: &mut UnificationTable<InPlace<InferVar>>,
        locals: &[LocalId],
    ) -> HashMap<LocalId, InferVar> {
        locals.iter().map(|id| (*id, table.new_key(None))).collect()
    }

    fn create_templates(
        table: &mut UnificationTable<InPlace<InferVar>>,
        templates: &[AstTemplateArg],
    ) -> Box<[InferVar]> {
        templates.iter().map(|_| table.new_key(None)).collect()
    }

    pub fn new(db: &'db dyn Db, locals: &[LocalId], templates: &[AstTemplateArg]) -> Self {
        let mut table = UnificationTable::new();
        let local_map = Self::create_local_map(&mut table, locals);
        let templates = Self::create_templates(&mut table, templates);
        Self {
            db,
            table,
            local_map,
            constraints: Vec::new(),
            templates,
        }
    }

    pub fn templates(&self) -> Arc<[InferTy]> {
        Arc::from(
            self.templates
                .iter()
                .map(|t| InferTy::Var(*t))
                .collect::<Box<[_]>>(),
        )
    }
}

pub enum UnificationError {
    TypeDefIdMismatch(TypeDefId, TypeDefId),
    FieldCountMismatch(usize, usize),
    RecursiveDefinition(InferVar),
    UnmetConstraint(InferenceConstraint, Box<UnificationError>),
    ExpectedPtrLike(TypeDefId),
    MinTupleLengthMismatch { expected: usize, got: usize },
    ExpectedStructWithField { def: TypeDefId, field: Symbol },
    IncompleteStructLit { id: StructId, missing: Symbol },
    NonStructForStructLit(PartialTypeRef),
}

impl<'db> InferenceCtx<'db> {
    pub fn snapshot<T>(
        &mut self,
        f: impl Fn(&mut Self) -> Result<T, UnificationError>,
    ) -> Result<T, UnificationError> {
        let snapshot = self.table.snapshot();
        match f(self) {
            Ok(res) => {
                self.table.commit(snapshot);
                Ok(res)
            }
            Err(err) => {
                self.table.rollback_to(snapshot);
                Err(err)
            }
        }
    }
}

impl InferTy {
    pub fn is_adt(&self) -> Option<(TypeDefId, &[InferTy])> {
        match self {
            InferTy::Var(_) => None,
            InferTy::Adt { def, fields } => Some((*def, fields)),
        }
    }
}
