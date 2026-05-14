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
    ril::{ImplId, ImplSource, InterfaceRef, InternedModuleId, Package},
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

            resolve_in_module(db, name.interned(), module).and_then(|def| {
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
                    resolved_args.map(|args| InterfaceRef::new(db, interface_id, args))
                } else {
                    None
                }
            })
        }
        AstTypeExprDesc::NameResolved { from, to } => {
            if let Some(Definition::Module(module)) = resolve_in_module(db, from.interned(), module)
            {
                resolve_type_expr_as_interface(db, to, module.interned(), template_args)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[salsa::tracked]
pub fn module_impls<'db>(db: &'db dyn Db, module: InternedModuleId<'db>) -> Vec<ImplSource<'db>> {
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
                        let src = ImplSource::new(
                            db,
                            impl_id,
                            module.into(),
                            item.template_args.clone(),
                            item.items.clone(),
                            item.span.clone(),
                        );
                        res.push(src);
                    }
                    None => {
                        let impl_id = ImplId::new(db, module.into(), implemented, None, templates);
                        let src = ImplSource::new(
                            db,
                            impl_id,
                            module.into(),
                            item.template_args.clone(),
                            item.items.clone(),
                            item.span.clone(),
                        );
                        res.push(src);
                    }
                    Some(None) => {
                        // TODO: report error
                        println!("Error: Could not resolve implementation because of interface");
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
pub fn impls_in_package<'db>(db: &'db dyn Db, package: Package<'db>) -> Set<ImplSource<'db>> {
    Set::from_iter(
        modules_in_package(db, package)
            .into_iter()
            .flat_map(|module| module_impls(db, module.interned())),
    )
}
