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
    hir::{PartialTypeArg, PartialTypeRef},
    name_resolve::type_expr::struct_item,
    parse_tree::type_expr::AstTypeExprDesc,
    ril::{BuiltinTypeId, ScopeOwnerId, StructId, TypeDefId, TypeRef, str_def},
    thir::inference::{
        InferTy, InferenceCtx,
        implicit::{AsAstImplCtx, ImplicitContext},
    },
};

impl<'db> InferenceCtx<'db> {
    pub fn void_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::void(self.db)),
            fields: Box::new([]),
        }
    }

    pub fn int_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::int(self.db)),
            fields: Box::new([]),
        }
    }

    pub fn bool_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::bool(self.db)),
            fields: Box::new([]),
        }
    }

    pub fn char_ty(&self) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::char(self.db)),
            fields: Box::new([]),
        }
    }

    pub fn str_ty(&self) -> InferTy {
        InferTy::Adt {
            def: str_def(self.db),
            fields: Box::new([]),
        }
    }

    pub fn ptr_of(&self, ty: InferTy) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::ptr(self.db)),
            fields: Box::new([ty]),
        }
    }

    pub fn slice_of(&self, ty: InferTy) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::slice(self.db)),
            fields: Box::new([ty]),
        }
    }

    pub fn tuple_of(&self, tys: Box<[InferTy]>) -> InferTy {
        InferTy::Adt {
            def: TypeDefId::Builtin(BuiltinTypeId::tuple(self.db)),
            fields: tys,
        }
    }

    pub fn cstr_ty(&self) -> InferTy {
        self.ptr_of(self.char_ty())
    }

    pub fn is_slice(&self, ty: &InferTy) -> Option<InferTy> {
        if let InferTy::Adt { def, fields } = ty
            && *def == TypeDefId::Builtin(BuiltinTypeId::slice(self.db))
        {
            assert_eq!(fields.len(), 1);
            Some(fields[0].clone())
        } else {
            None
        }
    }

    pub fn is_ref(&self, ty: &InferTy) -> Option<InferTy> {
        if let InferTy::Adt { def, fields } = &ty {
            if *def == TypeDefId::Builtin(BuiltinTypeId::ref_(self.db))
                || *def == TypeDefId::Builtin(BuiltinTypeId::mut_ref(self.db))
            {
                assert_eq!(fields.len(), 1);
                Some(fields[0].clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn is_ref_to_slice(&self, ty: &InferTy) -> Option<InferTy> {
        self.is_ref(ty).and_then(|ref ty| self.is_slice(ty))
    }

    pub fn is_ptr(&self, ty: &InferTy) -> Option<InferTy> {
        if let InferTy::Adt { def, fields } = &ty {
            if *def == TypeDefId::Builtin(BuiltinTypeId::ptr(self.db))
                || *def == TypeDefId::Builtin(BuiltinTypeId::mut_ptr(self.db))
            {
                assert_eq!(fields.len(), 1);
                Some(fields[0].clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn is_tuple<'a>(&self, ty: &'a InferTy) -> Option<&'a [InferTy]> {
        if let InferTy::Adt { def, fields } = &ty
            && *def == TypeDefId::Builtin(BuiltinTypeId::tuple(self.db))
        {
            Some(fields)
        } else {
            None
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
            TypeRef::Error => None,
            TypeRef::Zelf => ctx.zelf().cloned(),
            TypeRef::Associated(_symbol) => todo!(),
            TypeRef::Unknown => None,
        }
    }

    pub fn allocate_type_ref(&mut self, type_ref: &TypeRef, ctx: &ImplicitContext) -> InferTy {
        match type_ref {
            TypeRef::Concrete(type_id) => InferTy::Adt {
                def: type_id.def(self.db),
                fields: type_id
                    .args(self.db)
                    .iter()
                    .map(|ty| self.allocate_type_ref(ty, ctx))
                    .collect(),
            },
            TypeRef::Param(type_param_id) => match ctx.get_template(type_param_id.0) {
                Some(res) => res.clone(),
                None => {
                    todo!("Diagnostics");
                }
            },
            TypeRef::Error => panic!(),
            TypeRef::Zelf => {
                if let Some(zelf) = ctx.zelf() {
                    zelf.clone()
                } else {
                    unreachable!()
                }
            }
            TypeRef::Associated(_symbol) => todo!(),
            TypeRef::Unknown => InferTy::Var(self.fresh_var()),
        }
    }

    pub fn allocate_partial_type_arg(
        &mut self,
        arg: &PartialTypeArg,
        ctx: &ImplicitContext,
    ) -> InferTy {
        match arg {
            PartialTypeArg::Known(type_ref) => self.allocate_type_ref(type_ref, ctx),
            PartialTypeArg::Partial(partial_type_ref) => {
                self.allocate_partial_type_ref(partial_type_ref, ctx)
            }
            PartialTypeArg::Infer => InferTy::Var(self.fresh_var()),
        }
    }

    pub fn allocate_partial_type_ref(
        &mut self,
        type_ref: &PartialTypeRef,
        ctx: &ImplicitContext,
    ) -> InferTy {
        match type_ref {
            PartialTypeRef::Resolved(type_ref) => self.allocate_type_ref(type_ref, ctx),
            PartialTypeRef::WithHoles { def, args } => InferTy::Adt {
                def: *def,
                fields: args
                    .iter()
                    .map(|arg| self.allocate_partial_type_arg(arg, ctx))
                    .collect(),
            },
        }
    }

    pub fn allocate_ast_type_expr(
        &mut self,
        type_expr: &AstTypeExprDesc,
        ctx: &ImplicitContext,
    ) -> Option<InferTy> {
        ctx.resolve(self.db, type_expr)
            .map(|type_ref| self.allocate_type_ref(&type_ref, ctx))
    }

    pub fn is_struct(&mut self, ty: &InferTy) -> Option<(StructId, HashMap<Symbol, InferTy>)> {
        if let InferTy::Adt { def, fields } = &ty {
            match *def {
                TypeDefId::Struct(struct_id) => {
                    let templates = fields;
                    let ast = struct_item(self.db, struct_id.interned());
                    let templates = if templates.len() != ast.template_args.len() {
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
                    } else {
                        templates.iter().cloned().collect::<Arc<_>>()
                    };
                    let module = struct_id.parent(self.db);
                    let ctx = ImplicitContext::new(
                        self.db,
                        ScopeOwnerId::Module(module),
                        ast.template_args.iter().cloned().collect::<Arc<_>>(),
                        templates,
                        None,
                    )
                    .inspect_err(|err| println!("{err:#?}"))
                    .ok()?;
                    ast.fields
                        .iter()
                        .map(|field| {
                            self.allocate_ast_type_expr(&field.ty.data, &ctx)
                                .map(|ty| (field.name, ty))
                        })
                        .collect::<Option<_>>()
                        .map(|fields| (struct_id, fields))
                }
                TypeDefId::Builtin(id) => {
                    if id == BuiltinTypeId::mut_ref(self.db) || id == BuiltinTypeId::ref_(self.db) {
                        // Auto-deref for ref to struct
                        self.is_struct(&fields[0])
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    }
}
