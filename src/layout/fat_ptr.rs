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
    layout::{AggregateLayout, LayoutData, LayoutID, Offset, Size},
    ril::TypeRef,
};

pub(super) fn fat_ptr_layout_for(db: &dyn Db, ty: TypeRef) -> LayoutID {
    let Some((_, id)) = ty.as_ref(db) else {
        panic!("Not a fat ptr");
    };
    // Only slices are fat ptrs for the moment
    let Some(_) = id.as_slice(db) else {
        panic!("Not a fat ptr");
    };

    let target_witdh = db.target_width();
    let target_size: Size = target_witdh.into();
    let size = target_size + target_size;

    let metadata_id = LayoutID::int(db, target_witdh);
    let ptr_id = LayoutID::ptr(db);

    LayoutID::new(
        db,
        size,
        target_witdh.into(),
        LayoutData::Aggregate(AggregateLayout {
            fields: vec![
                (Offset::ZERO, ptr_id),
                (Offset(target_size.bytes()), metadata_id),
            ],
            source_to_layout: vec![0, 1],
        }),
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
        true
    }
}
