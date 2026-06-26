use std::collections::HashMap;

use inkwell::{
    AddressSpace,
    context::Context,
    types::{AnyType, AnyTypeEnum, BasicTypeEnum},
};
use itertools::Itertools;

use crate::{
    Db,
    name_resolve::type_expr::struct_item,
    ril::{BuiltinTypeId, InternedTypeId, TypeDefId, TypeId, TypeRef},
};

#[salsa::tracked]
impl<'db> InternedTypeId<'db> {
    pub fn as_llvm<'ctx>(self, db: &'db dyn Db, c: &'ctx Context) -> AnyTypeEnum<'ctx> {
        let tref = TypeRef::Concrete(TypeId::from(self));
        let args = self
            .args(db)
            .iter()
            .map(|ty| ty.as_type_id().unwrap().interned().as_llvm(db, c))
            .collect_vec();
        match self.def(db) {
            TypeDefId::Builtin(builtin) => {
                if builtin == BuiltinTypeId::void(db) {
                    return c.void_type().as_any_type_enum();
                }
                if builtin == BuiltinTypeId::ptr(db)
                    || builtin == BuiltinTypeId::ref_(db)
                    || builtin == BuiltinTypeId::mut_ptr(db)
                    || builtin == BuiltinTypeId::mut_ref(db)
                {
                    return c.ptr_type(AddressSpace::default()).as_any_type_enum();
                }
                if builtin == BuiltinTypeId::int(db) {
                    return c.i32_type().as_any_type_enum();
                }
                if builtin == BuiltinTypeId::bool(db) {
                    return c.bool_type().as_any_type_enum();
                }
                if builtin == BuiltinTypeId::char(db) {
                    return c.i8_type().as_any_type_enum();
                }
                if builtin == BuiltinTypeId::usize(db) {
                    // TODO: actually wire up to the ptr sized type
                    return c.i64_type().as_any_type_enum();
                }
                if builtin == BuiltinTypeId::never(db) {
                    return c.void_type().as_any_type_enum()
                }

                todo!("{}", tref.to_string(db))
            }
            TypeDefId::Struct(_) => {
                let struct_ref = tref.as_struct_ref(db).unwrap();
                assert!(struct_ref.args.is_empty(), "TODO: generic structs");
                let name = tref.to_string(db);
                if let Some(llvm_ty) = c.get_struct_type(&name) {
                    llvm_ty.as_any_type_enum()
                } else {
                    let fields = struct_ref
                        .get_fields_ty(db)
                        .iter()
                        .map(|(name, ty)| {
                            (
                                *name,
                                ty.as_type_id()
                                    .unwrap()
                                    .interned()
                                    .as_llvm(db, c)
                                    .try_into()
                                    .unwrap(),
                            )
                        })
                        .collect::<HashMap<_, BasicTypeEnum<'_>>>();
                    // Source-ordered for the moment
                    let ordered_fields = struct_item(db, struct_ref.def.into())
                        .fields
                        .iter()
                        .map(|field| fields[&field.name])
                        .collect_vec();
                    c.struct_type(&ordered_fields, false).as_any_type_enum()
                }
            }
            ty => todo!("{}", ty.to_string(db)),
        }
    }
}
