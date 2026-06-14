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

use crate::{
    Db,
    check::{fundef::check_fundef},
    common::{location::Span, symbols::Symbol},
    hir::impl_items,
    parse_tree::top_level::AstImplItem,
    ril::{FunctionId, ImplSource, ScopeOwnerId},
};

pub fn check_implem<'db>(db: &'db dyn Db, implem: ImplSource<'db>) {
    check_ambiguous_impl_items(db, implem);
    check_impl_items(db, implem);
    // TODO: check interface-ey stuff
}

fn check_ambiguous_impl_items<'db>(db: &'db dyn Db, implem: ImplSource<'db>) {
    let mut names: HashMap<Symbol, Span> = HashMap::new();
    for item in impl_items(db, implem.id(db).interned()) {
        let item_name = item.name();
        if let Some(_value) = names.get(&item_name).copied() {
            todo!()
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
                ScopeOwnerId::Impl(implem.id(db)),
            );
            check_fundef(db, id);
        }
    });
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
