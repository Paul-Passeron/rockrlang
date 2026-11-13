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
    common::arch::{FloatWidth, IntWidth},
    layout::{
        aggregate::{finish_aggregate, struct_layout},
        fat_ptr::fat_ptr_layout_for,
        union::enum_layout,
    },
    resolved::{
        BuiltinTypeId, BuiltinTypeKind, IntWidthKind, TypeDefId, TypeRef,
        concrete_ty::ConcreteTy,
    },
    unused,
};
use itertools::Itertools;
use std::range::RangeInclusive;

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
    pub origin: Option<ConcreteTy>,
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

#[derive(Clone, Copy)]
pub struct Aggregate;

#[derive(Clone, Copy)]
pub struct Union;

#[derive(Clone, Copy)]
pub struct Zst;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueClass {
    Scalar(ScalarKind),
    Aggregate,
    Union,
    None,
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
        BuiltinTypeKind::Tuple if args.is_empty() => LayoutID::zst(db),
        BuiltinTypeKind::Void | BuiltinTypeKind::Never => LayoutID::zst(db),
        BuiltinTypeKind::Bool => LayoutID::int(db, IntWidth::I8),
        BuiltinTypeKind::Int { width, .. } => LayoutID::int(
            db,
            match width {
                IntWidthKind::System => target_width(db),
                IntWidthKind::Fixed(int_width) => int_width,
            },
        ),
        BuiltinTypeKind::Ref { .. } | BuiltinTypeKind::Ptr { .. } => LayoutID::ptr(db),
        BuiltinTypeKind::Tuple => {
            let layouts = args.iter().map(|ty| layout_of(db, *ty)).collect_vec();
            finish_aggregate(db, &layouts)
        }

        BuiltinTypeKind::Slice { .. } => {
            // {ptr, usize}
            finish_aggregate(
                db,
                &[LayoutID::ptr(db), LayoutID::int(db, target_width(db))],
            )
        }
        BuiltinTypeKind::Array { length } => {
            let layout = layout_of(db, args[0]);
            finish_aggregate(db, &(0..length).map(|_| layout).collect_vec())
        }
    }
}

pub fn layout_of(db: &dyn Db, ty: TypeRef) -> LayoutID {
    _layout_of(db, InternedTRef::new(db, ty)).into()
}

impl LayoutID {
    pub fn is_zst(self, db: &dyn Db) -> bool {
        matches!(self.data(db), LayoutData::ZeroSized)
    }
}

pub fn target_width(db: &dyn Db) -> IntWidth {
    unused!(db);
    // For the moment, we don't have a way of setting the target, so we just
    // use the user's machine's width.
    match usize::BITS {
        8 => IntWidth::I8,
        16 => IntWidth::I16,
        32 => IntWidth::I32,
        64 => IntWidth::I64,
        128 => IntWidth::I128,
        _ => unreachable!(),
    }
}

impl LIRTy {
    pub fn is_zst(&self, db: &dyn Db) -> bool {
        self.layout.is_zst(db)
    }

    pub fn class(&self, db: &dyn Db) -> ValueClass {
        match self.layout.data(db) {
            LayoutData::Scalar(kind) => ValueClass::Scalar(*kind),
            LayoutData::Aggregate(_) => ValueClass::Aggregate,
            LayoutData::ZeroSized => ValueClass::None,
            LayoutData::Union(_) => ValueClass::Union,
        }
    }

    pub fn is_ptr(self, db: &dyn Db) -> bool {
        matches!(self.layout.data(db), LayoutData::Scalar(ScalarKind::Ptr))
    }

    pub fn is_union(self, db: &dyn Db) -> bool {
        matches!(self.layout.data(db), LayoutData::Union(_))
    }

    pub fn union_layout(self, db: &dyn Db) -> Option<&VariantsLayout> {
        match self.layout.data(db) {
            LayoutData::Union(variants_layout) => Some(variants_layout),
            _ => None,
        }
    }

    pub fn is_aggregate(self, db: &dyn Db) -> bool {
        matches!(self.layout.data(db), LayoutData::Aggregate(_))
    }

    pub fn aggregate_layout(self, db: &dyn Db) -> Option<&AggregateLayout> {
        match self.layout.data(db) {
            LayoutData::Aggregate(aggregate_layout) => Some(aggregate_layout),
            _ => None,
        }
    }

    pub fn is_int(self, db: &dyn Db) -> bool {
        matches!(self.layout.data(db), LayoutData::Scalar(ScalarKind::Int(_)))
    }

    pub fn is_scalar(self, db: &dyn Db) -> bool {
        matches!(self.layout.data(db), LayoutData::Scalar(_))
    }

    pub fn scalar(self, db: &dyn Db) -> Option<ScalarKind> {
        match self.layout.data(db) {
            LayoutData::Scalar(kind) => Some(*kind),
            _ => None,
        }
    }
}

pub fn ordering_strategy(db: &dyn Db) -> FieldOrderingKind {
    unused!(db);
    FieldOrderingKind::SourceOrder
}

pub fn discriminant_strategy(db: &dyn Db) -> DiscriminantStrategyKind {
    unused!(db);
    DiscriminantStrategyKind::AlwaysTagged
}
