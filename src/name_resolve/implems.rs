use crate::{
    Db,
    common::unord::Set,
    name_resolve::{
        definition::{Definition, resolve_in_module},
        module_items,
        type_expr::{TypeResolution, resolve_any_type_expr, resolve_type_expr},
    },
    parse_tree::{
        top_level::{AstTemplateArg, AstTopLevelItemDesc},
        type_expr::{AstTypeExpr, AstTypeExprDesc},
    },
    ril::{ImplId, InterfaceRef, InternedModuleId},
};

pub fn resolve_type_expr_as_interface<'db>(
    db: &'db dyn Db,
    interface: &'db AstTypeExpr,
    module: InternedModuleId<'db>,
    template_args: &'db [AstTemplateArg],
) -> Option<InterfaceRef> {
    match &interface.data {
        AstTypeExprDesc::Named { name, args } => {
            if args.is_empty()
                && let Some(_) = template_args.iter().position(|p| p.name == *name)
            {
                return None;
            }

            resolve_in_module(db, name.interned(), module).map_or(None, |def| {
                if let Definition::Interface(interface_id) = def {
                    let resolved_args = args
                        .iter()
                        .map(
                            |arg| match resolve_any_type_expr(db, arg, module, template_args) {
                                TypeResolution::Type(type_ref) => Some(type_ref),
                                _ => None,
                            },
                        )
                        .collect::<Option<Vec<_>>>();
                    match resolved_args {
                        Some(resolved_args) => {
                            Some(InterfaceRef::new(db, interface_id, resolved_args))
                        }
                        None => None,
                    }
                } else {
                    None
                }
            })
        }
        AstTypeExprDesc::NameResolved { from, to } => {
            if let Some(Definition::Module(module)) = resolve_in_module(db, from.interned(), module)
            {
                resolve_type_expr_as_interface(db, &**to, module.interned(), template_args)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[salsa::tracked]
pub fn module_impls<'db>(db: &'db dyn Db, module: InternedModuleId<'db>) -> Vec<ImplId> {
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
                    if let Some(interface) =
                        resolve_type_expr_as_interface(db, constraint, module, &item.template_args)
                    {
                        constraints.insert(interface);
                    } else {
                        // TODO
                    }
                }
                templates.push(constraints);
            }
            if let TypeResolution::Type(implemented) =
                resolve_type_expr(db, &item.implemented, module, &item.template_args)
            {
                match item.interface.as_ref().map(|interface| {
                    resolve_type_expr_as_interface(db, interface, module, &item.template_args)
                }) {
                    Some(Some(value)) => {
                        let impl_id =
                            ImplId::new(db, module.into(), implemented, Some(value), templates);
                        res.push(impl_id);
                    }
                    None => {
                        let impl_id = ImplId::new(db, module.into(), implemented, None, templates);
                        res.push(impl_id);
                    }
                    Some(None) => {
                        // TODO: report error
                        println!("Error: Could not resolve implementation because of interface");
                        ()
                    }
                }
            } else {
                println!("Error: Type could not resolve !");
            }
        }
    }
    res
}
