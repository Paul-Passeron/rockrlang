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

use itertools::Itertools;

use crate::{
    Db,
    check::thir::sanity_check::ConstructorType,
    layout::{
        Align, Discriminant, DiscriminantStrategyKind, IntWidth, LayoutID,
        Offset, Size, VariantsLayout, finish_aggregate, layout_of,
    },
    ril::{EnumId, TypeRef},
    thir::EnumRef,
};

use super::LayoutData;

fn layout_of_cons(db: &dyn Db, cons: &ConstructorType) -> LayoutID {
    let source_ordered = match cons {
        ConstructorType::Tuple(tys) => {
            tys.iter().map(|ty| layout_of(db, *ty)).collect_vec()
        }
        ConstructorType::Struct(named_tys) => {
            named_tys.iter().map(|(_, ty)| layout_of(db, *ty)).collect_vec()
        }
        ConstructorType::None => return LayoutID::zst(db),
    };
    finish_aggregate(db, source_ordered)
}

pub(super) fn enum_layout(
    db: &dyn Db,
    enum_id: EnumId,
    args: &[TypeRef],
) -> LayoutID {
    let enum_ref = EnumRef { def: enum_id, args: args.to_vec() };

    let variant_tys = enum_ref.variants(db);

    if variant_tys.is_empty() {
        // Unconstructible enum, we can just return a ZST
        return LayoutID::zst(db);
    }

    let source_ordered =
        variant_tys.iter().map(|cons| layout_of_cons(db, cons)).collect_vec();

    if source_ordered.len() == 1 {
        // Only a single variant, no discriminant needed
        // We'll have to check that when downcasting
        return source_ordered[0];
    }

    match db.discriminant_strategy() {
        DiscriminantStrategyKind::AlwaysTagged => {
            always_tagged_layout(db, source_ordered)
        }
        DiscriminantStrategyKind::NicheFilling => {
            todo!("Niche filling is not implemented")
        }
    }
}

fn always_tagged_layout(
    db: &dyn Db,
    source_ordered: Vec<LayoutID>,
) -> LayoutID {
    let tag_width = tag_width_for(source_ordered.len() as u32);
    let tag_align: Align = tag_width.into();
    let tag_size: Size = tag_width.into();

    let payload_align = source_ordered
        .iter()
        .map(|layout| layout.align(db))
        .max()
        .unwrap_or(Align::BYTE);

    let payload_size = source_ordered
        .iter()
        .map(|layout| layout.size(db))
        .max()
        .unwrap_or(Size::ZERO)
        .align_to(payload_align);

    if payload_size == Size::ZERO {
        // Just return the discriminant
        return LayoutID::int(db, tag_width);
    }

    // tag-first convention
    let payload_offset = (Offset::ZERO + tag_size).align_to(payload_align);

    let global_align = tag_align.max(payload_align);

    let size =
        (payload_offset + payload_size).align_to(global_align) - Offset::ZERO;

    let discriminant =
        Discriminant::Tagged { offset: Offset::ZERO, kind: tag_width };

    LayoutID::new(
        db,
        size,
        global_align,
        LayoutData::Union(VariantsLayout {
            variants: source_ordered,
            payload_offset,
            discriminant,
        }),
    )
}

pub fn tag_value_for_variant(
    db: &dyn Db,
    _enum_id: EnumId,
    source_idx: u32,
) -> Option<u128> {
    match db.discriminant_strategy() {
        DiscriminantStrategyKind::AlwaysTagged => Some(source_idx as u128),
        DiscriminantStrategyKind::NicheFilling => {
            todo!()
        }
    }
}

fn tag_width_for(n: u32) -> IntWidth {
    if n <= 1 << 8 {
        IntWidth::I8
    } else if n <= 1 << 16 {
        IntWidth::I16
    } else {
        IntWidth::I32
    }
}
