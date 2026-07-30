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
    Db,
    common::{location::Span, symbols::Symbol},
    hir_to_thir::builder::ThirBuilder,
    name_resolve::type_expr::{struct_item, templates_of_struct},
    resolved::{ScopeOwnerId, TypeDefId, TypeRef},
    thir::{EnumRef, LocalId, PlaceBase, StructRef, ThirPlace},
    typecheck::inference::implicit::{AsAstImplCtx, AstImplicitContext},
};

impl TypeRef {
    pub fn as_struct_ref(self, db: &dyn Db) -> Option<StructRef> {
        let type_id = self.as_type_id()?;
        match type_id.def(db) {
            TypeDefId::Struct(struct_id) => {
                Some(StructRef { def: struct_id, args: type_id.args(db).to_vec() })
            }
            _ => None,
        }
    }

    pub fn as_enum_ref(self, db: &dyn Db) -> Option<EnumRef> {
        let type_id = self.as_type_id()?;
        match type_id.def(db) {
            TypeDefId::Enum(enum_id) => {
                Some(EnumRef { def: enum_id, args: type_id.args(db).to_vec() })
            }
            _ => None,
        }
    }
}

impl ThirPlace {
    pub fn local(local: LocalId, b: &ThirBuilder, span: Span) -> Self {
        let l = b.get_local(local);
        Self {
            base: PlaceBase::Local(local),
            projections: vec![],
            ty: l.ty,
            span,
            is_synthetic: l.is_synthetic,
        }
    }
}

impl StructRef {
    pub fn declared_typeof_field(&self, db: &dyn Db, field: Symbol) -> Option<TypeRef> {
        let item = struct_item(db, self.def.interned());
        let found = item.fields.iter().find(|f| f.name == field)?;
        let ctx = AstImplicitContext::new(
            db,
            ScopeOwnerId::Module(self.def.parent(db)),
            templates_of_struct(db, self.def.into()),
        );
        ctx.resolve(db, &found.ty.data)
    }

    pub fn typeof_field(&self, db: &dyn Db, field: Symbol) -> Option<TypeRef> {
        Some(self.declared_typeof_field(db, field)?.with_substitution(db, &self.args))
    }
}
