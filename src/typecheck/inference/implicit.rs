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

#![allow(dead_code)]
// Implicit context module

use std::{collections::HashSet, sync::Arc};

use crate::{
    Db,
    common::symbols::Symbol,
    hir::{impl_items, interface_items, owning_module},
    name_resolve::{
        definition::{Definition, resolve_in_module},
        type_expr::{get_templates_of_fun, templates_of_owner},
    },
    parse_tree::{
        top_level::{AstImplItem, AstInterfaceItem, AstTemplateArg},
        type_expr::{AstTypeExpr, AstTypeExprDesc},
    },
    ril::{
        FunctionId, InterfaceRef, ModuleId, ScopeOwnerId, TypeId, TypeParamId,
        TypeRef, const_ptr_of, const_ref_of, mut_ptr_of, mut_ref_of, slice_of,
        tuple_of,
    },
    typecheck::inference::InferTy,
    unused,
};

pub struct AstImplicitContext {
    pub owner: ScopeOwnerId,
    pub template_asts: Arc<[AstTemplateArg]>,
}

impl AstImplicitContext {
    pub fn new(
        db: &dyn Db,
        owner: ScopeOwnerId,
        other_templates: Arc<[AstTemplateArg]>,
    ) -> ImplResult<Self> {
        let template_asts = templates_of_owner(db, owner);

        let mut template_names = HashSet::new();
        for ast in template_asts.as_ref() {
            if !template_names.insert(ast.name) {
                return Err(ImplicitCtxCreationError::DuplicateTemplateName(
                    ast.name,
                ));
            }
        }

        Ok(Self {
            owner,
            template_asts: template_asts
                .iter()
                .chain(other_templates.iter())
                .cloned()
                .collect(),
        })
    }

    pub fn into_implicit(
        self,
        templates: Arc<[InferTy]>,
        zelf: Option<InferTy>,
    ) -> ImplResult<ImplicitContext> {
        if self.template_asts.len() != templates.len() {
            dbg!(&self.template_asts);
            dbg!(&templates);
            return Err(ImplicitCtxCreationError::TemplateLenMismatch);
        }

        Ok(ImplicitContext {
            owner: self.owner,
            template_asts: self.template_asts,
            infer_templates: templates,
            zelf,
        })
    }
}

pub struct ImplicitContext {
    owner: ScopeOwnerId,
    template_asts: Arc<[AstTemplateArg]>,
    infer_templates: Arc<[InferTy]>,
    zelf: Option<InferTy>,
}

#[derive(Debug, Clone, Copy)]
pub enum ImplicitCtxCreationError {
    TemplateLenMismatch,
    DuplicateTemplateName(Symbol),
}

pub type ImplResult<T> = Result<T, ImplicitCtxCreationError>;

impl ImplicitContext {
    pub fn new(
        db: &dyn Db,
        owner: ScopeOwnerId,
        other_templates: Arc<[AstTemplateArg]>,
        infer_templates: Arc<[InferTy]>,
        zelf: Option<InferTy>,
    ) -> ImplResult<Self> {
        let ast_impl = AstImplicitContext::new(db, owner, other_templates)?;
        ast_impl.into_implicit(infer_templates, zelf)
    }

    pub fn from_function(
        db: &dyn Db,
        func: FunctionId,
        infer_templates: Arc<[InferTy]>,
        zelf: Option<InferTy>,
    ) -> ImplResult<Self> {
        let owner = func.parent(db);
        let full_templates = get_templates_of_fun(db, func.interned());
        let other_templates = full_templates
            .iter()
            .skip(templates_of_owner(db, owner).len())
            .cloned()
            .collect();
        Self::new(db, owner, other_templates, infer_templates, zelf)
    }
}

impl AstImplicitContext {
    pub fn get_interface_ref(&self, db: &dyn Db) -> Option<InterfaceRef> {
        match self.owner {
            ScopeOwnerId::Module(_) => None,
            ScopeOwnerId::Impl(impl_id) => impl_id.interface(db),
            ScopeOwnerId::Interface(interface_ref) => Some(interface_ref),
        }
    }

    pub fn get_associated_type_ast_template_arg(
        &self,
        db: &dyn Db,
        associated: Symbol,
    ) -> Option<AstTemplateArg> {
        let interface_ref = self.get_interface_ref(db)?;
        let items = interface_items(db, interface_ref.def(db).interned());
        items.iter().find_map(|item| match item {
            AstInterfaceItem::Type(arg) if arg.name == associated => {
                Some(arg.clone())
            }
            _ => None,
        })
    }

    pub fn get_associated_type_ast(
        &self,
        db: &dyn Db,
        associated: Symbol,
    ) -> Option<AstTypeExpr> {
        let impl_id = match self.owner {
            ScopeOwnerId::Impl(impl_id) => Some(impl_id),
            _ => None,
        }?;
        let items = impl_items(db, impl_id.interned());
        items.iter().find_map(|item| match item {
            AstImplItem::Type { name, ty, .. } if *name == associated => {
                Some(ty.clone())
            }
            _ => None,
        })
    }

    pub fn get_associated_type(
        &self,
        db: &dyn Db,
        associated: Symbol,
    ) -> Option<TypeRef> {
        if let Some(ast_ty) = self.get_associated_type_ast(db, associated) {
            self.resolve(db, &ast_ty.data)
        } else {
            let _ =
                self.get_associated_type_ast_template_arg(db, associated)?;
            // Fallback if the scope owner is an interface and actually contains
            // the associated type
            Some(TypeRef::Associated(associated))
        }
    }

    pub fn owning_module(&self, db: &dyn Db) -> ModuleId {
        owning_module(db, self.owner)
    }

    pub fn resolve(
        &self,
        db: &dyn Db,
        ty: &AstTypeExprDesc,
    ) -> Option<TypeRef> {
        fn _resolve(
            this: &AstImplicitContext,
            db: &dyn Db,
            ty: &AstTypeExprDesc,
            module: ModuleId,
            can_be_template: bool,
        ) -> Option<TypeRef> {
            match ty {
                AstTypeExprDesc::Named { name, args } => {
                    if *name == Symbol::new(db, "Self")
                        && module == this.owning_module(db)
                    {
                        return Some(TypeRef::Zelf);
                    } else if can_be_template
                        && let Some(pos) = this
                            .template_asts
                            .iter()
                            .position(|temp| temp.name == *name)
                    {
                        if !args.is_empty() {
                            return None;
                        }
                        return Some(TypeRef::Param(TypeParamId(pos)));
                    }
                    match resolve_in_module(db, *name, module)? {
                        Definition::Type(type_def_id) => {
                            let args = args
                                .iter()
                                .map(|arg| {
                                    arg.as_known().and_then(|arg| {
                                        this.resolve(db, &arg.data)
                                    })
                                })
                                .collect::<Option<Vec<_>>>()?;
                            Some(TypeRef::Concrete(TypeId::new(
                                db,
                                type_def_id,
                                args,
                            )))
                        }
                        _ => None,
                    }
                }
                AstTypeExprDesc::NameResolved { from, to } => {
                    if *from == Symbol::new(db, "Self")
                        && module == this.owning_module(db)
                    {
                        if let AstTypeExprDesc::Named { name, args } =
                            &to.as_ref().data
                            && args.is_empty()
                        {
                            this.get_associated_type(db, *name)
                        } else {
                            None
                        }
                    } else {
                        let new_module =
                            match resolve_in_module(db, *from, module)? {
                                Definition::Module(module_id) => module_id,
                                _ => return None,
                            };
                        // Cannot be a template because of the form A::B, so B
                        // here isn't a template
                        _resolve(this, db, &to.data, new_module, false)
                    }
                }
                AstTypeExprDesc::Ref { mutable, pointee } => {
                    let pointee = this.resolve(db, &pointee.data)?;
                    Some(if *mutable {
                        TypeRef::Concrete(mut_ref_of(db, pointee))
                    } else {
                        TypeRef::Concrete(const_ref_of(db, pointee))
                    })
                }
                AstTypeExprDesc::Pointer { mutable, pointee } => {
                    let pointee = this.resolve(db, &pointee.data)?;
                    Some(if *mutable {
                        TypeRef::Concrete(mut_ptr_of(db, pointee))
                    } else {
                        TypeRef::Concrete(const_ptr_of(db, pointee))
                    })
                }
                AstTypeExprDesc::Slice { ty, len } => {
                    assert!(len.is_none(), "TODO: length in slice");
                    let element = this.resolve(db, &ty.data)?;
                    Some(TypeRef::Concrete(slice_of(db, element)))
                }
                AstTypeExprDesc::Tuple(tys) => {
                    if tys.len() == 1 {
                        let t = tys.iter().next()?;
                        this.resolve(db, &t.data)
                    } else {
                        Some(TypeRef::Concrete(tuple_of(
                            db,
                            tys.iter()
                                .map(|ty| this.resolve(db, &ty.data))
                                .collect::<Option<_>>()?,
                        )))
                    }
                }
            }
        }
        _resolve(self, db, ty, self.owning_module(db), true)
    }

    pub fn resolve_interface(
        &self,
        db: &dyn Db,
        ty: &AstTypeExprDesc,
    ) -> Option<InterfaceRef> {
        fn _resolve(
            this: &AstImplicitContext,
            db: &dyn Db,
            ty: &AstTypeExprDesc,
            module: ModuleId,
        ) -> Option<InterfaceRef> {
            match ty {
                AstTypeExprDesc::Named { name, args } => {
                    if this.template_asts.iter().any(|temp| temp.name == *name)
                    {
                        return None;
                    }
                    match resolve_in_module(db, *name, module)? {
                        Definition::Interface(def) => {
                            let args = args
                                .iter()
                                .map(|arg| {
                                    arg.as_known().and_then(|arg| {
                                        this.resolve(db, &arg.data)
                                    })
                                })
                                .collect::<Option<Vec<_>>>()?;
                            Some(InterfaceRef::new(db, def, args))
                        }
                        _ => None,
                    }
                }
                AstTypeExprDesc::NameResolved { from, to } => {
                    let new_module = match resolve_in_module(db, *from, module)?
                    {
                        Definition::Module(module_id) => module_id,
                        _ => return None,
                    };
                    _resolve(this, db, &to.data, new_module)
                }
                AstTypeExprDesc::Tuple(_)
                | AstTypeExprDesc::Slice { .. }
                | AstTypeExprDesc::Pointer { .. }
                | AstTypeExprDesc::Ref { .. } => None,
            }
        }
        _resolve(self, db, ty, self.owning_module(db))
    }
}

impl ImplicitContext {
    pub fn get_template(&self, idx: usize) -> Option<InferTy> {
        self.infer_templates.get(idx).cloned()
    }

    pub fn get_templates(&self) -> Arc<[InferTy]> {
        self.infer_templates.clone()
    }

    pub fn zelf(&self) -> Option<&InferTy> {
        self.zelf.as_ref()
    }

    pub fn get_module(&self) -> Option<ModuleId> {
        match self.owner {
            ScopeOwnerId::Module(module_id) => Some(module_id),
            _ => None,
        }
    }

    pub fn owner_module(&self, db: &dyn Db) -> ModuleId {
        owning_module(db, self.owner)
    }

    pub fn get_owner(&self) -> ScopeOwnerId {
        self.owner
    }
}

pub trait AsAstImplCtx {
    fn get_ast_templates(&self) -> Arc<[AstTemplateArg]>;
    fn get_module(&self) -> Option<ModuleId>;
    fn owning_module(&self, db: &dyn Db) -> ModuleId;
    fn owner(&self, db: &dyn Db) -> ScopeOwnerId;

    fn get_associated_type_ast(
        &self,
        db: &dyn Db,
        associated: Symbol,
    ) -> Option<AstTypeExpr>;

    fn get_associated_type_ast_template_arg(
        &self,
        db: &dyn Db,
        associated: Symbol,
    ) -> Option<AstTemplateArg>;

    fn get_associated_type(
        &self,
        db: &dyn Db,
        associated: Symbol,
    ) -> Option<TypeRef> {
        if let Some(ast_ty) = self.get_associated_type_ast(db, associated) {
            self.resolve(db, &ast_ty.data)
        } else {
            let _ =
                self.get_associated_type_ast_template_arg(db, associated)?;
            // Fallback if the scope owner is an interface and actually contains
            // the associated type
            Some(TypeRef::Associated(associated))
        }
    }

    fn _resolve(
        this: &Self,
        db: &dyn Db,
        ty: &AstTypeExprDesc,
        module: ModuleId,
    ) -> Option<TypeRef> {
        match ty {
            AstTypeExprDesc::Named { name, args } => {
                if *name == Symbol::new(db, "Self")
                    && module == this.owning_module(db)
                {
                    return if let Some(zelf) =
                        this.owner(db).get_canonical_zelf(db)
                    {
                        Some(zelf)
                    } else {
                        panic!("No zelf ???")
                    };
                } else if let Some(pos) = this
                    .get_ast_templates()
                    .iter()
                    .position(|temp| temp.name == *name)
                {
                    if !args.is_empty() {
                        return None;
                    }
                    return Some(TypeRef::Param(TypeParamId(pos)));
                }
                match resolve_in_module(db, *name, module)? {
                    Definition::Type(type_def_id) => {
                        let args = args
                            .iter()
                            .map(|arg| {
                                arg.as_known()
                                    .and_then(|arg| this.resolve(db, &arg.data))
                            })
                            .collect::<Option<Vec<_>>>()?;
                        Some(TypeRef::Concrete(TypeId::new(
                            db,
                            type_def_id,
                            args,
                        )))
                    }
                    _ => None,
                }
            }
            AstTypeExprDesc::NameResolved { from, to } => {
                if *from == Symbol::new(db, "Self")
                    && module == this.owning_module(db)
                {
                    if let AstTypeExprDesc::Named { name, args } =
                        &to.as_ref().data
                        && args.is_empty()
                    {
                        this.get_associated_type(db, *name)
                    } else {
                        None
                    }
                } else {
                    let new_module = match resolve_in_module(db, *from, module)?
                    {
                        Definition::Module(module_id) => module_id,
                        _ => return None,
                    };
                    Self::_resolve(this, db, &to.data, new_module)
                }
            }
            AstTypeExprDesc::Ref { mutable, pointee } => {
                let pointee = this.resolve(db, &pointee.data)?;
                Some(if *mutable {
                    TypeRef::Concrete(mut_ref_of(db, pointee))
                } else {
                    TypeRef::Concrete(const_ref_of(db, pointee))
                })
            }
            AstTypeExprDesc::Pointer { mutable, pointee } => {
                let pointee = this.resolve(db, &pointee.data)?;
                Some(if *mutable {
                    TypeRef::Concrete(mut_ptr_of(db, pointee))
                } else {
                    TypeRef::Concrete(const_ptr_of(db, pointee))
                })
            }
            AstTypeExprDesc::Slice { ty, len } => {
                assert!(len.is_none(), "TODO: length in slice");
                let element = this.resolve(db, &ty.data)?;
                Some(TypeRef::Concrete(slice_of(db, element)))
            }
            AstTypeExprDesc::Tuple(tys) => {
                if tys.len() == 1 {
                    let t = tys.iter().next()?;
                    this.resolve(db, &t.data)
                } else {
                    Some(TypeRef::Concrete(tuple_of(
                        db,
                        tys.iter()
                            .map(|ty| this.resolve(db, &ty.data))
                            .collect::<Option<_>>()?,
                    )))
                }
            }
        }
    }

    fn resolve(&self, db: &dyn Db, ty: &AstTypeExprDesc) -> Option<TypeRef> {
        Self::_resolve(self, db, ty, self.owning_module(db))
    }
}

impl AsAstImplCtx for ImplicitContext {
    fn get_ast_templates(&self) -> Arc<[AstTemplateArg]> {
        self.template_asts.clone()
    }

    fn owning_module(&self, db: &dyn Db) -> ModuleId {
        owning_module(db, self.owner)
    }

    fn owner(&self, _: &dyn Db) -> ScopeOwnerId {
        self.owner
    }

    fn get_module(&self) -> Option<ModuleId> {
        match self.owner {
            ScopeOwnerId::Module(module) => Some(module),
            _ => None,
        }
    }

    fn get_associated_type_ast(
        &self,
        db: &dyn Db,
        associated: Symbol,
    ) -> Option<AstTypeExpr> {
        unused!(db);
        unused!(associated);
        todo!()
    }

    fn get_associated_type_ast_template_arg(
        &self,
        db: &dyn Db,
        associated: Symbol,
    ) -> Option<AstTemplateArg> {
        unused!(db);
        unused!(associated);
        todo!()
    }
}
