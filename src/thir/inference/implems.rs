use std::{
    collections::HashSet,
    iter::{self, once},
};

use crate::{
    name_resolve::implems::impls_in_package,
    ril::{ImplSource, ScopeOwnerId},
    thir::inference::{constraints::InferenceConstraintKind, implicit::ImplicitContext},
};

use super::*;

#[derive(Debug)]
pub struct PotentialBlockRes {
    pub templates: Box<[InferVar]>,
    pub constraints: HashSet<InferenceConstraintKind>,
}

impl<'a> InferenceCtx<'a> {
    pub fn matches_ty(
        &mut self,
        ty: &InferTy,
        matcher: TypeRef,
        ctx: &ImplicitContext,
    ) -> Option<HashSet<InferenceConstraintKind>> {
        match ty {
            InferTy::Var(infer_var) => {
                let allocated = self.allocate_type_ref(&matcher, ctx); // Self not allowed here
                Some(HashSet::from_iter(once(InferenceConstraintKind::Unify {
                    a: InferTy::Var(*infer_var),
                    b: allocated,
                })))
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
                    let mut constraints = HashSet::new();
                    fields
                        .iter()
                        .zip(other_fields)
                        .try_for_each(|(infer_ty, matcher)| {
                            let extension = self.matches_ty(infer_ty, matcher, ctx)?;
                            constraints.extend(extension);
                            Some(())
                        })?;
                    Some(constraints)
                }
                TypeRef::Param(id) => Some(HashSet::from_iter(iter::once(
                    InferenceConstraintKind::Unify {
                        a: ty.clone(),
                        b: ctx.get_template(id.0)?.clone(),
                    },
                ))),
                TypeRef::Unknown | TypeRef::Error => None,
                TypeRef::Associated(_) | TypeRef::Zelf => {
                    // A Self or associated type should not have been encountered here
                    None
                }
            },
            InferTy::Param(_) => {
                // TODO: I think that this should not be allowed
                None
            }
            InferTy::Zelf => todo!(),
        }
    }

    pub fn is_potential_block(
        &mut self,
        ty: &InferTy,
        source: ImplSource<'a>,
    ) -> Option<PotentialBlockRes> {
        let mut constraints = HashSet::new();
        let templates = source.id(self.db).templates(self.db);
        let infer_templates = templates
            .iter()
            .map(|_| self.fresh_var())
            .collect::<Box<[_]>>();
        let mapped_templates = infer_templates
            .iter()
            .map(|var| InferTy::Var(*var))
            .collect::<Box<[_]>>();

        let ctx = ImplicitContext::new(
            self.db,
            ScopeOwnerId::Impl(source.id(self.db)),
            Arc::new([]), // TODO: check this is right
            mapped_templates.iter().cloned().collect(),
            Some(ty.clone()),
        )?;

        infer_templates
            .iter()
            .zip(templates.iter())
            .for_each(|(infer_ty, refs)| {
                for interface_ref in refs.iter() {
                    let id = interface_ref.def(self.db);
                    let args = interface_ref
                        .args(self.db)
                        .iter()
                        .map(|arg| self.allocate_type_ref(arg, &ctx))
                        .collect::<Box<[_]>>();
                    constraints.insert(InferenceConstraintKind::Implements {
                        ty: InferTy::Var(*infer_ty),
                        id,
                        args,
                    });
                }
            });

        constraints.extend(self.matches_ty(ty, source.id(self.db).implemented(self.db), &ctx)?);

        Some(PotentialBlockRes {
            templates: infer_templates,
            constraints,
        })
    }

    pub fn get_potential_blocks(
        &mut self,
        ty: &InferTy,
    ) -> HashMap<ImplSource<'a>, PotentialBlockRes> {
        self.packages
            .iter()
            .copied()
            .collect::<Box<[_]>>()
            .into_iter()
            .flat_map(|package| impls_in_package(self.db, package))
            .filter_map(|impl_source| {
                self.is_potential_block(ty, impl_source)
                    .map(|constraints| (impl_source, constraints))
            })
            .collect()
    }
}
