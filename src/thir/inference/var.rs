use crate::thir::inference::InferenceCtx;

use super::InferTy;
use ena::unify::UnifyKey;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InferVar(pub usize);

impl fmt::Display for InferVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}", self.0)
    }
}

impl From<InferVar> for InferTy {
    fn from(value: InferVar) -> Self {
        InferTy::Var(value)
    }
}

impl UnifyKey for InferVar {
    type Value = Option<InferTy>;

    fn index(&self) -> u32 {
        self.0 as u32
    }

    fn from_index(u: u32) -> Self {
        Self(u as usize)
    }

    fn tag() -> &'static str {
        "InferVar"
    }
}

impl<'a> InferenceCtx<'a> {
    pub fn fresh_var(&mut self) -> InferVar {
        self.table.new_key(None)
    }
}
