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
    hir::{HirPattern, HirPatternDesc, HirStructFieldPattern},
    name_resolve::type_expr::{struct_item, templates_of_struct},
    ril::ScopeOwnerId,
};

use super::*;

impl<'a> InferenceCtx<'a> {
    pub fn infer_pattern(
        &mut self,
        pattern: &HirPattern,
        binds_like: Option<InferTy>,
    ) -> Result<InferTy, UnificationError> {
        self.snapshot(|this| {
            let ty = this._infer_pattern(pattern, binds_like.clone())?;
            this.inferred_patterns
                .insert(PatternId(pattern.id), ty.clone());
            Ok(ty)
        })
    }

    fn _infer_pattern(
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
                if let Some(like) = binds_like {
                    let like_var = self.fresh_var();
                    self.unify(InferTy::Var(like_var), like).unwrap();
                    let res =
                        self.emit_binds_like_constraint(like_var, InferTy::Var(var));
                    Ok(InferTy::Var(res))
                } else {
                    Ok(InferTy::Var(var))
                }
            }
            HirPatternDesc::Error | HirPatternDesc::Any => {
                Ok(InferTy::Var(self.fresh_var()))
            }
            HirPatternDesc::Tuple(_) => todo!(),
            HirPatternDesc::DestructureBinding {
                resolution: struct_id,
                fields,
            } => {
                let templates = templates_of_struct(self.db, struct_id.interned());
                let infer_templates = templates
                    .iter()
                    .map(|_| InferTy::Var(self.fresh_var()))
                    .collect_vec();
                let struct_ty = InferTy::Adt {
                    def: TypeDefId::Struct(*struct_id),
                    fields: infer_templates.iter().cloned().collect(),
                };
                let ctx = ImplicitContext::new(
                    self.db,
                    ScopeOwnerId::Module(struct_id.parent(self.db)),
                    templates.iter().cloned().collect(),
                    infer_templates.iter().cloned().collect(),
                    Some(struct_ty.clone()),
                )
                .unwrap();
                let item = struct_item(self.db, struct_id.interned());

                let field_types: HashMap<Symbol, InferTy> = item
                    .fields
                    .iter()
                    .map(|field| {
                        (
                            field.name,
                            self.allocate_ast_type_expr(&field.ty.data, &ctx)
                                .unwrap_or(InferTy::Var(self.fresh_var())),
                        )
                    })
                    .collect();

                for field in fields {
                    let (name, inferred) = match field {
                        HirStructFieldPattern::Rebind { name, pattern } => {
                            (*name, self.infer_pattern(pattern, binds_like.clone())?)
                        }
                        HirStructFieldPattern::Name { id, name } => {
                            let local_ty = self.infer_local(*id);
                            let ty = if let Some(like) = &binds_like {
                                let like_var = self.fresh_var();
                                self.unify(InferTy::Var(like_var), like.clone()).unwrap();
                                InferTy::Var(
                                    self.emit_binds_like_constraint(like_var, local_ty),
                                )
                            } else {
                                local_ty
                            };
                            (*name, ty)
                        }
                    };
                    if let Some(expected_type) = field_types.get(&name) {
                        self.unify(inferred, expected_type.clone())?;
                    } else {
                        todo!()
                    };
                }
                Ok(struct_ty)
            }
            HirPatternDesc::Constructor { .. } => todo!(),
            HirPatternDesc::IntLit(_) => Ok(InferTy::Var(self.emit_intlike_constraint())),
        }
    }
}
