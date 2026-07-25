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

use itertools::EitherOrBoth;
use salsa::Accumulator;

use crate::{
    common::location::Span,
    compiler::diagnostic::Diag,
    hir::{HirPattern, HirPatternConstructorArgs, HirPatternDesc, HirStructFieldPattern},
    name_resolve::type_expr::{enum_item, struct_item, templates_of_struct},
    parse_tree::top_level::{AstEnumVariant, AstEnumVariantKind},
    resolved::{EnumId, ScopeOwnerId},
};

use super::{
    Definition, HashMap, ImplicitContext, InferTy, InferenceCtx, Itertools, LocalId,
    PatternId, StructId, Symbol, TypeDefId, UnificationError,
};

impl InferenceCtx<'_> {
    pub fn infer_pattern(
        &mut self,
        pattern: &HirPattern,
        binds_like: Option<&InferTy>,
    ) -> Result<InferTy, UnificationError> {
        self.snapshot(|this| {
            let ty = this.infer_pattern_aux(pattern, binds_like)?;
            this.inferred_patterns.insert(PatternId(pattern.id), ty.clone());
            Ok(ty)
        })
    }
    fn _infer_bind_pattern(&self, id: LocalId) -> InferTy {
        let var = self.local_map[&id];
        InferTy::Var(var)
    }

    fn infer_pattern_aux(
        &mut self,
        pattern: &HirPattern,
        binds_like: Option<&InferTy>,
    ) -> Result<InferTy, UnificationError> {
        match &pattern.data {
            HirPatternDesc::Error | HirPatternDesc::Any => Ok(self.fresh_var().into()),
            HirPatternDesc::Bind { id, .. } => Ok(InferTy::Var(self.local_map[id])),
            HirPatternDesc::Tuple(pats) => {
                let tys = pats
                    .iter()
                    .map(|p| self.infer_pattern(p, binds_like))
                    .try_collect()?;
                Ok(self.tuple_of(tys))
            }
            HirPatternDesc::IntLit(_) => Ok(InferTy::Var(self.emit_intlike_constraint())),
            HirPatternDesc::DestructureBinding { resolution: struct_id, fields } => {
                Ok(self.infer_destructure_binding_aux(binds_like, *struct_id, fields))
            }
            HirPatternDesc::Constructor { resolution, name, fields } => Ok(self
                .infer_constructor_aux(
                    *resolution,
                    *name,
                    fields,
                    binds_like,
                    pattern.span,
                )),
        }
    }

    fn get_unit_type_or_diagnose(&self, enum_id: EnumId, name: Symbol, span: Span) {
        match self.get_enum_variant(enum_id, name).as_ref().map(|v| &v.kind) {
            Some(AstEnumVariantKind::Unit) => (),
            _ => Diag::generic_error(
                format!(
                    "Expected variant `{}` to be unit for enum `{}`",
                    name.to_string(self.db),
                    enum_id.name(self.db).to_string(self.db)
                ),
                span,
            )
            .accumulate(self.db),
        }
    }

    fn get_enum_variant(&self, enum_id: EnumId, name: Symbol) -> Option<AstEnumVariant> {
        let item = enum_item(self.db, enum_id.interned());
        item.variants.iter().find(|variant| variant.name == name).cloned()
    }

    fn get_ctx_for_enum(
        &self,
        enum_id: EnumId,
        template_tys: &[InferTy],
    ) -> ImplicitContext {
        let zelf =
            InferTy::Adt { def: TypeDefId::Enum(enum_id), fields: template_tys.to_vec() };
        ImplicitContext::new(
            self.db,
            ScopeOwnerId::Module(enum_id.parent(self.db)),
            enum_item(self.db, enum_id.interned())
                .template_args
                .iter()
                .cloned()
                .collect(),
            template_tys.iter().cloned().collect(),
            Some(zelf),
        )
        .expect("ImplicitContext error during construction is an ICE")
    }

    fn get_tuple_fields_or_diagnose(
        &mut self,
        enum_id: EnumId,
        name: Symbol,
        template_tys: &[InferTy],
        span: Span,
    ) -> Vec<InferTy> {
        let variant = self.get_enum_variant(enum_id, name);
        if let Some(AstEnumVariantKind::TupleLike(tys)) =
            variant.as_ref().map(|v| &v.kind)
        {
            let ctx = self.get_ctx_for_enum(enum_id, template_tys);
            tys.iter()
                .map(|ty| {
                    self.allocate_ast_type_expr(&ty.data, &ctx)
                        .unwrap_or_else(|| self.fresh_var().into())
                })
                .collect()
        } else {
            Diag::generic_error(
                format!(
                    "Expected variant `{}` to be tuple for enum `{}`",
                    name.to_string(self.db),
                    enum_id.name(self.db).to_string(self.db)
                ),
                span,
            )
            .accumulate(self.db);
            vec![]
        }
    }

    fn get_field_types_of_variant_or_diagnose(
        &mut self,
        enum_id: EnumId,
        name: Symbol,
        template_tys: &[InferTy],
        span: Span,
    ) -> HashMap<Symbol, InferTy> {
        let ctx = self.get_ctx_for_enum(enum_id, template_tys);
        let variant = self.get_enum_variant(enum_id, name);
        if let Some(AstEnumVariantKind::StructLike(fields)) =
            variant.as_ref().map(|v| &v.kind)
        {
            fields
                .iter()
                .map(|field| {
                    (
                        field.name,
                        self.allocate_ast_type_expr(&field.ty.data, &ctx)
                            .unwrap_or_else(|| self.fresh_var().into()),
                    )
                })
                .collect()
        } else {
            Diag::generic_error(
                format!(
                    "Expected variant `{}` to be struct-like for enum `{}`",
                    name.to_string(self.db),
                    enum_id.name(self.db).to_string(self.db)
                ),
                span,
            )
            .accumulate(self.db);
            HashMap::new()
        }
    }

    fn infer_fields_variants_aux(
        &mut self,
        enum_id: EnumId,
        infos: &VariantInfos<HirStructFieldPattern>,
        span: Span,
    ) {
        let field_types = self.get_field_types_of_variant_or_diagnose(
            enum_id,
            infos.name,
            infos.template_tys,
            span,
        );
        self.infer_fields_aux(infos.children, &field_types, infos.binds_like);
    }

    fn infer_constructor_aux(
        &mut self,
        enum_id: EnumId,
        name: Symbol,
        fields: &HirPatternConstructorArgs,
        binds_like: Option<&InferTy>,
        span: Span,
    ) -> InferTy {
        let template_tys =
            self.get_templates_for(Definition::Type(TypeDefId::Enum(enum_id)));

        match fields {
            HirPatternConstructorArgs::None => {
                self.get_unit_type_or_diagnose(enum_id, name, span);
            }
            HirPatternConstructorArgs::StructFields(fields) => {
                self.infer_fields_variants_aux(
                    enum_id,
                    &VariantInfos {
                        name,
                        children: fields,
                        template_tys: &template_tys,
                        binds_like,
                    },
                    span,
                );
            }
            HirPatternConstructorArgs::TupleFields(hir_patterns) => {
                self.infer_tuple_variant_aux(
                    enum_id,
                    &VariantInfos {
                        name,
                        children: hir_patterns,
                        template_tys: &template_tys,
                        binds_like,
                    },
                    span,
                );
            }
        }
        InferTy::Adt { def: TypeDefId::Enum(enum_id), fields: template_tys }
    }

    fn apply_binds_like(&mut self, like: Option<&InferTy>, target: InferTy) -> InferTy {
        if let Some(ty) = like {
            let var = self.fresh_var();
            self.unify(ty, &var.into())
                .expect("binding to a fresh variable should not fail");
            self.emit_binds_like_constraint(var, target).into()
        } else {
            target
        }
    }

    fn infer_tuple_variant_aux(
        &mut self,
        enum_id: EnumId,
        infos: &VariantInfos<HirPattern>,
        span: Span,
    ) {
        let variant_tys = self.get_tuple_fields_or_diagnose(
            enum_id,
            infos.name,
            infos.template_tys,
            span,
        );

        let tys: Box<_> = infos
            .children
            .iter()
            .map(|pat| {
                self.infer_pattern(pat, infos.binds_like)
                    .unwrap_or_else(|_| self.fresh_var().into())
            })
            .collect();

        if variant_tys.len() != tys.len() {
            Diag::generic_error(
                format!(
                    "Mismatched length in tuple variant `{}` for enum `{}`, expected {} but got {}",
                    infos.name.to_string(self.db),
                    enum_id.name(self.db).to_string(self.db),
                    variant_tys.len(),
                    tys.len()
                ),
                span,
            )
            .accumulate(self.db);
        }

        variant_tys
            .into_iter()
            .zip_longest(tys.into_iter().zip(infos.children))
            .take(infos.children.len())
            .for_each(|zipped| {
                let (variant_ty, pat_ty, pattern) = match zipped {
                    EitherOrBoth::Both(a, (b, c)) => (a, b, Some(c)),
                    EitherOrBoth::Left(a) => (a, self.fresh_var().into(), None),
                    EitherOrBoth::Right((b, c)) => (self.fresh_var().into(), b, Some(c)),
                };
                self.unify_pattern_depending_on_kind(
                    infos.binds_like,
                    variant_ty,
                    pat_ty,
                    pattern,
                );
            });
    }

    /// TODO: find a better name
    pub fn unify_pattern_depending_on_kind(
        &mut self,
        binds_like: Option<&InferTy>,
        inner_ty: InferTy,
        pot_ref_ty: InferTy,
        pattern: Option<&HirPattern>,
    ) {
        match pattern {
            Some(HirPattern {
                data: HirPatternDesc::Any | HirPatternDesc::Bind { .. },
                span,
                ..
            }) => {
                let adjusted = self.apply_binds_like(binds_like, inner_ty);
                if let Err(err) = self.unify(&adjusted, &pot_ref_ty) {
                    Diag::generic_error(
                        format!("Unification error: {}", err.display(self.db)),
                        *span,
                    )
                    .accumulate(self.db);
                }
            }
            _ => {
                self.emit_is_inner_constraint(inner_ty, pot_ref_ty);
            }
        }
    }

    fn infer_destructure_binding_aux(
        &mut self,
        binds_like: Option<&InferTy>,
        struct_id: StructId,
        fields: &[HirStructFieldPattern],
    ) -> InferTy {
        let (struct_ty, field_types) = self.fresh_struct_instance(struct_id);
        self.infer_fields_aux(fields, &field_types, binds_like);
        struct_ty
    }

    fn fresh_struct_instance(
        &mut self,
        struct_id: StructId,
    ) -> (InferTy, HashMap<Symbol, InferTy>) {
        let templates = templates_of_struct(self.db, struct_id.interned());
        let infer_templates =
            self.get_templates_for(Definition::Type(TypeDefId::Struct(struct_id)));
        let struct_ty = InferTy::Adt {
            def: TypeDefId::Struct(struct_id),
            fields: infer_templates.clone(),
        };
        let ctx = ImplicitContext::new(
            self.db,
            ScopeOwnerId::Module(struct_id.parent(self.db)),
            templates.iter().cloned().collect(),
            infer_templates.iter().cloned().collect(),
            Some(struct_ty.clone()),
        )
        .expect("Implicit context failing to be created is an internal compiler error");
        let item = struct_item(self.db, struct_id.interned());

        let field_types: HashMap<Symbol, InferTy> = item
            .fields
            .iter()
            .map(|field| {
                (
                    field.name,
                    self.allocate_ast_type_expr(&field.ty.data, &ctx)
                        .unwrap_or_else(|| self.fresh_var().into()),
                )
            })
            .collect();
        (struct_ty, field_types)
    }

    fn infer_fields_aux(
        &mut self,
        fields: &[HirStructFieldPattern],
        field_types: &HashMap<Symbol, InferTy>,
        binds_like: Option<&InferTy>,
    ) {
        for field in fields {
            match field {
                HirStructFieldPattern::Rebind { name, pattern, .. } => {
                    match self.infer_pattern(pattern, binds_like) {
                        Ok(inferred) => {
                            let field_ty = field_types
                                .get(name)
                                .cloned()
                                .unwrap_or_else(|| self.fresh_var().into());
                            self.unify_pattern_depending_on_kind(
                                binds_like,
                                field_ty,
                                inferred,
                                Some(pattern),
                            );
                        }
                        Err(err) => {
                            let span = field.span();
                            Diag::generic_error(format!("Unification error in field `{}` of struct pattern: {}",
                                field.name().to_string(self.db),
                                err.display(self.db),
                            ), span)
                                .accumulate(self.db);
                        }
                    }
                }
                HirStructFieldPattern::Name { id, name, span } => {
                    let local_ty = self.infer_local(*id);
                    let field_ty = field_types
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| self.fresh_var().into());
                    let adjusted = self.apply_binds_like(binds_like, field_ty);
                    if let Err(err) = self.unify(&adjusted, &local_ty) {
                        Diag::generic_error(
                            format!(
                                "Unification error in field pattern: {}",
                                err.display(self.db)
                            ),
                            *span,
                        )
                        .accumulate(self.db);
                    }
                }
            }
        }
    }
}
struct VariantInfos<'a, T> {
    pub name: Symbol,
    pub children: &'a [T],
    pub template_tys: &'a [InferTy],
    pub binds_like: Option<&'a InferTy>,
}
