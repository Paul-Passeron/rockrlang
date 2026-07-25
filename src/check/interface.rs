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

use std::collections::{HashMap, hash_map::Entry};

use salsa::Accumulator;

use crate::{
    Db,
    common::{location::Span, symbols::Symbol},
    compiler::diagnostic::Diag,
    hir::interface_items,
    parse_tree::top_level::AstInterfaceItem,
    resolved::InterfaceId,
};

pub fn check_interface(db: &dyn Db, interface: InterfaceId) {
    check_ambiguous_interface_items(db, interface);
}

fn check_ambiguous_interface_items(db: &dyn Db, interface: InterfaceId) {
    let mut names: HashMap<Symbol, Span> = HashMap::new();
    for item in interface_items(db, interface.interned()) {
        let name = item.name();
        if let Entry::Vacant(e) = names.entry(name) {
            e.insert(item.name_span());
        } else {
            Diag::generic_error(
                format!(
                    "Cannot define the same name multiple time: `{}`",
                    name.to_string(db)
                ),
                item.name_span(),
            )
            .accumulate(db);
        }
    }
}

impl AstInterfaceItem {
    pub fn name(&self) -> Symbol {
        match self {
            Self::Type(arg) => arg.name,
            Self::Sig(sig) => sig.data.name.data,
        }
    }

    pub fn name_span(&self) -> Span {
        match self {
            Self::Type(arg) => arg.name_span,
            Self::Sig(sig) => sig.data.name.span,
        }
    }
}
