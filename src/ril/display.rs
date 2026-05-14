use std::fmt;

use crate::{Db, ril::EnumId};

use crate::name_resolve::type_expr::TypeResolution;

use super::{
    BuiltinTypeId, FunctionId, ImplId, InterfaceId, InterfaceRef, ModuleId, StructId, TypeDefId,
    TypeId, TypeParamId, TypeRef,
};

pub struct Display<'db, T> {
    pub value: T,
    pub db: &'db dyn Db,
}

impl<'db, T> Display<'db, T> {
    pub fn new(db: &'db dyn Db, value: T) -> Self {
        Self { value, db }
    }
}

// Convenience trait so you can call `.display(db)` on any RIL type
pub trait RilDisplay: Sized + Copy {
    fn display<'db>(self, db: &'db dyn Db) -> Display<'db, Self> {
        Display::new(db, self)
    }
}

impl RilDisplay for TypeRef {}
impl RilDisplay for TypeId {}
impl RilDisplay for TypeDefId {}
impl RilDisplay for BuiltinTypeId {}
impl RilDisplay for StructId {}
impl RilDisplay for EnumId {}
impl RilDisplay for InterfaceId {}
impl RilDisplay for InterfaceRef {}
impl RilDisplay for FunctionId {}
impl RilDisplay for ModuleId {}
impl RilDisplay for TypeParamId {}
impl RilDisplay for ImplId {}
impl RilDisplay for TypeResolution {}

// ---------------------------------------------------------------------------
// fmt::Display implementations
// ---------------------------------------------------------------------------

impl fmt::Display for Display<'_, TypeParamId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T{}", self.value.0)
    }
}

impl fmt::Display for Display<'_, BuiltinTypeId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.value.name(self.db).interned().contents(self.db)
        )
    }
}

impl fmt::Display for Display<'_, StructId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.value.name(self.db).interned().contents(self.db)
        )
    }
}

impl fmt::Display for Display<'_, EnumId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.value.name(self.db).interned().contents(self.db)
        )
    }
}

impl fmt::Display for Display<'_, InterfaceId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.value.name(self.db).interned().contents(self.db)
        )
    }
}

impl fmt::Display for Display<'_, TypeDefId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            TypeDefId::Builtin(b) => write!(f, "{}", b.display(self.db)),
            TypeDefId::Struct(s) => write!(f, "{}", s.display(self.db)),
            TypeDefId::Enum(e) => write!(f, "{}", e.display(self.db)),
        }
    }
}

impl fmt::Display for Display<'_, TypeId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let def = self.value.def(self.db);
        let args = self.value.args(self.db);
        write!(f, "{}", def.display(self.db))?;
        if !args.is_empty() {
            write!(f, "<")?;
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", arg.display(self.db))?;
            }
            write!(f, ">")?;
        }
        Ok(())
    }
}

impl fmt::Display for Display<'_, TypeRef> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            TypeRef::Concrete(type_id) => write!(f, "{}", type_id.display(self.db)),
            TypeRef::Param(param_id) => write!(f, "{}", param_id.display(self.db)),
            TypeRef::Zelf => write!(f, "Self"),
            TypeRef::Error => write!(f, "{{ERROR}}"),
        }
    }
}

impl fmt::Display for Display<'_, InterfaceRef> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let def = self.value.def(self.db);
        let args = self.value.args(self.db);
        write!(f, "{}", def.display(self.db))?;
        if !args.is_empty() {
            write!(f, "<")?;
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", arg.display(self.db))?;
            }
            write!(f, ">")?;
        }
        Ok(())
    }
}

impl fmt::Display for Display<'_, FunctionId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.value.name(self.db).interned().contents(self.db)
        )
    }
}

impl fmt::Display for Display<'_, ImplId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "impl ")?;

        let templates = self.value.templates(self.db);
        if !templates.is_empty() {
            write!(f, "<")?;
            for (i, constraints) in self.value.templates(self.db).iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "T{i}",)?;
                if !constraints.is_empty() {
                    write!(f, ": ")?;
                    for (j, constraint) in constraints.iter().enumerate() {
                        if j > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", constraint.display(self.db))?;
                    }
                }
            }
            write!(f, "> ")?;
        }
        if let Some(iface) = self.value.interface(self.db) {
            write!(f, "{} for ", iface.display(self.db))?;
        }
        write!(f, "{}", self.value.implemented(self.db).display(self.db))
    }
}

impl fmt::Display for Display<'_, TypeResolution> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value {
            TypeResolution::Error => write!(f, "<error>"),
            TypeResolution::Infer => write!(f, "_"),
            TypeResolution::Type(type_ref) => write!(f, "{}", type_ref.display(self.db)),
        }
    }
}

impl fmt::Display for Display<'_, ModuleId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Recursively build the full qualified path
        if let Some(parent) = self.value.parent(self.db) {
            // Don't print the @builtin prefix — it's an implementation detail
            let parent_name = parent.name(self.db).interned().contents(self.db);
            if parent_name != "@builtin" {
                write!(f, "{}::", parent.display(self.db))?;
            }
        }
        write!(
            f,
            "{}",
            self.value.name(self.db).interned().contents(self.db)
        )
    }
}

impl fmt::Debug for Display<'_, ImplId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Debug for Display<'_, TypeResolution> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Debug for Display<'_, TypeRef> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Debug for Display<'_, TypeId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Debug for Display<'_, ModuleId> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Debug for Display<'_, InterfaceRef> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
