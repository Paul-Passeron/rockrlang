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

use crate::{
    hir::{
        HirPattern, HirPatternConstructorArgs, HirPatternDesc,
        HirStructFieldPattern,
    },
    name_resolve::type_expr::{enum_item, struct_item, templates_of_struct},
    parse_tree::top_level::{AstEnumVariant, AstEnumVariantKind},
    ril::{EnumId, ScopeOwnerId},
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
            this.inferred_patterns.insert(PatternId(pattern.id), ty.clone());
            Ok(ty)
        })
    }
    fn _infer_bind_pattern(&mut self, id: LocalId) -> InferTy {
        let var = self.local_map[&id];
        InferTy::Var(var)
    }

    fn _infer_pattern(
        &mut self,
        pattern: &HirPattern,
        binds_like: Option<InferTy>,
    ) -> Result<InferTy, UnificationError> {
        match &pattern.data {
            HirPatternDesc::Error | HirPatternDesc::Any => {
                Ok(self.fresh_var().into())
            }
            HirPatternDesc::Bind { id, .. } => {
                Ok(InferTy::Var(self.local_map[id]))
            }
            HirPatternDesc::Tuple(pats) => {
                let tys: Box<[_]> = pats
                    .iter()
                    .map(|p| self.infer_pattern(p, binds_like.clone()))
                    .try_collect()?;
                Ok(self.tuple_of(tys))
            }
            HirPatternDesc::IntLit(_) => {
                Ok(InferTy::Var(self.emit_intlike_constraint()))
            }
            HirPatternDesc::DestructureBinding {
                resolution: struct_id,
                fields,
            } => self._infer_destructure_binding(binds_like, struct_id, fields),
            HirPatternDesc::Constructor { resolution, name, fields } => {
                self._infer_constructor(*resolution, *name, fields, binds_like)
            }
        }
    }

    fn get_unit_type_or_diagnose(&mut self, enum_id: EnumId, name: Symbol) {
        let item = enum_item(self.db, enum_id.interned());
        let Some(variant) =
            item.variants.iter().find(|variant| variant.name == name)
        else {
            todo!()
        };
        match &variant.kind {
            AstEnumVariantKind::Unit => (),
            _ => todo!(),
        }
    }

    fn get_enum_variant(
        &self,
        enum_id: EnumId,
        name: Symbol,
    ) -> Option<AstEnumVariant> {
        let item = enum_item(self.db, enum_id.interned());
        item.variants.iter().find(|variant| variant.name == name).cloned()
    }

    fn get_ctx_for_enum(
        &mut self,
        enum_id: EnumId,
        template_tys: &[InferTy],
    ) -> ImplicitContext {
        let zelf = InferTy::Adt {
            def: TypeDefId::Enum(enum_id),
            fields: template_tys.iter().cloned().collect(),
        };
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
        .unwrap()
    }

    fn get_tuple_fields_or_diagnose(
        &mut self,
        enum_id: EnumId,
        name: Symbol,
        template_tys: &[InferTy],
    ) -> Vec<InferTy> {
        let Some(variant) = self.get_enum_variant(enum_id, name) else {
            todo!()
        };
        match &variant.kind {
            AstEnumVariantKind::TupleLike(tys) => {
                let ctx = self.get_ctx_for_enum(enum_id, template_tys);
                tys.iter()
                    .map(|ty| {
                        self.allocate_ast_type_expr(&ty.data, &ctx)
                            .unwrap_or_else(|| self.fresh_var().into())
                    })
                    .collect()
            }
            _ => todo!(),
        }
    }

    fn get_field_types_of_variant_or_diagnose(
        &mut self,
        enum_id: EnumId,
        name: Symbol,
        template_tys: &[InferTy],
    ) -> HashMap<Symbol, InferTy> {
        let Some(variant) = self.get_enum_variant(enum_id, name) else {
            todo!()
        };
        let ctx = self.get_ctx_for_enum(enum_id, template_tys);
        match &variant.kind {
            AstEnumVariantKind::StructLike(fields) => fields
                .iter()
                .map(|field| {
                    (
                        field.name,
                        self.allocate_ast_type_expr(&field.ty.data, &ctx)
                            .unwrap_or_else(|| self.fresh_var().into()),
                    )
                })
                .collect(),
            _ => todo!(),
        }
    }

    fn _infer_fields_variants(
        &mut self,
        enum_id: EnumId,
        name: Symbol,
        fields: &[HirStructFieldPattern],
        template_tys: &[InferTy],
        binds_like: Option<InferTy>,
    ) {
        let field_types = self.get_field_types_of_variant_or_diagnose(
            enum_id,
            name,
            template_tys,
        );
        self._infer_fields(fields, &field_types, binds_like);
    }

    fn _infer_constructor(
        &mut self,
        enum_id: EnumId,
        name: Symbol,
        fields: &HirPatternConstructorArgs,
        binds_like: Option<InferTy>,
    ) -> Result<InferTy, UnificationError> {
        let template_tys =
            self.get_templates_for(Definition::Type(TypeDefId::Enum(enum_id)));

        match fields {
            HirPatternConstructorArgs::None => {
                self.get_unit_type_or_diagnose(enum_id, name);
            }
            HirPatternConstructorArgs::StructFields(fields) => {
                self._infer_fields_variants(
                    enum_id,
                    name,
                    fields,
                    &template_tys,
                    binds_like,
                );
            }
            HirPatternConstructorArgs::TupleFields(hir_patterns) => {
                self._infer_tuple_variant(
                    enum_id,
                    name,
                    hir_patterns,
                    &template_tys,
                    binds_like,
                );
            }
        }
        Ok(InferTy::Adt { def: TypeDefId::Enum(enum_id), fields: template_tys })
    }

    fn apply_binds_like(
        &mut self,
        like: Option<&InferTy>,
        target: InferTy,
    ) -> InferTy {
        if let Some(ty) = like {
            let var = self.fresh_var();
            self.unify(ty.clone(), var.into()).unwrap();
            self.emit_binds_like_constraint(var, target).into()
        } else {
            target
        }
    }

    fn _infer_tuple_variant(
        &mut self,
        enum_id: EnumId,
        name: Symbol,
        hir_patterns: &[HirPattern],
        template_tys: &[InferTy],
        binds_like: Option<InferTy>,
    ) {
        let variant_tys = self.get_tuple_fields_or_diagnose(
            enum_id,
            name,
            template_tys.as_ref(),
        );

        let tys: Box<_> = hir_patterns
            .iter()
            .map(|pat| {
                self.infer_pattern(pat, binds_like.clone())
                    .unwrap_or_else(|_| self.fresh_var().into())
            })
            .collect();

        if variant_tys.len() != tys.len() {
            todo!("Emit diag here")
        }

        variant_tys
            .into_iter()
            .zip_longest(tys.into_iter().zip(hir_patterns))
            .take(hir_patterns.len())
            .for_each(|zipped| {
                let (variant_ty, pat_ty, pattern) = match zipped {
                    EitherOrBoth::Both(a, (b, c)) => (a, b, Some(c)),
                    EitherOrBoth::Left(a) => (a, self.fresh_var().into(), None),
                    EitherOrBoth::Right((b, c)) => {
                        (self.fresh_var().into(), b, Some(c))
                    }
                };
                self.unify_pattern_depending_on_kind(
                    binds_like.as_ref(),
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
        let data = pattern.map(|pat| &pat.data);
        match data {
            Some(HirPatternDesc::Any | HirPatternDesc::Bind { .. }) => {
                let adjusted = self.apply_binds_like(binds_like, inner_ty);
                self.unify(adjusted.clone(), pot_ref_ty.clone()).expect("TODO");
            }
            _ => {
                let mut p = TypePrinter::new();
                p.options = p.options.with(
                    crate::printer::type_printer::TypePrinterOption::DebugInferenceVars,
                );
                self.emit_is_inner_constraint(inner_ty, pot_ref_ty);
            }
        }
    }

    fn _infer_destructure_binding(
        &mut self,
        binds_like: Option<InferTy>,
        struct_id: &StructId,
        fields: &[HirStructFieldPattern],
    ) -> Result<InferTy, UnificationError> {
        let (struct_ty, field_types) = self.fresh_struct_instance(struct_id);
        self._infer_fields(fields, &field_types, binds_like);
        Ok(struct_ty)
    }

    fn fresh_struct_instance(
        &mut self,
        struct_id: &StructId,
    ) -> (InferTy, HashMap<Symbol, InferTy>) {
        let templates = templates_of_struct(self.db, struct_id.interned());
        let infer_templates = self
            .get_templates_for(Definition::Type(TypeDefId::Struct(*struct_id)));
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
        (struct_ty, field_types)
    }

    fn _infer_fields(
        &mut self,
        fields: &[HirStructFieldPattern],
        field_types: &HashMap<Symbol, InferTy>,
        binds_like: Option<InferTy>,
    ) {
        for field in fields {
            match field {
                HirStructFieldPattern::Rebind { name, pattern } => {
                    let inferred = self
                        .infer_pattern(pattern, binds_like.clone())
                        .expect("TODO");
                    let field_ty = field_types
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| self.fresh_var().into());
                    self.unify_pattern_depending_on_kind(
                        binds_like.as_ref(),
                        field_ty,
                        inferred,
                        Some(pattern),
                    );
                }
                HirStructFieldPattern::Name { id, name } => {
                    let local_ty = self.infer_local(*id);
                    let field_ty = field_types
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| self.fresh_var().into());
                    let adjusted =
                        self.apply_binds_like(binds_like.as_ref(), field_ty);
                    self.unify(adjusted, local_ty).expect("TODO");
                }
            }
        }
    }
}
