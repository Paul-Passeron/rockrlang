// display/writer.rs

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
    pub fn display<'a>(&'a self, db: &'a dyn Db) -> MIRDisplay<'a, MIR> {
        MIRDisplay { value: self, db }
    }
}

impl<'a> fmt::Display for MIRDisplay<'a, MIR> {
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

fn fmt_local<W: super::MIRWrite>(
    w: &mut W,
    db: &dyn Db,
    local: &MIRLocal,
) -> fmt::Result {
    if local.mutability.is_mut() {
        w.write_str("mut ")?;
    }
    if let Some(name) = local.name {
        mwrite!(w, "{} :", name.to_string(db))?;
    }
    mwrite!(w, "{}", local.ty.to_string(db))
}
