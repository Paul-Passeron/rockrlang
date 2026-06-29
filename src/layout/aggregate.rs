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
    layout::{
        AggregateLayout, Align, FieldInput, FieldOrderingKind, LayoutID, Offset, Size,
        layout_of,
    },
    name_resolve::type_expr::struct_item,
    ril::{StructId, TypeRef},
    thir::StructRef,
};

impl FieldOrderingKind {
    pub fn order_fields(self, fields: &[FieldInput]) -> Vec<u32> {
        match self {
            FieldOrderingKind::SourceOrder => (0..fields.len() as u32).collect(),
        }
    }
}

pub(super) fn finish_aggregate(db: &dyn Db, source_ordered: Vec<LayoutID>) -> LayoutID {
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

pub(super) fn struct_layout(
    db: &dyn Db,
    struct_id: StructId,
    args: &[TypeRef],
) -> LayoutID {
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
pub(super) fn aggregate_layout(
    db: &dyn Db,
    source_ordered: Vec<LayoutID>,
) -> AggregateLayout {
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

impl AggregateLayout {
    pub fn size(&self, db: &dyn Db) -> Size {
        // We get the biggest offset and add the size of its layout
        let Some((offset, layout)) = self.fields.iter().max_by_key(|f| f.0) else {
            return Size::ZERO;
        };
        Size(offset.bytes()) + layout.size(db)
    }
}
