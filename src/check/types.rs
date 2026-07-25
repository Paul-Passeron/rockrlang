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

use std::collections::HashSet;

use salsa::Accumulator;

use crate::{
    Db,
    common::symbols::Symbol,
    compiler::diagnostic::Diag,
    name_resolve::type_expr::{enum_item, struct_item},
    parse_tree::top_level::{AstEnumVariant, AstEnumVariantKind},
    resolved::{EnumId, ScopeOwnerId, StructId, TypeDefId},
    typecheck::inference::implicit::{AsAstImplCtx, AstImplicitContext},
};

pub fn check_typedef(db: &dyn Db, typedef: TypeDefId) {
    match typedef {
        TypeDefId::Builtin(_) => (),
        TypeDefId::Struct(struct_id) => check_struct(db, struct_id),
        TypeDefId::Enum(enum_id) => check_enum(db, enum_id),
    }
}

fn check_struct(db: &dyn Db, struct_id: StructId) {
    let item = struct_item(db, struct_id.interned());
    let ctx = AstImplicitContext::new(
        db,
        ScopeOwnerId::Module(struct_id.parent(db)),
        &item.template_args,
    );
    let mut names: HashSet<Symbol> = HashSet::new();
    for field in &item.fields {
        if !names.insert(field.name) {
            // Duplicate field.name
            Diag::generic_error(
                format!(
                    "Duplicated field name `{}` in struct `{}`",
                    field.name.to_string(db),
                    TypeDefId::Struct(struct_id).to_string(db)
                ),
                field.span,
            )
            .accumulate(db);
        }
        match ctx.resolve(db, &field.ty.data) {
            Some(_) => (),
            None => {
                Diag::generic_error(
                    format!(
                        "Field `{}` in struct `{}` has error type",
                        field.name.to_string(db),
                        TypeDefId::Struct(struct_id).to_string(db)
                    ),
                    field.ty.span,
                )
                .accumulate(db);
            }
        }
    }
}

fn check_enum(db: &dyn Db, enum_id: EnumId) {
    let item = enum_item(db, enum_id.interned());
    let ctx = AstImplicitContext::new(
        db,
        ScopeOwnerId::Module(enum_id.parent(db)),
        &item.template_args,
    );
    let mut names: HashSet<Symbol> = HashSet::new();
    for variant in &item.variants {
        if !names.insert(variant.name) {
            // Duplicate variant.name
            Diag::generic_error(
                format!(
                    "Duplicated variant name `{}` in enum `{}`",
                    variant.name.to_string(db),
                    TypeDefId::Enum(enum_id).to_string(db)
                ),
                variant.span,
            )
            .accumulate(db);
        }
        check_variant(db, enum_id, variant, &ctx);
    }
}

fn check_variant(
    db: &dyn Db,
    enum_id: EnumId,
    variant: &AstEnumVariant,
    ctx: &AstImplicitContext,
) {
    match &variant.kind {
        AstEnumVariantKind::Unit => (),
        AstEnumVariantKind::StructLike(fields) => {
            let mut names: HashSet<Symbol> = HashSet::new();
            for field in fields {
                if !names.insert(field.name) {
                    // Duplicate field.name
                    Diag::generic_error(
                        format!(
                            "Duplicated field name `{}` in variant `{}` of enum `{}`",
                            field.name.to_string(db),
                            variant.name.to_string(db),
                            TypeDefId::Enum(enum_id).to_string(db)
                        ),
                        field.span,
                    )
                    .accumulate(db);
                }
                match ctx.resolve(db, &field.ty.data) {
                    Some(_) => (),
                    None => {
                        Diag::generic_error(
                            format!(
                                "Field name `{}` in variant `{}` of enum `{}` has error type",
                                field.name.to_string(db),
                                variant.name.to_string(db),
                                TypeDefId::Enum(enum_id).to_string(db)
                            ),
                            field.ty.span,
                        )
                        .accumulate(db);
                    }
                }
            }
        }
        AstEnumVariantKind::TupleLike(tys) => {
            for (i, (ast, ty)) in
                tys.iter().map(|ty| (ty, ctx.resolve(db, &ty.data))).enumerate()
            {
                match ty {
                    Some(_) => (),
                    None => {
                        Diag::generic_error(
                            format!(
                                "Tuple field in position {i} of variant `{}` in enum `{}` has error type",
                                variant.name.to_string(db),
                                TypeDefId::Enum(enum_id).to_string(db)
                            ),
                            ast.span,
                        )
                        .accumulate(db);
                    }
                }
            }
        }
    }
}
