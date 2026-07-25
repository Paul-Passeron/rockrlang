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
    compiler::{Workspace, workspace_packages},
    name_resolve::implems::impls_in_package,
    resolved::{ImplSource, ScopeOwnerId},
    typecheck::inference::{
        constraints::InferenceConstraintKind, implicit::ImplicitContext,
    },
};

use super::{Db, HashMap, InferTy, InferVar, InferenceCtx, Package, TypeRef};

#[derive(Debug)]
pub struct PotentialBlockRes {
    pub templates: Box<[InferVar]>,
    pub constraints: Vec<InferenceConstraintKind>,
}

impl<'a> InferenceCtx<'a> {
    pub fn matches_ty(
        &mut self,
        ty: &InferTy,
        matcher: TypeRef,
        ctx: &ImplicitContext,
    ) -> Option<Vec<InferenceConstraintKind>> {
        match ty {
            InferTy::Var(infer_var) => {
                let allocated = self.allocate_type_ref(matcher, ctx);
                Some(vec![InferenceConstraintKind::Unify {
                    a: InferTy::Var(*infer_var),
                    b: allocated,
                }])
            }
            InferTy::Adt { def, fields } => match matcher {
                TypeRef::Concrete(type_id) => {
                    let other_def = type_id.def(self.db);
                    if *def != other_def {
                        return None;
                    }
                    let other_fields = type_id.args(self.db);
                    if other_fields.len() != fields.len() {
                        return None;
                    }
                    let mut constraints = Vec::new();
                    fields.iter().zip(other_fields).try_for_each(
                        |(infer_ty, matcher)| {
                            constraints.extend(self.matches_ty(infer_ty, *matcher, ctx)?);
                            Some(())
                        },
                    )?;
                    Some(constraints)
                }
                TypeRef::Param(id) => Some(vec![InferenceConstraintKind::Unify {
                    a: ty.clone(),
                    b: ctx.get_template(id.0)?,
                }]),
                TypeRef::Associated(_)
                | TypeRef::Zelf
                | TypeRef::Unknown
                | TypeRef::Error => None,
            },
            InferTy::Param(_) => {
                if let TypeRef::Param(id) = matcher {
                    Some(vec![InferenceConstraintKind::Unify {
                        a: ty.clone(),
                        b: ctx.get_template(id.0)?,
                    }])
                } else {
                    None
                }
            }
        }
    }

    pub fn is_potential_block(
        &mut self,
        ty: &InferTy,
        source: ImplSource<'a>,
    ) -> Option<PotentialBlockRes> {
        let templates = source.id(self.db).templates(self.db);
        let infer_templates =
            templates.iter().map(|_| self.fresh_var()).collect::<Box<[_]>>();
        let mapped_templates =
            infer_templates.iter().map(|var| InferTy::Var(*var)).collect::<Box<[_]>>();

        let ctx = ImplicitContext::new(
            self.db,
            ScopeOwnerId::Impl(*source.id(self.db)),
            &[],
            mapped_templates.iter().cloned().collect(),
            Some(ty.clone()),
        );

        let mut constraints =
            self.matches_ty(ty, source.id(self.db).implemented(self.db), &ctx)?;

        for (infer_ty, refs) in infer_templates.iter().zip(templates.iter()) {
            for interface_ref in refs.iter() {
                let id = interface_ref.def(self.db);
                let args = interface_ref
                    .args(self.db)
                    .iter()
                    .map(|arg| self.allocate_type_ref(*arg, &ctx))
                    .collect::<Box<[_]>>();
                constraints.push(InferenceConstraintKind::Implements {
                    ty: InferTy::Var(*infer_ty),
                    id,
                    args,
                });
            }
        }

        Some(PotentialBlockRes { templates: infer_templates, constraints })
    }

    fn get_packages(db: &dyn Db) -> &[Package<'_>] {
        workspace_packages(db, Workspace::get(db))
    }

    pub fn get_potential_blocks(
        &mut self,
        ty: &InferTy,
    ) -> HashMap<ImplSource<'a>, PotentialBlockRes> {
        let db = self.db;
        Self::get_packages(db)
            .iter()
            .flat_map(|package| impls_in_package(self.db, *package).iter())
            .filter_map(|impl_source| {
                self.is_potential_block(ty, *impl_source).map(|res| (*impl_source, res))
            })
            .collect()
    }
}
