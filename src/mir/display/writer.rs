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

use super::{FmtWriter, fmt_block, fmt_local_id};
use crate::{
    Db,
    mir::{
        MIR, MIRLocal,
        display::{MIRWrite, mwrite},
    },
};
use std::fmt;

pub struct MIRDisplay<'a, T> {
    pub value: &'a T,
    pub db: &'a dyn Db,
}

impl MIR {
    pub fn display<'a>(&'a self, db: &'a dyn Db) -> MIRDisplay<'a, Self> {
        MIRDisplay { value: self, db }
    }
}

impl fmt::Display for MIRDisplay<'_, MIR> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut w = FmtWriter(f);
        let db = self.db;
        let mir = self.value;

        mwrite!(w, "mir {{\n")?;

        mwrite!(w, "  locals:\n")?;
        for (id, local) in mir.locals.iter() {
            mwrite!(w, "    ")?;
            fmt_local_id(&mut w, id)?;
            mwrite!(w, ": ")?;
            fmt_local(&mut w, db, local)?;
            mwrite!(w, "\n")?;
        }

        mwrite!(w, "\n  params: [\n")?;
        for param in &mir.parameters {
            mwrite!(w, "    ")?;
            fmt_local_id(&mut w, *param)?;
            mwrite!(w, ",\n")?;
        }
        mwrite!(w, "  ]\n")?;

        mwrite!(w, "\n  entry: bb{}\n", mir.entry.into_raw())?;

        for (id, block) in mir.blocks.iter() {
            let idx = id.into_raw();
            mwrite!(w, "\n  bb{idx}")?;
            if let Some(name) = &block.name {
                mwrite!(w, " ({name})")?;
            }
            mwrite!(w, ":\n")?;
            fmt_block(&mut w, db, block)?;
        }

        mwrite!(w, "}}")
    }
}

fn fmt_local<W: MIRWrite>(w: &mut W, db: &dyn Db, local: &MIRLocal) -> fmt::Result {
    if local.mutability.is_mut() {
        w.write_str("mut ")?;
    }
    if let Some(name) = local.name {
        mwrite!(w, "{} :", name.to_string(db))?;
    }
    mwrite!(w, "{}", local.ty.to_string(db))
}
