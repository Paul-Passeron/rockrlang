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

use std::{collections::HashMap, sync::Arc};

use crate::{
    Db,
    common::symbols::Symbol,
    hir::Mutability,
    name_resolve::type_expr::struct_item,
    parse_tree::type_expr::AstTypeExprDesc,
    resolved::{
        BuiltinTypeId, BuiltinTypeKind, ScopeOwnerId, StructId, TypeDefId, TypeRef,
        rehole, str_def,
    },
    typecheck::inference::{
        InferTy, InferenceCtx,
        implicit::{AsAstImplCtx, ImplicitContext},
    },
};

impl InferenceCtx<'_> {
    pub fn void_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::void(self.db)),
            fields: Vec::new(),
        }
    }

    pub fn int_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::int(self.db)),
            fields: Vec::new(),
        }
    }

    pub fn usize_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::usize(self.db)),
            fields: Vec::new(),
        }
    }

    pub fn bool_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::bool(self.db)),
            fields: Vec::new(),
        }
    }

    pub fn char_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::char(self.db)),
            fields: Vec::new(),
        }
    }

    pub fn str_ty(&self) -> InferTy {
        InferTy::Adt { def: str_def(self.db), fields: Vec::new() }
    }

    pub fn ptr_of(&self, ty: InferTy) -> InferTy {
        InferTy::Adt { def: BuiltinTypeId::const_ptr(self.db).into(), fields: vec![ty] }
    }

    pub fn slice_of(&self, ty: InferTy) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::slice(self.db)),
            fields: vec![ty],
        }
    }

    pub fn tuple_of(&self, tys: Vec<InferTy>) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::tuple(self.db)),
            fields: tys,
        }
    }

    pub fn cstr_ty(&self) -> InferTy {
        self.ptr_of(self.char_ty())
    }

    pub fn is_slice(&self, ty: &InferTy) -> Option<InferTy> {
        let (builtin, fields) = ty.as_builtin()?;
        match builtin.kind(self.db) {
            BuiltinTypeKind::Slice => Some(fields[0].clone()),
            _ => None,
        }
    }

    pub fn is_ref(&self, ty: &InferTy) -> Option<InferTy> {
        let (builtin, fields) = ty.as_builtin()?;
        match builtin.kind(self.db) {
            BuiltinTypeKind::Ref { .. } => {
                assert_eq!(fields.len(), 1);
                Some(fields[0].clone())
            }
            _ => None,
        }
    }

    pub fn is_ref_to_slice(&self, ty: &InferTy) -> Option<InferTy> {
        self.is_ref(ty).and_then(|ref ty| self.is_slice(ty))
    }

    pub fn is_ptr(&self, ty: &InferTy) -> Option<InferTy> {
        let (builtin, fields) = ty.as_builtin()?;
        match builtin.kind(self.db) {
            BuiltinTypeKind::Ptr { .. } => {
                assert_eq!(fields.len(), 1);
                Some(fields[0].clone())
            }
            _ => None,
        }
    }

    pub fn is_tuple<'a>(&self, ty: &'a InferTy) -> Option<&'a [InferTy]> {
        let (builtin, fields) = ty.as_builtin()?;
        match builtin.kind(self.db) {
            BuiltinTypeKind::Tuple => Some(fields),
            _ => None,
        }
    }

    pub fn static_allocate_type_ref(
        db: &dyn Db,
        type_ref: &TypeRef,
        ctx: &ImplicitContext,
    ) -> Option<InferTy> {
        match type_ref {
            TypeRef::Concrete(type_id) => Some(InferTy::Adt {
                def: type_id.def(db),
                fields: type_id
                    .args(db)
                    .iter()
                    .map(|ty| Self::static_allocate_type_ref(db, ty, ctx))
                    .collect::<Option<_>>()?,
            }),
            TypeRef::Param(type_param_id) => ctx.get_template(type_param_id.0),
            TypeRef::Zelf => ctx.zelf().cloned(),
            TypeRef::Associated(_symbol) => {
                eprintln!("TODO: Deal with associated types");
                None
            }
            TypeRef::Error | TypeRef::Unknown => None,
        }
    }

    pub fn allocate_type_ref(
        &mut self,
        type_ref: TypeRef,
        ctx: &ImplicitContext,
    ) -> InferTy {
        let type_ref = rehole(self.db, type_ref);
        match type_ref {
            TypeRef::Concrete(type_id) => InferTy::Adt {
                def: type_id.def(self.db),
                fields: type_id
                    .args(self.db)
                    .iter()
                    .map(|ty| self.allocate_type_ref(*ty, ctx))
                    .collect(),
            },
            TypeRef::Param(type_param_id) => match ctx.get_template(type_param_id.0) {
                Some(res) => res,
                None => self.fresh_var().into(),
            },
            TypeRef::Zelf => {
                if let Some(zelf) = ctx.zelf() {
                    zelf.clone()
                } else {
                    unreachable!()
                }
            }
            TypeRef::Associated(_symbol) => {
                // TODO
                self.fresh_var().into()
            }
            TypeRef::Error | TypeRef::Unknown => self.fresh_var().into(),
        }
    }

    pub fn allocate_ast_type_expr(
        &mut self,
        type_expr: &AstTypeExprDesc,
        ctx: &ImplicitContext,
    ) -> InferTy {
        self.allocate_type_ref(
            ctx.resolve(self.db, type_expr).unwrap_or(TypeRef::Error),
            ctx,
        )
    }

    pub fn is_struct(
        &mut self,
        ty: &InferTy,
    ) -> Option<(StructId, HashMap<Symbol, InferTy>)> {
        if let InferTy::Adt { def, fields } = &ty {
            match *def {
                TypeDefId::Struct(struct_id) => {
                    let templates = fields;
                    let ast = struct_item(self.db, struct_id.interned());
                    let templates = if templates.len() == ast.template_args.len() {
                        templates.iter().cloned().collect::<Arc<_>>()
                    } else {
                        ast.template_args
                            .iter()
                            .enumerate()
                            .map(|(i, _)| {
                                templates
                                    .get(i)
                                    .cloned()
                                    .unwrap_or_else(|| InferTy::Var(self.fresh_var()))
                            })
                            .collect::<Arc<_>>()
                    };
                    let module = struct_id.parent(self.db);
                    let ctx = ImplicitContext::new(
                        self.db,
                        ScopeOwnerId::Module(module),
                        &ast.template_args,
                        templates,
                        None,
                    );
                    let fields = ast
                        .fields
                        .iter()
                        .map(|field| {
                            let ty = self.allocate_ast_type_expr(&field.ty.data, &ctx);
                            (field.name, ty)
                        })
                        .collect();
                    Some((struct_id, fields))
                }
                TypeDefId::Builtin(id) => {
                    if matches!(id.kind(self.db), BuiltinTypeKind::Ref { .. }) {
                        // Auto-deref for ref to struct
                        self.is_struct(&fields[0])
                    } else {
                        None
                    }
                }
                TypeDefId::Enum(_) => None,
            }
        } else {
            None
        }
    }
}

impl InferTy {
    pub fn ptr_like(&self, db: &dyn Db) -> Option<(Mutability, &Self)> {
        self.as_adt().and_then(|(def, vals)| {
            let kind = def.is_ptr_like(db)?;
            Some((kind.mutability(), &vals[0]))
        })
    }
}
