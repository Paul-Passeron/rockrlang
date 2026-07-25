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

use super::{FmtWriter, MIRWrite, StringWriter, fmt_stmt, fmt_terminator, mwrite};
use crate::{
    Db,
    mir::{Mir, basic_block::MIRTerminator},
};
use std::fmt::{self, Write};

pub struct MIRDotDisplay<'a> {
    pub mir: &'a Mir,
    pub db: &'a dyn Db,
    pub name: &'a str,
}

impl Mir {
    pub fn dot<'a>(&'a self, db: &'a dyn Db, name: &'a str) -> MIRDotDisplay<'a> {
        MIRDotDisplay { mir: self, db, name }
    }
}

impl fmt::Display for MIRDotDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut w = FmtWriter(f);
        let db = self.db;
        let mir = self.mir;

        mwrite!(w, "digraph {} {{\n", dot_escape(self.name))?;
        mwrite!(w, "  node [shape=record fontname=\"Courier New\" fontsize=10]\n")?;
        mwrite!(w, "  edge [fontname=\"Courier New\" fontsize=9]\n\n")?;

        let entry_idx = mir.entry.into_raw();

        for (id, block) in mir.blocks.iter() {
            let idx = id.into_raw();
            let label = build_block_label(db, idx, block);

            if idx == entry_idx {
                mwrite!(
                    w,
                    "  bb{idx} [label={label} style=filled fillcolor=lightblue]\n"
                )?;
            } else {
                mwrite!(w, "  bb{idx} [label={label}]\n")?;
            }

            fmt_dot_edges(&mut w, idx, &block.terminator)?;
        }

        mwrite!(w, "}}")
    }
}

/// Build the DOT record label for a block.
/// Uses `StringWriter` so we can apply `dot_escape_label` to each piece.
fn build_block_label(
    db: &dyn Db,
    idx: usize,
    block: &crate::mir::basic_block::MIRBasicBlock,
) -> String {
    let mut label = String::from("\"");
    let _ = write!(label, "{{bb{idx}");
    if let Some(name) = &block.name {
        let _ = write!(label, " ({name})");
    }
    label.push('|');

    for stmt in &block.stmts {
        let mut sw = StringWriter(String::new());
        let _ = fmt_stmt(&mut sw, db, stmt);
        label.push_str(&dot_escape_label(&sw.0));
        label.push_str("\\l");
    }

    let mut sw = StringWriter(String::new());
    let _ = fmt_terminator(&mut sw, db, &block.terminator);
    label.push_str("---\\l");
    label.push_str(&dot_escape_label(&sw.0));
    label.push_str("\\l");

    label.push_str("}\"");
    label
}

fn fmt_dot_edges<W: MIRWrite>(
    w: &mut W,
    from: usize,
    terminator: &MIRTerminator,
) -> fmt::Result {
    match terminator {
        MIRTerminator::Diverge | MIRTerminator::Return { .. } => Ok(()),

        MIRTerminator::Branch { then, else_, .. } => {
            mwrite!(w, "  bb{from} -> bb{} [label=\"true\"]\n", then.into_raw())?;
            mwrite!(w, "  bb{from} -> bb{} [label=\"false\"]\n", else_.into_raw())
        }
        MIRTerminator::Switch { branches, default, .. } => {
            for (value, block) in branches {
                mwrite!(w, "  bb{from} -> bb{} [label=\"{value}\"]\n", block.into_raw())?;
            }
            mwrite!(w, "  bb{from} -> bb{} [label=\"_\"]\n", default.into_raw())
        }
        MIRTerminator::Goto { next, .. } | MIRTerminator::Call { next, .. } => {
            mwrite!(w, "  bb{from} -> bb{}\n", next.into_raw())
        }
    }
}

fn dot_escape(s: &str) -> String {
    s.replace(['-', ':', ' '], "_")
}

fn dot_escape_label(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('<', "\\<")
        .replace('>', "\\>")
        .replace('|', "\\|")
}
