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
};

use crate::{
    Db,
    layout::{
        AggregateLayout, Align, FloatWidth, IntWidth, Layout, LayoutData,
        LayoutID, Offset, ScalarKind, Size, VariantsLayout,
    },
};

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
        self.0
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

    pub fn new(
        db: &dyn Db,
        size: Size,
        align: Align,
        data: LayoutData,
    ) -> Self {
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

impl From<FloatWidth> for Align {
    fn from(value: FloatWidth) -> Self {
        match value {
            FloatWidth::F32 => Align::B32,
            FloatWidth::F64 => Align::B64,
        }
    }
}

impl LayoutID {
    pub fn zst(db: &dyn Db) -> Self {
        Self::new(db, Size::ZERO, Align::BYTE, LayoutData::ZeroSized)
    }

    pub fn int(db: &dyn Db, width: IntWidth) -> Self {
        Self::new(db, width.into(), width.into(), ScalarKind::Int(width).into())
    }

    pub fn float(db: &dyn Db, width: FloatWidth) -> Self {
        Self::new(
            db,
            width.into(),
            width.into(),
            ScalarKind::Float(width).into(),
        )
    }

    pub fn ptr(db: &dyn Db) -> Self {
        let width = db.target_width();
        Self::new(db, width.into(), width.into(), ScalarKind::Ptr.into())
    }

    pub fn scalar(db: &dyn Db, kind: ScalarKind) -> Self {
        match kind {
            ScalarKind::Int(width) => Self::int(db, width),
            ScalarKind::Ptr => Self::ptr(db),
            ScalarKind::Float(width) => Self::float(db, width),
        }
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
