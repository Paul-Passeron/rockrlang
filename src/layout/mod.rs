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

use std::{
    marker::PhantomData,
    ops::{Add, Mul, Sub},
    range::RangeInclusive,
};

use itertools::Itertools;

use crate::{
    Db,
    layout::LayoutData::ScalarPair,
    name_resolve::type_expr::struct_item,
    ril::{BuiltinTypeId, EnumId, StructId, TypeDefId, TypeRef},
    thir::StructRef,
};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Offset(u64);

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Size(u64);

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Align(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
        kind: ScalarKind,
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
    fields: Vec<(Offset, LayoutID)>,
    source_to_layout: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LayoutData {
    Scalar(ScalarKind),
    ScalarPair(ScalarKind, ScalarKind), /* Fat pointer, both are expected to be the
                                         * same width */
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

fn align_up(value: u64, align: u64) -> u64 {
    let remainder = value % align;
    if remainder == 0 { value } else { value + (align - remainder) }
}

impl Offset {
    pub const ZERO: Self = Self(0);

    pub fn bytes(self) -> u64 {
        self.0
    }

    pub fn bytes_from(self, other: Offset) -> u64 {
        if self < other {
            panic!("Cannot get bytes from a bigger offset")
        }
        self.0 - other.0
    }

    pub fn align_to(self, align: Align) -> Self {
        Self(align_up(self.bytes(), align.bytes()))
    }
}

impl Size {
    pub const ZERO: Self = Self(0);

    pub fn bytes(self) -> u64 {
        self.0 * 8
    }

    pub fn align_to(self, align: Align) -> Self {
        Self(align_up(self.bytes(), align.bytes()))
    }
}

impl Add<Size> for Offset {
    type Output = Self;

    fn add(self, rhs: Size) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Offset {
    type Output = Size;

    fn sub(self, rhs: Self) -> Size {
        Size(self.0 - rhs.0)
    }
}

impl Add for Size {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Mul<u64> for Size {
    type Output = Self;

    fn mul(self, rhs: u64) -> Self {
        Self(self.0 * rhs)
    }
}

impl Align {
    pub const BYTE: Self = Self(0);
    pub const B8: Self = Self::BYTE;
    pub const B16: Self = Self(1);
    pub const B32: Self = Self(2);
    pub const B64: Self = Self(3);
    pub const B128: Self = Self(4);

    pub fn bytes(self) -> u64 {
        1 << self.0
    }
}

impl<'a> From<Layout<'a>> for LayoutID {
    fn from(value: Layout<'a>) -> Self {
        Self(value.0)
    }
}

impl<'a> From<LayoutID> for Layout<'a> {
    fn from(value: LayoutID) -> Self {
        Self(value.0, PhantomData)
    }
}

impl LayoutID {
    pub fn interned<'a>(self) -> Layout<'a> {
        self.into()
    }

    pub fn data<'db>(self, db: &dyn Db) -> &LayoutData {
        self.interned().inner(db)
    }

    pub fn new(db: &dyn Db, size: Size, align: Align, data: LayoutData) -> Self {
        Layout::new(db, size, align, data).into()
    }

    pub fn size(self, db: &dyn Db) -> Size {
        self.interned().size(db)
    }

    pub fn align(self, db: &dyn Db) -> Align {
        self.interned().align(db)
    }
}

impl From<ScalarKind> for LayoutData {
    fn from(value: ScalarKind) -> Self {
        Self::Scalar(value)
    }
}

impl From<IntWidth> for Size {
    fn from(value: IntWidth) -> Self {
        match value {
            IntWidth::I8 => Self(1),
            IntWidth::I16 => Self(2),
            IntWidth::I32 => Self(4),
            IntWidth::I64 => Self(8),
            IntWidth::I128 => Self(16),
        }
    }
}

impl From<FloatWidth> for Size {
    fn from(value: FloatWidth) -> Self {
        match value {
            FloatWidth::F32 => Self(4),
            FloatWidth::F64 => Self(8),
        }
    }
}

#[salsa::interned]
struct InternedTRef {
    inner: TypeRef,
}

#[salsa::tracked]
fn _layout_of<'db>(db: &'db dyn Db, ty: InternedTRef<'db>) -> Layout<'db> {
    let ty = ty.inner(db);
    if ty.is_fat_ptr(db) {
        return fat_ptr_layout_for(db, ty).into();
    }
    match ty {
        TypeRef::Concrete(type_id) => match type_id.def(db) {
            TypeDefId::Builtin(builtin_id) => {
                builtin_layout(db, builtin_id, &type_id.args(db)).into()
            }
            TypeDefId::Struct(struct_id) => {
                struct_layout(db, struct_id, &type_id.args(db)).into()
            }
            TypeDefId::Enum(enum_id) => {
                enum_layout(db, enum_id, &type_id.args(db)).into()
            }
        },
        _ => panic!("Expected a concrete type"),
    }
}

fn enum_layout(db: &dyn Db, enum_id: EnumId, args: &[TypeRef]) -> LayoutID {
    todo!()
}

fn finish_aggregate(db: &dyn Db, source_ordered: Vec<LayoutID>) -> LayoutID {
    if source_ordered.is_empty() {
        return LayoutID::zst(db);
    }
    let align = source_ordered
        .iter()
        .map(|l| l.align(db))
        .max()
        .unwrap_or(Align::BYTE);
    let aggregated = aggregate_layout(db, source_ordered);
    let size = aggregated.size(db).align_to(align);
    LayoutID::new(db, size, align, aggregated.into())
}

fn struct_layout(db: &dyn Db, struct_id: StructId, args: &[TypeRef]) -> LayoutID {
    let struct_ref = StructRef {
        def: struct_id,
        args: args.to_vec(),
    };
    let fields = struct_ref.get_fields_ty(db);
    if fields.is_empty() {
        return LayoutID::zst(db);
    }

    let source_ordered_fields = struct_item(db, struct_id.into())
        .fields
        .iter()
        .map(|field| fields[&field.name])
        .collect_vec();

    let layouts = source_ordered_fields
        .iter()
        .map(|ty| layout_of(db, *ty))
        .collect_vec();

    finish_aggregate(db, layouts)
}

/// Seems like this is an NP-hard problem, so the algorithm will probably have
/// to be "good enough" if we want good perfomance
fn aggregate_layout(db: &dyn Db, source_ordered: Vec<LayoutID>) -> AggregateLayout {
    let fields = source_ordered
        .iter()
        .map(|layout| FieldInput {
            size: layout.size(db),
            align: layout.align(db),
        })
        .collect_vec();

    let ordering = db.ordering_strategy().order_fields(&fields);

    let mut ordered = ordering
        .iter()
        .copied()
        .enumerate()
        .map(|(src, layout_idx)| (layout_idx, src as u32))
        .collect_vec();
    ordered.sort_by_key(|&(layout_idx, _)| layout_idx);

    let mut offset = Offset::ZERO;
    let mut align = Align::BYTE;

    let mut fields = vec![(Offset::ZERO, LayoutID::zst(db)); source_ordered.len()];

    // We build the offsets by walking in layout order
    for (layout_idx, src_idx) in ordered {
        let field_layout = source_ordered[src_idx as usize];
        let field_align = field_layout.align(db);
        offset = offset.align_to(field_align);
        fields[layout_idx as usize] = (offset, field_layout);
        offset = offset + field_layout.size(db);
        align = align.max(field_align);
    }

    AggregateLayout {
        fields,
        source_to_layout: ordering,
    }
}

impl LayoutID {
    pub fn zst(db: &dyn Db) -> Self {
        Self::new(db, Size::ZERO, Align::BYTE, LayoutData::ZeroSized)
    }

    pub fn int(db: &dyn Db, width: IntWidth) -> Self {
        Self::new(
            db,
            width.into(),
            width.into(),
            ScalarKind::Int(width).into(),
        )
    }

    pub fn ptr(db: &dyn Db) -> Self {
        let width = db.target_width();
        Self::new(db, width.into(), width.into(), ScalarKind::Ptr.into())
    }
}

impl AggregateLayout {
    pub fn size(&self, db: &dyn Db) -> Size {
        // We get the biggest offset and add the size of its layout
        let Some((offset, layout)) = self.fields.iter().max_by_key(|f| f.0) else {
            return Size::ZERO;
        };
        Size(offset.bytes()) + layout.size(db)
    }
}

impl From<AggregateLayout> for LayoutData {
    fn from(value: AggregateLayout) -> Self {
        LayoutData::Aggregate(value)
    }
}

impl From<VariantsLayout> for LayoutData {
    fn from(value: VariantsLayout) -> Self {
        LayoutData::Union(value)
    }
}

fn builtin_layout(db: &dyn Db, builtin_id: BuiltinTypeId, args: &[TypeRef]) -> LayoutID {
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
            source_to_layout: (0..length).into_iter().map(|i| i as u32).collect_vec(),
        });

        LayoutID::new(db, element_size * length as u64, align, data)
    } else {
        unreachable!()
    }
}

pub fn layout_of(db: &dyn Db, ty: TypeRef) -> LayoutID {
    _layout_of(db, InternedTRef::new(db, ty)).into()
}

impl dyn Db {
    pub fn target_width(&self) -> IntWidth {
        // For the moment, we don't have a way of setting the target, so we just use the
        // user's machine's width.
        match usize::BITS {
            8 => IntWidth::I8,
            16 => IntWidth::I16,
            32 => IntWidth::I32,
            64 => IntWidth::I64,
            128 => IntWidth::I128,
            _ => unreachable!(),
        }
    }

    pub fn ordering_strategy(&self) -> FieldOrderingKind {
        FieldOrderingKind::SourceOrder
    }
}

fn fat_ptr_layout_for(db: &dyn Db, ty: TypeRef) -> LayoutID {
    let Some((_, id)) = ty.as_ref(db) else {
        panic!("Not a fat ptr");
    };
    // Only slices are fat ptrs for the moment
    let Some(_) = id.as_slice(db) else {
        panic!("Not a fat ptr");
    };

    let target_witdh = db.target_width();
    let size: Size = target_witdh.into();
    let size = size + size;

    let metadata_scalar = ScalarKind::Int(target_witdh);
    let ptr_scalar = ScalarKind::Ptr;

    LayoutID::new(
        db,
        size,
        target_witdh.into(),
        ScalarPair(ptr_scalar, metadata_scalar),
    )
}

impl TypeRef {
    pub fn is_fat_ptr(self, db: &dyn Db) -> bool {
        let Some((_, id)) = self.as_ref(db) else {
            return false;
        };
        // Only slices are fat ptrs for the moment
        let Some(_) = id.as_slice(db) else {
            return false;
        };
        return true;
    }
}

impl FieldOrderingKind {
    pub fn order_fields(self, fields: &[FieldInput]) -> Vec<u32> {
        match self {
            FieldOrderingKind::SourceOrder => (0..fields.len() as u32).collect(),
        }
    }
}

impl From<IntWidth> for Align {
    fn from(value: IntWidth) -> Self {
        match value {
            IntWidth::I8 => Self::B8,
            IntWidth::I16 => Self::B16,
            IntWidth::I32 => Self::B32,
            IntWidth::I64 => Self::B64,
            IntWidth::I128 => Self::B128,
        }
    }
}
