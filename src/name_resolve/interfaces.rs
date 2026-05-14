use crate::{
    Db,
    common::symbols::InternedSymbol,
    name_resolve::{
        core_module,
        definition::{Definition, resolve_in_module},
    },
    ril::{EnumId, InterfaceId, ModuleId, StructId, TypeDefId},
};

#[salsa::tracked]
pub fn core_iter_module<'db>(db: &'db dyn Db) -> ModuleId {
    let core_module = core_module(db);
    let iter_module = resolve_in_module(db, InternedSymbol::new(db, "iter"), core_module);
    match iter_module {
        Some(Definition::Module(id)) => id,
        _ => panic!("core::iter module not found"),
    }
}

#[salsa::tracked]
pub fn core_mem_module<'db>(db: &'db dyn Db) -> ModuleId {
    let core_module = core_module(db);
    let iter_module = resolve_in_module(db, InternedSymbol::new(db, "mem"), core_module);
    match iter_module {
        Some(Definition::Module(id)) => id,
        _ => panic!("core::mem module not found"),
    }
}

#[salsa::tracked]
pub fn core_opt_module<'db>(db: &'db dyn Db) -> ModuleId {
    let core_module = core_module(db);
    let iter_module = resolve_in_module(db, InternedSymbol::new(db, "opt"), core_module);
    match iter_module {
        Some(Definition::Module(id)) => id,
        _ => panic!("core::mem module not found"),
    }
}

#[salsa::tracked]
pub fn core_opt_enum<'db>(db: &'db dyn Db) -> EnumId {
    let opt_module = core_opt_module(db);
    let iter_module = resolve_in_module(db, InternedSymbol::new(db, "opt"), opt_module.interned());
    match iter_module {
        Some(Definition::Type(TypeDefId::Enum(id))) => id,
        _ => panic!("core::mem module not found"),
    }
}

#[salsa::tracked]
pub fn core_res_module<'db>(db: &'db dyn Db) -> ModuleId {
    let core_module = core_module(db);
    let iter_module = resolve_in_module(db, InternedSymbol::new(db, "opt"), core_module);
    match iter_module {
        Some(Definition::Module(id)) => id,
        _ => panic!("core::mem module not found"),
    }
}

#[salsa::tracked]
pub fn core_iter_interface<'db>(db: &'db dyn Db) -> InterfaceId {
    let core_iter_module = core_iter_module(db);
    let interface = resolve_in_module(
        db,
        InternedSymbol::new(db, "Iter"),
        core_iter_module.interned(),
    );
    match interface {
        Some(Definition::Interface(id)) => id,
        _ => panic!("core::iter::Iter interface not found"),
    }
}

#[salsa::tracked]
pub fn core_into_iterator_interface<'db>(db: &'db dyn Db) -> InterfaceId {
    let core_iter_module = core_iter_module(db);
    let interface = resolve_in_module(
        db,
        InternedSymbol::new(db, "IntoIterator"),
        core_iter_module.interned(),
    );
    match interface {
        Some(Definition::Interface(id)) => id,
        _ => panic!("core::iter::IntoIterator interface not found"),
    }
}

#[salsa::tracked]
pub fn core_int_iter_struct<'db>(db: &'db dyn Db) -> StructId {
    let core_iter_module = core_iter_module(db);
    let int_iter = resolve_in_module(
        db,
        InternedSymbol::new(db, "IntIter"),
        core_iter_module.interned(),
    );
    match int_iter {
        Some(Definition::Type(TypeDefId::Struct(id))) => id,
        _ => panic!("core::iter::IntIter struct not found"),
    }
}
