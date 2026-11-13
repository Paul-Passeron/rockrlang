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

use rockr_base::symbol::Symbol;
use rockr_base::{Db, Span};

use crate::model::{EnumRef, StructRef, Subst};
use crate::{
    model::{ScopeOwnerId, TypeDefId, TypeId, TypeRef},
    name_resolve::type_expr::templates_of_struct,
    phases::hir_to_thir::builder::ThirBuilder,
    thir::{LocalId, PlaceBase, ThirPlace},
    typecheck::inference::implicit::{AsAstImplCtx, AstImplicitContext},
};

impl TypeId {
    pub fn as_struct_ref(self, db: &dyn Db) -> Option<StructRef> {
        match self.def(db) {
            TypeDefId::Struct(struct_id) => {
                Some(StructRef { def: struct_id, args: self.args(db).to_vec() })
            }
            _ => None,
        }
    }

    pub fn as_enum_ref(self, db: &dyn Db) -> Option<EnumRef> {
        match self.def(db) {
            TypeDefId::Enum(enum_id) => {
                Some(EnumRef { def: enum_id, args: self.args(db).to_vec() })
            }
            _ => None,
        }
    }
}

impl TypeRef {
    pub fn as_struct_ref(self, db: &dyn Db) -> Option<StructRef> {
        self.as_type_id()?.as_struct_ref(db)
    }

    pub fn as_enum_ref(self, db: &dyn Db) -> Option<EnumRef> {
        self.as_type_id()?.as_enum_ref(db)
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
        let item = self.def.item(db);
        let found = item.fields.iter().find(|f| f.name == field)?;
        let ctx = AstImplicitContext::new(
            db,
            ScopeOwnerId::Module(self.def.parent(db)),
            templates_of_struct(db, self.def.into()),
        );
        ctx.resolve(db, &found.ty.data)
    }

    pub fn typeof_field(&self, db: &dyn Db, field: Symbol) -> Option<TypeRef> {
        Some(self.declared_typeof_field(db, field)?.subst(db, &Subst::params(&self.args)))
    }
}
