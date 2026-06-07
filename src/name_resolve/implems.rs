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
    common::unord::Set,
    name_resolve::{
        definition::{Definition, resolve_in_module},
        module_items, modules_in_package,
        type_expr::{TypeResolution, resolve_any_type_expr, resolve_type_expr},
    },
    parse_tree::{
        top_level::{AstTemplateArg, AstTopLevelItemDesc},
        type_expr::{AstTypeExpr, AstTypeExprDesc},
    },
    ril::{ImplId, ImplSource, InterfaceRef, InternedModuleId, ModuleId, Package},
};

pub fn resolve_type_expr_as_interface<'db>(
    db: &'db dyn Db,
    interface: &'db AstTypeExpr,
    module: ModuleId,
    template_args: &'db [AstTemplateArg],
    has_zelf: bool,
) -> Option<InterfaceRef> {
    match &interface.data {
        AstTypeExprDesc::Named { name, args } => {
            if args.is_empty()
                && let Some(_) = template_args.iter().position(|p| p.name == *name)
            {
                return None;
            }

            resolve_in_module(db, *name, module).and_then(|def| {
                if let Definition::Interface(interface_id) = def {
                    let resolved_args = args
                        .iter()
                        .map(|arg| {
                            match resolve_any_type_expr(
                                db,
                                arg,
                                module.interned(),
                                template_args,
                                has_zelf,
                            ) {
                                TypeResolution::Type(type_ref) => Some(type_ref),
                                _ => None,
                            }
                        })
                        .collect::<Option<Vec<_>>>();
                    resolved_args.map(|args| InterfaceRef::new(db, interface_id, args))
                } else {
                    None
                }
            })
        }
        AstTypeExprDesc::NameResolved { from, to } => {
            if let Some(Definition::Module(module)) = resolve_in_module(db, *from, module)
            {
                resolve_type_expr_as_interface(db, to, module, template_args, has_zelf)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[salsa::tracked]
pub fn module_impls<'db>(
    db: &'db dyn Db,
    module: InternedModuleId<'db>,
) -> Vec<ImplSource<'db>> {
    let mut res = vec![];
    if let Some(items) = module_items(db, module) {
        for item in items.iter().filter_map(|item| match &item.data {
            AstTopLevelItemDesc::Impl(ast_impl_block) => Some(ast_impl_block),
            _ => None,
        }) {
            let mut templates = vec![];
            for arg in &item.template_args {
                let mut constraints = Set::new();
                for constraint in &arg.constraints {
                    if let Some(interface) = resolve_type_expr_as_interface(
                        db,
                        constraint,
                        module.into(),
                        &item.template_args,
                        false,
                    ) {
                        constraints.insert(interface);
                    } else {
                        // TODO
                    }
                }
                templates.push(constraints);
            }
            if let TypeResolution::Type(implemented) = resolve_type_expr(
                db,
                &item.implemented,
                module,
                &item.template_args,
                false,
            )
            // Zelf types are not allowed here
            {
                match item.interface.as_ref().map(|interface| {
                    resolve_type_expr_as_interface(
                        db,
                        interface,
                        module.into(),
                        &item.template_args,
                        false,
                    )
                }) {
                    Some(Some(value)) => {
                        let impl_id = ImplId::new(
                            db,
                            module.into(),
                            implemented,
                            Some(value),
                            templates,
                        );
                        let src = ImplSource::new(
                            db,
                            impl_id,
                            module.into(),
                            item.template_args.clone(),
                            item.items.clone(),
                            item.span,
                        );
                        res.push(src);
                    }
                    None => {
                        let impl_id =
                            ImplId::new(db, module.into(), implemented, None, templates);
                        let src = ImplSource::new(
                            db,
                            impl_id,
                            module.into(),
                            item.template_args.clone(),
                            item.items.clone(),
                            item.span,
                        );
                        res.push(src);
                    }
                    Some(None) => {
                        // TODO: report error
                        println!(
                            "Error: Could not resolve implementation because of interface"
                        );
                    }
                }
            } else {
                println!("Error: Type could not resolve !");
            }
        }
    }
    res
}

#[salsa::tracked]
pub fn impls_in_package<'db>(
    db: &'db dyn Db,
    package: Package<'db>,
) -> Set<ImplSource<'db>> {
    Set::from_iter(
        modules_in_package(db, package)
            .into_iter()
            .flat_map(|module| module_impls(db, module.interned())),
    )
}
