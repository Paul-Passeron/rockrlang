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

use itertools::Itertools;
use rockr::{
    Db,
    compiler::{FunctionSignature, get_sig_of_function},
    hir::Mutability,
    name_resolve::type_expr::get_templates_of_fun,
    printer::type_printer::TypePrinter,
    ril::{
        BuiltinTypeId, BuiltinTypeKind, FunctionId, ImplId, InterfaceRef, ScopeOwnerId,
        TypeDefId, TypeId, TypeRef,
    },
};

pub fn template_names(db: &dyn Db, func: FunctionId) -> Vec<String> {
    get_templates_of_fun(db, func.interned())
        .iter()
        .map(|t| t.name.to_string(db))
        .collect()
}

fn param_name(names: &[String], index: usize) -> String {
    names.get(index).cloned().unwrap_or_else(|| format!("T{index}"))
}

fn is_def_builtin(def: TypeDefId) -> Option<BuiltinTypeId> {
    match def {
        TypeDefId::Builtin(id) => Some(id),
        _ => None,
    }
}

fn builtin_pretty_print(db: &dyn Db, def: BuiltinTypeId) -> Option<(String, String)> {
    match def.kind(db) {
        BuiltinTypeKind::Ref { mutability } => {
            let muta = match mutability {
                Mutability::Const => "",
                Mutability::Mutable => "mut ",
            };
            Some((format!("&{muta}"), String::new()))
        }
        BuiltinTypeKind::Ptr { mutability } => {
            let muta = match mutability {
                Mutability::Const => "",
                Mutability::Mutable => "mut ",
            };
            Some((format!("*{muta}"), String::new()))
        }
        BuiltinTypeKind::Tuple => Some(("(".into(), ")".into())),
        BuiltinTypeKind::Slice => Some(("[".into(), "]".into())),
        _ => None,
    }
}

pub fn type_ref_to_named_string(db: &dyn Db, names: &[String], ty: TypeRef) -> String {
    match ty {
        TypeRef::Concrete(type_id) => type_id_to_named_string(db, names, type_id),
        TypeRef::Associated(symbol) => format!("Self::{}", symbol.to_string(db)),
        TypeRef::Param(type_param_id) => param_name(names, type_param_id.0),
        TypeRef::Zelf => "Self".to_string(),
        TypeRef::Error => "<ERROR>".to_string(),
        TypeRef::Unknown => "<???>".to_string(),
    }
}

pub fn type_id_to_named_string(db: &dyn Db, names: &[String], type_id: TypeId) -> String {
    let def = type_id.def(db);
    let args = type_id.args(db);
    if args.is_empty() {
        return TypePrinter::new().type_def_id_to_string(db, def);
    }
    let args_str =
        args.iter().map(|ty| type_ref_to_named_string(db, names, *ty)).join(", ");
    if let Some(id) = is_def_builtin(def)
        && let Some((prefix, suffix)) = builtin_pretty_print(db, id)
    {
        format!("{prefix}{args_str}{suffix}")
    } else {
        format!("{}<{args_str}>", TypePrinter::new().type_def_id_to_string(db, def))
    }
}

pub fn interface_ref_to_named_string(
    db: &dyn Db,
    names: &[String],
    interface_ref: InterfaceRef,
) -> String {
    let args = interface_ref.args(db);
    let def_str = TypePrinter::new().interface_id_to_string(db, interface_ref.def(db));
    if args.is_empty() {
        return def_str;
    }
    let args_str =
        args.iter().map(|ty| type_ref_to_named_string(db, names, *ty)).join(", ");
    format!("{def_str}<{args_str}>")
}

pub fn impl_id_to_named_string(db: &dyn Db, names: &[String], id: ImplId) -> String {
    let implemented = type_ref_to_named_string(db, names, id.implemented(db));
    match id.interface(db) {
        Some(inter) => {
            format!("<{implemented} as {}>", interface_ref_to_named_string(db, names, inter))
        }
        None => implemented,
    }
}

pub fn scope_owner_to_named_string(
    db: &dyn Db,
    names: &[String],
    scope_owner: ScopeOwnerId,
) -> String {
    match scope_owner {
        ScopeOwnerId::Module(module_id) => TypePrinter::new().module_to_string(db, module_id),
        ScopeOwnerId::Impl(impl_id) => impl_id_to_named_string(db, names, impl_id),
        ScopeOwnerId::Interface(interface_ref) => {
            interface_ref_to_named_string(db, names, interface_ref)
        }
    }
}

pub fn function_sig_to_named_string(
    db: &dyn Db,
    names: &[String],
    sig: &FunctionSignature,
) -> String {
    let mut s = sig.name.display(db).to_string();
    if !sig.added_templates.is_empty() || !sig.implicit_templates.is_empty() {
        let templates_str = sig
            .implicit_templates
            .iter()
            .chain(&sig.added_templates)
            .enumerate()
            .map(|(i, bounds)| {
                let name = param_name(names, i);
                if bounds.is_empty() {
                    name
                } else {
                    format!(
                        "{name}: {}",
                        bounds
                            .iter()
                            .map(|c| interface_ref_to_named_string(db, names, *c))
                            .join(" + ")
                    )
                }
            })
            .join(", ");
        s.push_str(&format!("<{templates_str}>"));
    }
    s.push('(');
    if let Some(arg) = sig.zelf {
        s.push_str(&arg.to_string());
        if !sig.args.is_empty() {
            s.push_str(", ");
        }
    }
    let args_str = sig
        .args
        .iter()
        .map(|(name, ty)| format!("{}: {}", name.display(db), type_ref_to_named_string(db, names, *ty)))
        .join(", ");
    s.push_str(&args_str);
    s.push_str("): ");
    s.push_str(&type_ref_to_named_string(db, names, sig.ret));
    s
}

pub fn function_id_to_named_string(db: &dyn Db, func: FunctionId) -> String {
    let names = template_names(db, func);
    let sig = get_sig_of_function(db, func.interned());
    format!(
        "{}::{}",
        scope_owner_to_named_string(db, &names, func.parent(db)),
        function_sig_to_named_string(db, &names, sig)
    )
}

pub fn type_ref_to_named_string_in(db: &dyn Db, func: FunctionId, ty: TypeRef) -> String {
    type_ref_to_named_string(db, &template_names(db, func), ty)
}
