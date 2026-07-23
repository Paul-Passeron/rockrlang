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
    resolved::{BuiltinTypeId, BuiltinTypeKind, TypeDefId, TypeRef},
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
    Tagged { offset: Offset, kind: IntWidth },
    Niche { offset: Offset, variant: LayoutID, valid_range: RangeInclusive<u128> },
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
            TypeDefId::Enum(enum_id) => enum_layout(db, enum_id, type_id.args(db)).into(),
        },
        _ => panic!("Expected a concrete type but got {}", ty.to_string(db)),
    }
}

fn builtin_layout(db: &dyn Db, builtin_id: BuiltinTypeId, args: &[TypeRef]) -> LayoutID {
    match builtin_id.kind(db) {
        BuiltinTypeKind::Void => LayoutID::zst(db),
        BuiltinTypeKind::Never => LayoutID::zst(db),
        BuiltinTypeKind::Bool => LayoutID::int(db, IntWidth::I8),
        BuiltinTypeKind::Int { width, .. } => LayoutID::int(db, width),
        BuiltinTypeKind::Ref { .. } | BuiltinTypeKind::Ptr { .. } => LayoutID::ptr(db),
        BuiltinTypeKind::Tuple => {
            if args.is_empty() {
                return LayoutID::zst(db);
            }
            let layouts = args.iter().map(|ty| layout_of(db, *ty)).collect_vec();
            finish_aggregate(db, layouts)
        }
        BuiltinTypeKind::Slice => todo!(),
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
