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

use std::collections::HashMap;

use salsa::Accumulator;

use crate::{
    Db,
    check::fundef::check_fundef,
    common::{location::Span, symbols::Symbol},
    compiler::diagnostic::Diag,
    hir::{impl_items, signature::ZelfArg},
    parse_tree::top_level::AstImplItem,
    resolved::{FunctionId, ImplSource, ScopeOwnerId},
    typecheck::conformance::{ConformanceError, interface_conformance_errors},
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
    let iface = iref.to_string(db);
    let impl_span = *implem.span(db);

    for err in interface_conformance_errors(db, impl_id) {
        let (message, span) = match err {
            ConformanceError::MissingMethod(name) => (
                format!(
                    "missing method `{}` required by interface `{}`",
                    name.to_string(db),
                    iface
                ),
                impl_span,
            ),
            ConformanceError::MissingAssocType(name) => (
                format!(
                    "missing associated type `{}` required by interface `{}`",
                    name.to_string(db),
                    iface
                ),
                impl_span,
            ),
            ConformanceError::ExtraMethod { name, span } => (
                format!(
                    "method `{}` is not a member of interface `{}`",
                    name.to_string(db),
                    iface
                ),
                span,
            ),
            ConformanceError::ExtraAssocType { name, span } => (
                format!(
                    "associated type `{}` is not a member of interface `{}`",
                    name.to_string(db),
                    iface
                ),
                span,
            ),
            ConformanceError::ReceiverMismatch { method, span, expected, found } => (
                format!(
                    "method `{}` has an incompatible receiver: interface `{}` \
                     declares `{}`, implementation has `{}`",
                    method.to_string(db),
                    iface,
                    describe_receiver(expected),
                    describe_receiver(found),
                ),
                span,
            ),
            ConformanceError::ArityMismatch { method, span, expected, found } => (
                format!(
                    "method `{}` has {} parameter(s) but interface `{}` declares {}",
                    method.to_string(db),
                    found,
                    iface,
                    expected,
                ),
                span,
            ),
            ConformanceError::GenericArityMismatch { method, span, expected, found } => (
                format!(
                    "method `{}` has {} generic parameter(s) but interface `{}` \
                         declares {}",
                    method.to_string(db),
                    found,
                    iface,
                    expected,
                ),
                span,
            ),
            ConformanceError::ParamTypeMismatch {
                method,
                span,
                param,
                expected,
                found,
            } => (
                format!(
                    "parameter `{}` of method `{}` has type `{}` but interface `{}` \
                     declares `{}`",
                    param.to_string(db),
                    method.to_string(db),
                    found.to_string(db),
                    iface,
                    expected.to_string(db),
                ),
                span,
            ),
            ConformanceError::ReturnTypeMismatch { method, span, expected, found } => (
                format!(
                    "method `{}` returns `{}` but interface `{}` declares `{}`",
                    method.to_string(db),
                    found.to_string(db),
                    iface,
                    expected.to_string(db),
                ),
                span,
            ),
        };
        Diag::generic_error(message, span).accumulate(db);
    }
}

fn describe_receiver(zelf: Option<ZelfArg>) -> String {
    match zelf {
        Some(z) => z.to_string(),
        None => "no receiver".to_owned(),
    }
}

impl AstImplItem {
    pub fn name(&self) -> Symbol {
        match self {
            Self::Type { name, .. } => *name,
            Self::Fundef(fdef) => fdef.data.name.data,
        }
    }

    pub fn name_span(&self) -> Span {
        match self {
            Self::Type { name_span, .. } => *name_span,
            Self::Fundef(fdef) => fdef.data.name.span,
        }
    }
}
