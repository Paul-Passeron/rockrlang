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
    common::symbols::Symbol,
    name_resolve::{
        core_module,
        definition::{Definition, resolve_in_module},
        module_items,
    },
    parse_tree::top_level::{AstInterface, AstTopLevelItemDesc},
    resolved::{
        EnumId, InterfaceId, InternedInterfaceId, InternedModuleId, ModuleId, StructId,
        TypeDefId,
    },
};

#[salsa::tracked(returns(copy))]
pub fn core_iter_module(db: &dyn Db) -> ModuleId {
    let core_module = core_module(db);
    let iter_module = resolve_in_module(db, Symbol::new(db, "iter"), core_module);
    match iter_module {
        Some(Definition::Module(id)) => id,
        _ => panic!("core::iter module not found"),
    }
}

#[salsa::tracked]
pub fn core_mem_module(db: &dyn Db) -> ModuleId {
    let core_module = core_module(db);
    let iter_module = resolve_in_module(db, Symbol::new(db, "mem"), core_module);
    match iter_module {
        Some(Definition::Module(id)) => id,
        _ => panic!("core::mem module not found"),
    }
}

#[salsa::tracked(returns(copy))]
pub fn core_opt_module(db: &dyn Db) -> ModuleId {
    let core_module = core_module(db);
    let iter_module = resolve_in_module(db, Symbol::new(db, "opt"), core_module);
    match iter_module {
        Some(Definition::Module(id)) => id,
        _ => panic!("core::opt module not found"),
    }
}

#[salsa::tracked(returns(copy))]
pub fn core_opt_enum(db: &dyn Db) -> EnumId {
    let opt_module = core_opt_module(db);
    let iter_module = resolve_in_module(db, Symbol::new(db, "opt"), opt_module);
    match iter_module {
        Some(Definition::Type(TypeDefId::Enum(id))) => id,
        _ => panic!("core::opt::opt enum not found"),
    }
}

#[salsa::tracked]
pub fn core_res_module(db: &dyn Db) -> ModuleId {
    let core_module = core_module(db);
    let iter_module = resolve_in_module(db, Symbol::new(db, "res"), core_module);
    match iter_module {
        Some(Definition::Module(id)) => id,
        _ => panic!("core::res module not found"),
    }
}

#[salsa::tracked]
pub fn core_iter_interface(db: &dyn Db) -> InterfaceId {
    let core_iter_module = core_iter_module(db);
    let interface = resolve_in_module(db, Symbol::new(db, "Iter"), core_iter_module);
    match interface {
        Some(Definition::Interface(id)) => id,
        _ => panic!("core::iter::Iter interface not found"),
    }
}

#[salsa::tracked(returns(copy))]
pub fn core_into_iterator_interface(db: &dyn Db) -> InterfaceId {
    let core_iter_module = core_iter_module(db);
    let interface =
        resolve_in_module(db, Symbol::new(db, "IntoIterator"), core_iter_module);
    match interface {
        Some(Definition::Interface(id)) => id,
        _ => panic!("core::iter::IntoIterator interface not found"),
    }
}

#[salsa::tracked(returns(copy))]
pub fn core_copy_interface(db: &dyn Db) -> InterfaceId {
    let core_mem_module = core_mem_module(db);
    let interface = resolve_in_module(db, Symbol::new(db, "Copy"), *core_mem_module);
    match interface {
        Some(Definition::Interface(id)) => id,
        _ => panic!("core::mem::Copy interface not found"),
    }
}

#[salsa::tracked(returns(copy))]
pub fn core_drop_interface(db: &dyn Db) -> InterfaceId {
    let core_mem_module = core_mem_module(db);
    let interface = resolve_in_module(db, Symbol::new(db, "Drop"), *core_mem_module);
    match interface {
        Some(Definition::Interface(id)) => id,
        _ => panic!("core::mem::Copy interface not found"),
    }
}

#[salsa::tracked(returns(copy))]
pub fn core_clone_interface(db: &dyn Db) -> InterfaceId {
    let core_mem_module = core_mem_module(db);
    let interface = resolve_in_module(db, Symbol::new(db, "Clone"), *core_mem_module);
    match interface {
        Some(Definition::Interface(id)) => id,
        _ => panic!("core::mem::Copy interface not found"),
    }
}


#[salsa::tracked(returns(copy))]
pub fn core_int_iter_struct(db: &dyn Db) -> StructId {
    let core_iter_module = core_iter_module(db);
    let int_iter = resolve_in_module(db, Symbol::new(db, "IntIter"), core_iter_module);
    match int_iter {
        Some(Definition::Type(TypeDefId::Struct(id))) => id,
        _ => panic!("core::iter::IntIter struct not found"),
    }
}

#[salsa::tracked(returns(deref))]
pub fn module_interfaces<'db>(
    db: &'db dyn Db,
    module: InternedModuleId<'db>,
) -> Vec<AstInterface> {
    module_items(db, module)
        .iter()
        .flatten()
        .filter_map(|item| match &item.data {
            AstTopLevelItemDesc::Interface(ast_interface) => Some(ast_interface.clone()),
            _ => None,
        })
        .collect()
}

pub fn interface_item<'db>(
    db: &'db dyn Db,
    interface: InternedInterfaceId<'db>,
) -> AstInterface {
    module_interfaces(db, interface.parent(db).interned())
        .iter()
        .find(|inter| inter.name.data == *interface.name(db))
        .expect("No interface id without source backing it")
        .clone()
}
