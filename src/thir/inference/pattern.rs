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
            HirPatternDesc::IntLit(_) => Ok(InferTy::Var(self.emit_intlike_constraint())),
        }
    }
}
