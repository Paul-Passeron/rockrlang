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

use std::fmt;

use crate::{
    Db,
    mir::{
        MIR,
        basic_block::{MIRBasicBlock, MIRTerminator, Stmt},
        display::fmt_binop,
        operand::{
            MIRCallee, MIRConstant, MIRConstructorArgs, MIROperand, MIRPlace,
            MIRProjection, MIRRValue, MIRRValueKind, UnaryOperator,
        },
    },
};

pub struct MIRDotDisplay<'a> {
    pub mir: &'a MIR,
    pub db: &'a dyn Db,
    pub name: &'a str,
}

impl MIR {
    pub fn dot<'a>(&'a self, db: &'a dyn Db, name: &'a str) -> MIRDotDisplay<'a> {
        MIRDotDisplay {
            mir: self,
            db,
            name,
        }
    }
}

impl<'a> fmt::Display for MIRDotDisplay<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "digraph {} {{", dot_escape(self.name))?;
        writeln!(
            f,
            "  node [shape=record fontname=\"Courier New\" fontsize=10]"
        )?;
        writeln!(f, "  edge [fontname=\"Courier New\" fontsize=9]")?;
        writeln!(f)?;

        let entry_idx = self.mir.entry.into_raw();

        for (id, block) in self.mir.blocks.iter() {
            let idx = id.into_raw();
            let is_entry = idx == entry_idx;

            let label = build_block_label(self.db, idx, block);

            if is_entry {
                writeln!(
                    f,
                    "  bb{idx} [label={} style=filled fillcolor=lightblue]",
                    label
                )?;
            } else {
                writeln!(f, "  bb{idx} [label={}]", label)?;
            }

            fmt_edges(f, idx, &block.terminator)?;
        }

        writeln!(f, "}}")
    }
}

fn build_block_label(db: &dyn Db, idx: usize, block: &MIRBasicBlock) -> String {
    let mut label = String::from("\"");

    // Header
    label.push_str(&format!("{{bb{idx}"));
    if let Some(name) = &block.name {
        label.push_str(&format!(" ({name})"));
    }
    label.push('|');

    // Statements
    for stmt in &block.stmts {
        let s = fmt_stmt_str(db, stmt);
        label.push_str(&dot_escape_label(&s));
        label.push_str("\\l");
    }

    // Terminator
    let t = fmt_terminator_label(db, &block.terminator);
    label.push_str("---\\l");
    label.push_str(&dot_escape_label(&t));
    label.push_str("\\l");

    label.push_str("}\"");
    label
}

fn fmt_stmt_str(db: &dyn Db, stmt: &Stmt) -> String {
    match stmt {
        Stmt::Assign { dest, rvalue } => {
            format!(
                "{} = {}",
                fmt_place_str(db, dest),
                fmt_rvalue_str(db, rvalue)
            )
        }
    }
}

fn fmt_place_str(db: &dyn Db, place: &MIRPlace) -> String {
    let mut s = format!("_{}", place.local.into_raw());
    for proj in &place.projections {
        s.push_str(&fmt_projection_str(db, proj));
    }
    s
}

fn fmt_projection_str(db: &dyn Db, proj: &MIRProjection) -> String {
    match proj {
        MIRProjection::Deref => ".*".into(),
        MIRProjection::Field { name, .. } => format!(".{}", name.to_string(db)),
        MIRProjection::TupleField { index, .. } => format!(".{index}"),
        MIRProjection::Index { index } => format!("[{}]", fmt_operand_str(db, index)),
        MIRProjection::Downcast { variant } => format!(".downcast#{variant}"),
    }
}

fn fmt_operand_str(db: &dyn Db, operand: &MIROperand) -> String {
    match operand {
        MIROperand::Constant(c) => fmt_constant_str(db, c),
        MIROperand::Move(place) => format!("move {}", fmt_place_str(db, place)),
        MIROperand::Copy(place) => format!("copy {}", fmt_place_str(db, place)),
        MIROperand::Constructor {
            enum_ref,
            idx,
            args,
            ..
        } => {
            format!(
                "{}::#{idx}({})",
                enum_ref.def.name(db).to_string(db),
                fmt_constructor_args_str(db, args)
            )
        }
        MIROperand::StructLit {
            struct_ref, fields, ..
        } => {
            let fields_str = fields
                .iter()
                .map(|(name, op)| {
                    format!("{}: {}", name.to_string(db), fmt_operand_str(db, op))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{} {{ {fields_str} }}",
                struct_ref.def.name(db).to_string(db)
            )
        }
        MIROperand::Tuple(operands, _) => {
            let inner = operands
                .iter()
                .map(|op| fmt_operand_str(db, op))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({inner})")
        }
    }
}

fn fmt_constant_str(db: &dyn Db, constant: &MIRConstant) -> String {
    match constant {
        MIRConstant::Integer { value, ty } => format!("{value}_{}", ty.to_string(db)),
        MIRConstant::Bool(b) => b.to_string(),
        MIRConstant::CString {
            contents,
            null_terminated,
        } => {
            if *null_terminated {
                format!("c\"{contents}\"")
            } else {
                format!("\"{contents}\"")
            }
        }
    }
}

fn fmt_rvalue_str(db: &dyn Db, rvalue: &MIRRValue) -> String {
    match &rvalue.kind {
        MIRRValueKind::Use(operand) => fmt_operand_str(db, operand),
        MIRRValueKind::Ref(place, mutability) => {
            if mutability.is_mut() {
                format!("&mut {}", fmt_place_str(db, place))
            } else {
                format!("&{}", fmt_place_str(db, place))
            }
        }
        MIRRValueKind::AddressOf(place, mutability) => {
            if mutability.is_mut() {
                format!("addr_of_mut {}", fmt_place_str(db, place))
            } else {
                format!("addr_of {}", fmt_place_str(db, place))
            }
        }
        MIRRValueKind::BinOp(op, lhs, rhs) => {
            format!(
                "{} {} {}",
                fmt_operand_str(db, lhs),
                fmt_binop(op),
                fmt_operand_str(db, rhs)
            )
        }
        MIRRValueKind::UnaryOp(op, operand) => {
            let op_str = match op {
                UnaryOperator::Neg => "-",
                UnaryOperator::LNot => "!",
            };
            format!("{op_str}{}", fmt_operand_str(db, operand))
        }
        MIRRValueKind::Discriminant(place) => {
            format!("discriminant({})", fmt_place_str(db, place))
        }
        MIRRValueKind::Metadata(operand) => {
            format!("@metadata({})", fmt_operand_str(db, operand))
        }
        MIRRValueKind::SizeOf(ty) => format!("@sizeof({})", ty.to_string(db)),
    }
}

fn fmt_constructor_args_str(db: &dyn Db, args: &MIRConstructorArgs) -> String {
    match args {
        MIRConstructorArgs::None => String::new(),
        MIRConstructorArgs::Tuple(operands) => operands
            .iter()
            .map(|op| fmt_operand_str(db, op))
            .collect::<Vec<_>>()
            .join(", "),
        MIRConstructorArgs::Struct(fields) => fields
            .iter()
            .map(|(name, op)| {
                format!("{}: {}", name.to_string(db), fmt_operand_str(db, op))
            })
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn fmt_terminator_label(db: &dyn Db, terminator: &MIRTerminator) -> String {
    match terminator {
        MIRTerminator::Diverge => "diverge".into(),
        MIRTerminator::Goto { next, .. } => format!("goto bb{}", next.into_raw()),
        MIRTerminator::Return { value, .. } => match value {
            Some(operand) => format!("return {}", fmt_operand_str(db, operand)),
            None => "return".into(),
        },
        MIRTerminator::Branch {
            cond, then, else_, ..
        } => {
            format!(
                "branch {} ? bb{} : bb{}",
                fmt_operand_str(db, cond),
                then.into_raw(),
                else_.into_raw()
            )
        }
        MIRTerminator::Switch {
            discriminant,
            branches,
            default,
            ..
        } => {
            let arms = branches
                .iter()
                .map(|(v, bb)| format!("{v} => bb{}", bb.into_raw()))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "switch {}  {{ {arms}, _ => bb{} }}",
                fmt_operand_str(db, discriminant),
                default.into_raw()
            )
        }
        MIRTerminator::Call {
            callee,
            arguments,
            dest,
            next,
            ..
        } => {
            let args = arguments
                .iter()
                .map(|op| fmt_operand_str(db, op))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "_{} = call {}({args}) -> bb{}",
                dest.into_raw(),
                fmt_callee_str(db, callee),
                next.into_raw()
            )
        }
    }
}

fn fmt_edges(
    f: &mut fmt::Formatter<'_>,
    from: usize,
    terminator: &MIRTerminator,
) -> fmt::Result {
    match terminator {
        MIRTerminator::Diverge | MIRTerminator::Return { .. } => Ok(()),
        MIRTerminator::Goto { next, .. } => {
            writeln!(f, "  bb{from} -> bb{}", next.into_raw())
        }
        MIRTerminator::Branch { then, else_, .. } => {
            writeln!(f, "  bb{from} -> bb{} [label=\"true\"]", then.into_raw())?;
            writeln!(f, "  bb{from} -> bb{} [label=\"false\"]", else_.into_raw())
        }
        MIRTerminator::Switch {
            branches, default, ..
        } => {
            for (value, block) in branches {
                writeln!(
                    f,
                    "  bb{from} -> bb{} [label=\"{value}\"]",
                    block.into_raw()
                )?;
            }
            writeln!(f, "  bb{from} -> bb{} [label=\"_\"]", default.into_raw())
        }
        MIRTerminator::Call { next, .. } => {
            writeln!(f, "  bb{from} -> bb{}", next.into_raw())
        }
    }
}

fn fmt_callee_str(db: &dyn Db, callee: &MIRCallee) -> String {
    match callee {
        MIRCallee::Direct(fref) => {
            let mut s = fref.id.called_to_string(db);
            if !fref.args.is_empty() {
                s.push_str("::<");
                let args = fref
                    .args
                    .iter()
                    .map(|ty| ty.to_string(db))
                    .collect::<Vec<_>>()
                    .join(", ");
                s.push_str(&args);
                s.push('>');
            }
            s
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
