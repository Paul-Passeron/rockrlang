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

use std::collections::{HashMap, HashSet};

use salsa::Accumulator;

use crate::{
    Db,
    check::fundef::check_fundef,
    common::{location::Span, symbols::Symbol},
    compiler::diagnostic::Diag,
    hir::{
        impl_items, interface_items,
        signature::{FunctionSignature, ZelfArg, get_sig_of_function},
    },
    parse_tree::top_level::{AstImplItem, AstInterfaceItem},
    resolved::{
        FunctionId, ImplId, ImplSource, InterfaceRef, ScopeOwnerId, TypeId,
        TypeParamId, TypeRef,
    },
};

pub fn check_implem<'db>(db: &'db dyn Db, implem: ImplSource<'db>) {
    check_ambiguous_impl_items(db, implem);
    check_impl_items(db, implem);
    check_interface_conformance(db, implem);
}

fn check_ambiguous_impl_items<'db>(db: &'db dyn Db, implem: ImplSource<'db>) {
    let mut names: HashMap<Symbol, Span> = HashMap::new();
    for item in impl_items(db, implem.id(db).interned()) {
        let item_name = item.name();
        if let Some(_value) = names.get(&item_name).copied() {
            Diag::generic_error(
                format!(
                    "Cannot define the same name multiple time: `{}`",
                    item_name.to_string(db)
                ),
                item.name_span(),
            )
            .accumulate(db);
        } else {
            names.insert(item_name, item.name_span());
        }
    }
}

fn check_impl_items<'db>(db: &'db dyn Db, implem: ImplSource<'db>) {
    implem.items(db).iter().for_each(|item| match item {
        AstImplItem::Type { .. } => (),
        AstImplItem::Fundef(fdef) => {
            let id = FunctionId::new(
                db,
                fdef.data.name.data,
                ScopeOwnerId::Impl(*implem.id(db)),
            );
            check_fundef(db, id);
        }
    });
}

fn check_interface_conformance<'db>(db: &'db dyn Db, implem: ImplSource<'db>) {
    let impl_id = *implem.id(db);
    let Some(iref) = impl_id.interface(db) else {
        return;
    };

    let interface_id = iref.def(db);
    let iface_name = iref.to_string(db);
    let impl_span = *implem.span(db);

    let mut required_methods: HashSet<Symbol> = HashSet::new();
    let mut required_types: HashSet<Symbol> = HashSet::new();
    for item in interface_items(db, interface_id.interned()).iter() {
        match item {
            AstInterfaceItem::Sig(sig) => {
                required_methods.insert(sig.data.name.data);
            }
            AstInterfaceItem::Type(arg) => {
                required_types.insert(arg.name);
            }
        }
    }

    let mut provided_methods: HashMap<Symbol, Span> = HashMap::new();
    let mut provided_types: HashMap<Symbol, Span> = HashMap::new();
    for item in impl_items(db, impl_id.interned()) {
        match &item {
            AstImplItem::Fundef(_) => {
                provided_methods.entry(item.name()).or_insert(item.name_span());
            }
            AstImplItem::Type { .. } => {
                provided_types.entry(item.name()).or_insert(item.name_span());
            }
        }
    }

    for &name in &required_methods {
        match provided_methods.get(&name) {
            None => Diag::generic_error(
                format!(
                    "missing method `{}` required by interface `{}`",
                    name.to_string(db),
                    iface_name
                ),
                impl_span,
            )
            .accumulate(db),
            Some(&method_span) => check_method_signature(
                db, impl_id, iref, name, method_span, &iface_name,
            ),
        }
    }
    for &name in &required_types {
        if !provided_types.contains_key(&name) {
            Diag::generic_error(
                format!(
                    "missing associated type `{}` required by interface `{}`",
                    name.to_string(db),
                    iface_name
                ),
                impl_span,
            )
            .accumulate(db);
        }
    }

    for (name, span) in &provided_methods {
        if !required_methods.contains(name) {
            Diag::generic_error(
                format!(
                    "method `{}` is not a member of interface `{}`",
                    name.to_string(db),
                    iface_name
                ),
                *span,
            )
            .accumulate(db);
        }
    }
    for (name, span) in &provided_types {
        if !required_types.contains(name) {
            Diag::generic_error(
                format!(
                    "associated type `{}` is not a member of interface `{}`",
                    name.to_string(db),
                    iface_name
                ),
                *span,
            )
            .accumulate(db);
        }
    }
}

fn check_method_signature<'db>(
    db: &'db dyn Db,
    impl_id: ImplId,
    iref: InterfaceRef,
    method: Symbol,
    method_span: Span,
    iface_name: &str,
) {
    let iface_fn = FunctionId::new(db, method, ScopeOwnerId::Interface(iref));
    let impl_fn = FunctionId::new(db, method, ScopeOwnerId::Impl(impl_id));
    let iface_sig = get_sig_of_function(db, iface_fn.interned());
    let impl_sig = get_sig_of_function(db, impl_fn.interned());

    let method_name = method.to_string(db);

    if iface_sig.zelf != impl_sig.zelf {
        Diag::generic_error(
            format!(
                "method `{}` has an incompatible receiver: interface `{}` \
                 declares `{}`, implementation has `{}`",
                method_name,
                iface_name,
                describe_receiver(iface_sig.zelf),
                describe_receiver(impl_sig.zelf),
            ),
            method_span,
        )
        .accumulate(db);
    }

    if iface_sig.args.len() != impl_sig.args.len() {
        Diag::generic_error(
            format!(
                "method `{}` has {} parameter(s) but interface `{}` declares {}",
                method_name,
                impl_sig.args.len(),
                iface_name,
                iface_sig.args.len(),
            ),
            method_span,
        )
        .accumulate(db);
        return;
    }

    if iface_sig.added_templates.len() != impl_sig.added_templates.len() {
        Diag::generic_error(
            format!(
                "method `{}` has {} generic parameter(s) but interface `{}` \
                 declares {}",
                method_name,
                impl_sig.added_templates.len(),
                iface_name,
                iface_sig.added_templates.len(),
            ),
            method_span,
        )
        .accumulate(db);
        return;
    }

    let subs = build_substitution(db, impl_id, iref, &iface_sig);
    let implemented = impl_id.implemented(db);

    for ((_, iface_ty), (impl_arg_name, impl_ty)) in
        iface_sig.args.iter().zip(impl_sig.args.iter())
    {
        let expected = substitute(db, *iface_ty, &subs, implemented);
        if is_comparable(db, expected)
            && is_comparable(db, *impl_ty)
            && expected != *impl_ty
        {
            Diag::generic_error(
                format!(
                    "parameter `{}` of method `{}` has type `{}` but interface \
                     `{}` declares `{}`",
                    impl_arg_name.to_string(db),
                    method_name,
                    impl_ty.to_string(db),
                    iface_name,
                    expected.to_string(db),
                ),
                method_span,
            )
            .accumulate(db);
        }
    }

    let expected_ret = substitute(db, iface_sig.ret, &subs, implemented);
    if is_comparable(db, expected_ret)
        && is_comparable(db, impl_sig.ret)
        && expected_ret != impl_sig.ret
    {
        Diag::generic_error(
            format!(
                "method `{}` returns `{}` but interface `{}` declares `{}`",
                method_name,
                impl_sig.ret.to_string(db),
                iface_name,
                expected_ret.to_string(db),
            ),
            method_span,
        )
        .accumulate(db);
    }
}

fn build_substitution<'db>(
    db: &'db dyn Db,
    impl_id: ImplId,
    iref: InterfaceRef,
    iface_sig: &FunctionSignature,
) -> Vec<TypeRef> {
    let mut subs = iref.args(db).to_vec();
    let impl_template_count = impl_id.templates(db).len();
    for k in 0..iface_sig.added_templates.len() {
        subs.push(TypeRef::Param(TypeParamId(impl_template_count + k)));
    }
    subs
}

fn substitute(db: &dyn Db, ty: TypeRef, subs: &[TypeRef], zelf: TypeRef) -> TypeRef {
    match ty {
        TypeRef::Concrete(id) => TypeRef::Concrete(TypeId::new(
            db,
            id.def(db),
            id.args(db).iter().map(|t| substitute(db, *t, subs, zelf)).collect(),
        )),
        TypeRef::Param(p) => subs.get(p.0).copied().unwrap_or(TypeRef::Error),
        TypeRef::Zelf => zelf,
        other => other,
    }
}

fn is_comparable(db: &dyn Db, ty: TypeRef) -> bool {
    match ty {
        TypeRef::Concrete(id) => id.args(db).iter().all(|t| is_comparable(db, *t)),
        TypeRef::Param(_) => true,
        TypeRef::Associated(_)
        | TypeRef::Zelf
        | TypeRef::Error
        | TypeRef::Unknown => false,
    }
}

fn describe_receiver(zelf: Option<ZelfArg>) -> String {
    match zelf {
        Some(z) => z.to_string(),
        None => "no receiver".to_string(),
    }
}

impl AstImplItem {
    pub fn name(&self) -> Symbol {
        match self {
            AstImplItem::Type { name, .. } => *name,
            AstImplItem::Fundef(fdef) => fdef.data.name.data,
        }
    }

    pub fn name_span(&self) -> Span {
        match self {
            AstImplItem::Type { name_span, .. } => *name_span,
            AstImplItem::Fundef(fdef) => fdef.data.name.span,
        }
    }
}
