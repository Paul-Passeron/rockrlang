use crate::hir::{HirPattern, HirPatternDesc};

use super::*;

impl<'a> InferenceCtx<'a> {
    pub fn infer_pattern(
        &mut self,
        pattern: &HirPattern,
        binds_like: Option<InferTy>,
    ) -> Result<InferTy, UnificationError> {
        match &pattern.data {
            HirPatternDesc::Bind { id, .. } => {
                let var = *self
                    .local_map
                    .get(id)
                    .expect("Internal error: local_map should contain all bind ids");
                if let Some(_like) = binds_like {
                    todo!()
                }
                Ok(InferTy::Var(var))
            }
            HirPatternDesc::Any => Ok(InferTy::Var(self.fresh_var())),
            HirPatternDesc::Tuple(_) => todo!(),
            HirPatternDesc::DestructureBinding { .. } => todo!(),
            HirPatternDesc::Constructor { .. } => todo!(),
        }
    }
}
