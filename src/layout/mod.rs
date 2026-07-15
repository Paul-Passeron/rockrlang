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

use std::range::RangeInclusive;

use itertools::Itertools;

use crate::{
    Db,
    layout::{
        aggregate::{finish_aggregate, struct_layout},
        fat_ptr::fat_ptr_layout_for,
        union::enum_layout,
    },
    ril::{BuiltinTypeId, TypeDefId, TypeRef},
};

pub mod aggregate;
pub mod fat_ptr;
pub mod union;
pub mod utils;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Offset(u64);

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Size(u64);

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Align(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IntWidth {
    I8,
    I16,
    I32,
    I64,
    I128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloatWidth {
    F32,
    F64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScalarKind {
    Int(IntWidth),
    Ptr,
    Float(FloatWidth),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Discriminant {
    None,
    Tagged {
        offset: Offset,
        kind: IntWidth,
    },
    Niche {
        offset: Offset,
        variant: LayoutID,
        valid_range: RangeInclusive<u128>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VariantsLayout {
    pub variants: Vec<LayoutID>,
    pub payload_offset: Offset,
    pub discriminant: Discriminant,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AggregateLayout {
    pub fields: Vec<(Offset, LayoutID)>,
    pub source_to_layout: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LayoutData {
    Scalar(ScalarKind),
    Aggregate(AggregateLayout),
    ZeroSized,
    Union(VariantsLayout),
}

#[salsa::interned]
pub struct Layout {
    pub size: Size,
    pub align: Align,
    #[returns(ref)]
    pub inner: self::LayoutData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutID(salsa::Id);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LIRTy {
    pub layout: LayoutID,
    pub origin: Option<TypeRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldOrderingKind {
    SourceOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldInput {
    size: Size,
    align: Align,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiscriminantStrategyKind {
    AlwaysTagged, /* dumb default: explicit tag if >1 variant, None if
                   * exactly 1 */
    NicheFilling, /* search for spare bit patterns before falling back to
                   * tagged, this will be implemented at
                   * a later date as I'm just trying to
                   * get the MVP up and running */
}

#[salsa::interned]
struct InternedTRef {
    inner: TypeRef,
}

#[salsa::tracked(returns(copy))]
fn _layout_of<'db>(db: &'db dyn Db, ty: InternedTRef<'db>) -> Layout<'db> {
    let ty = ty.inner(db);
    if ty.is_fat_ptr(db) {
        return fat_ptr_layout_for(db, *ty).into();
    }
    match ty {
        TypeRef::Concrete(type_id) => match type_id.def(db) {
            TypeDefId::Builtin(builtin_id) => {
                builtin_layout(db, builtin_id, type_id.args(db)).into()
            }
            TypeDefId::Struct(struct_id) => {
                struct_layout(db, struct_id, type_id.args(db)).into()
            }
            TypeDefId::Enum(enum_id) => {
                enum_layout(db, enum_id, type_id.args(db)).into()
            }
        },
        _ => panic!("Expected a concrete type but got {}", ty.to_string(db)),
    }
}

fn builtin_layout(
    db: &dyn Db,
    builtin_id: BuiltinTypeId,
    args: &[TypeRef],
) -> LayoutID {
    if builtin_id == BuiltinTypeId::tuple(db) {
        if args.is_empty() {
            return LayoutID::zst(db);
        }
        let layouts = args.iter().map(|ty| layout_of(db, *ty)).collect_vec();
        finish_aggregate(db, layouts)
    } else if builtin_id == BuiltinTypeId::bool(db)
        || builtin_id == BuiltinTypeId::char(db)
    {
        LayoutID::int(db, IntWidth::I8)
    } else if builtin_id == BuiltinTypeId::never(db)
        || builtin_id == BuiltinTypeId::void(db)
    {
        LayoutID::zst(db)
    } else if builtin_id == BuiltinTypeId::int(db) {
        LayoutID::int(db, IntWidth::I32)
    } else if builtin_id == BuiltinTypeId::ref_(db)
        || builtin_id == BuiltinTypeId::mut_ref(db)
        || builtin_id == BuiltinTypeId::ptr(db)
        || builtin_id == BuiltinTypeId::mut_ptr(db)
    {
        LayoutID::ptr(db)
    } else if builtin_id == BuiltinTypeId::usize(db) {
        LayoutID::int(db, db.target_width())
    } else if builtin_id == BuiltinTypeId::slice(db) {
        // This should have a length but it does not yet.
        // Let's assume (even that it's false for the moment) that the secnd
        // argument here is a dummy type whose type's name is the length (pretty
        // bad, I know :|)

        let inner_layout = layout_of(db, args[0]);

        // Horrible :(
        let length: usize = args[1]
            .as_type_id()
            .unwrap()
            .def(db)
            .name(db)
            .to_string(db)
            .parse()
            .unwrap();

        let align = inner_layout.align(db);

        let element_size = inner_layout.size(db).align_to(align);

        let data = LayoutData::Aggregate(AggregateLayout {
            fields: (0..length)
                .into_iter()
                .map(|i| (Offset::ZERO + element_size * i as u64, inner_layout))
                .collect(),
            source_to_layout: (0..length)
                .into_iter()
                .map(|i| i as u32)
                .collect_vec(),
        });

        LayoutID::new(db, element_size * length as u64, align, data)
    } else {
        unreachable!()
    }
}

pub fn layout_of(db: &dyn Db, ty: TypeRef) -> LayoutID {
    _layout_of(db, InternedTRef::new(db, ty)).into()
}

impl LIRTy {
    pub fn is_zst(&self, db: &dyn Db) -> bool {
        self.layout.is_zst(db)
    }
}

impl LayoutID {
    pub fn is_zst(&self, db: &dyn Db) -> bool {
        matches!(self.data(db), LayoutData::ZeroSized)
    }
}
