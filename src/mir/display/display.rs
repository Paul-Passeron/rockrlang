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
        MIR, MIRLocal, MIRLocalID,
        basic_block::{MIRBasicBlock, MIRTerminator, Stmt},
        display::fmt_binop,
        operand::{
            MIRCallee, MIRConstant, MIRConstructorArgs, MIROperand, MIRPlace,
            MIRProjection, MIRRValue, MIRRValueKind, UnaryOperator,
        },
    },
};

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
        writeln!(f, "mir {{")?;

        writeln!(f, "  locals:")?;
        for (id, local) in self.value.locals.iter() {
            write!(f, "    ")?;
            fmt_local_id(f, id)?;
            write!(f, ": ")?;
            fmt_local(f, self.db, local)?;
            writeln!(f)?;
        }

        writeln!(f)?;
        writeln!(f, "  params: [")?;
        for param in &self.value.parameters {
            write!(f, "    ")?;
            fmt_local_id(f, *param)?;
            writeln!(f, ",")?;
        }
        writeln!(f, "  ]")?;

        writeln!(f)?;
        writeln!(f, "  entry: bb{}", self.value.entry.into_raw())?;

        writeln!(f)?;
        for (id, block) in self.value.blocks.iter() {
            let idx = id.into_raw();
            write!(f, "  bb{idx}")?;
            if let Some(name) = &block.name {
                write!(f, " ({name})")?;
            }
            writeln!(f, ":")?;
            fmt_block(f, self.db, block)?;
            writeln!(f)?;
        }

        write!(f, "}}")
    }
}

fn fmt_local_id(f: &mut fmt::Formatter<'_>, id: MIRLocalID) -> fmt::Result {
    write!(f, "_{}", id.into_raw())
}

fn fmt_local(f: &mut fmt::Formatter<'_>, db: &dyn Db, local: &MIRLocal) -> fmt::Result {
    if local.mutability.is_mut() {
        write!(f, "mut ")?;
    }
    if let Some(name) = local.name {
        write!(f, "{} :", name.to_string(db))?;
    }
    write!(f, "{}", local.ty.to_string(db))
}

fn fmt_block(
    f: &mut fmt::Formatter<'_>,
    db: &dyn Db,
    block: &MIRBasicBlock,
) -> fmt::Result {
    for stmt in &block.stmts {
        write!(f, "    ")?;
        fmt_stmt(f, db, stmt)?;
        writeln!(f)?;
    }
    write!(f, "    ")?;
    fmt_terminator(f, db, &block.terminator)?;
    writeln!(f)
}

fn fmt_stmt(f: &mut fmt::Formatter<'_>, db: &dyn Db, stmt: &Stmt) -> fmt::Result {
    match stmt {
        Stmt::Assign { dest, rvalue } => {
            fmt_place(f, db, dest)?;
            write!(f, " = ")?;
            fmt_rvalue(f, db, rvalue)
        }
    }
}

fn fmt_place(f: &mut fmt::Formatter<'_>, db: &dyn Db, place: &MIRPlace) -> fmt::Result {
    fmt_local_id(f, place.local)?;
    for proj in &place.projections {
        fmt_projection(f, db, proj)?;
    }
    Ok(())
}

fn fmt_projection(
    f: &mut fmt::Formatter<'_>,
    db: &dyn Db,
    proj: &MIRProjection,
) -> fmt::Result {
    match proj {
        MIRProjection::Deref => write!(f, ".*"),
        MIRProjection::Field { name, .. } => write!(f, ".{}", name.to_string(db)),
        MIRProjection::TupleField { index, .. } => write!(f, ".{index}"),
        MIRProjection::Index { index } => {
            write!(f, "[")?;
            fmt_operand(f, db, index)?;
            write!(f, "]")
        }
        MIRProjection::Downcast { variant } => write!(f, ".downcast#{variant}"),
    }
}

fn fmt_operand(
    f: &mut fmt::Formatter<'_>,
    db: &dyn Db,
    operand: &MIROperand,
) -> fmt::Result {
    match operand {
        MIROperand::Constant(c) => fmt_constant(f, db, c),
        MIROperand::Move(place) => {
            write!(f, "move ")?;
            fmt_place(f, db, place)
        }
        MIROperand::Copy(place) => {
            write!(f, "copy ")?;
            fmt_place(f, db, place)
        }
        MIROperand::Constructor {
            enum_ref,
            idx,
            args,
        } => {
            write!(f, "{}::#{idx}(", enum_ref.def.name(db).to_string(db))?;
            fmt_constructor_args(f, db, args)?;
            write!(f, ")")
        }
        MIROperand::StructLit { struct_ref, fields } => {
            write!(f, "{} {{", struct_ref.def.name(db).to_string(db))?;
            let mut first = true;
            for (name, operand) in fields {
                if !first {
                    write!(f, ", ")?;
                }
                first = false;
                write!(f, "{}: ", name.to_string(db))?;
                fmt_operand(f, db, operand)?;
            }
            write!(f, "}}")
        }
        MIROperand::Tuple(operands) => {
            write!(f, "(")?;
            for (i, operand) in operands.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                fmt_operand(f, db, operand)?;
            }
            write!(f, ")")
        }
    }
}

fn fmt_constant(
    f: &mut fmt::Formatter<'_>,
    db: &dyn Db,
    constant: &MIRConstant,
) -> fmt::Result {
    match constant {
        MIRConstant::Integer { value, ty } => {
            write!(f, "{value}_{}", ty.to_string(db))
        }
        MIRConstant::Bool(b) => write!(f, "{b}"),
        MIRConstant::CString {
            contents,
            null_terminated,
        } => {
            if *null_terminated {
                write!(f, "c\"{contents}\"")
            } else {
                write!(f, "\"{contents}\"")
            }
        }
    }
}

fn fmt_rvalue(
    f: &mut fmt::Formatter<'_>,
    db: &dyn Db,
    rvalue: &MIRRValue,
) -> fmt::Result {
    fmt_rvalue_kind(f, db, &rvalue.kind)
}

fn fmt_rvalue_kind(
    f: &mut fmt::Formatter<'_>,
    db: &dyn Db,
    kind: &MIRRValueKind,
) -> fmt::Result {
    match kind {
        MIRRValueKind::Use(operand) => fmt_operand(f, db, operand),
        MIRRValueKind::Ref(place, mutability) => {
            if mutability.is_mut() {
                write!(f, "&mut ")?;
            } else {
                write!(f, "&")?;
            }
            fmt_place(f, db, place)
        }
        MIRRValueKind::AddressOf(place, mutability) => {
            if mutability.is_mut() {
                write!(f, "addr_of_mut ")?;
            } else {
                write!(f, "addr_of ")?;
            }
            fmt_place(f, db, place)
        }
        MIRRValueKind::BinOp(op, lhs, rhs) => {
            fmt_operand(f, db, lhs)?;
            write!(f, " {} ", fmt_binop(op))?;
            fmt_operand(f, db, rhs)
        }
        MIRRValueKind::UnaryOp(op, operand) => {
            match op {
                UnaryOperator::Neg => write!(f, "-")?,
                UnaryOperator::LNot => write!(f, "!")?,
            }
            fmt_operand(f, db, operand)
        }
        MIRRValueKind::Discriminant(place) => {
            write!(f, "discriminant(")?;
            fmt_place(f, db, place)?;
            write!(f, ")")
        }
        MIRRValueKind::Metadata(operand) => {
            write!(f, "@metadata(")?;
            fmt_operand(f, db, operand)?;
            write!(f, ")")
        }
        MIRRValueKind::SizeOf(ty) => {
            write!(f, "@sizeof({})", ty.to_string(db))
        }
    }
}

fn fmt_constructor_args(
    f: &mut fmt::Formatter<'_>,
    db: &dyn Db,
    args: &MIRConstructorArgs,
) -> fmt::Result {
    match args {
        MIRConstructorArgs::None => Ok(()),
        MIRConstructorArgs::Tuple(operands) => {
            for (i, operand) in operands.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                fmt_operand(f, db, operand)?;
            }
            Ok(())
        }
        MIRConstructorArgs::Struct(fields) => {
            let mut first = true;
            for (name, operand) in fields {
                if !first {
                    write!(f, ", ")?;
                }
                first = false;
                write!(f, "{}: ", name.to_string(db))?;
                fmt_operand(f, db, operand)?;
            }
            Ok(())
        }
    }
}

fn fmt_terminator(
    f: &mut fmt::Formatter<'_>,
    db: &dyn Db,
    terminator: &MIRTerminator,
) -> fmt::Result {
    match terminator {
        MIRTerminator::Diverge => write!(f, "diverge"),
        MIRTerminator::Goto { next, .. } => {
            write!(f, "goto bb{}", next.into_raw())
        }
        MIRTerminator::Return { value, .. } => match value {
            Some(operand) => {
                write!(f, "return ")?;
                fmt_operand(f, db, operand)
            }
            None => write!(f, "return"),
        },
        MIRTerminator::Branch {
            cond, then, else_, ..
        } => {
            write!(f, "branch ")?;
            fmt_operand(f, db, cond)?;
            write!(f, " ? bb{} : bb{}", then.into_raw(), else_.into_raw())
        }
        MIRTerminator::Switch {
            discriminant,
            branches,
            default,
            ..
        } => {
            write!(f, "switch ")?;
            fmt_operand(f, db, discriminant)?;
            write!(f, " {{")?;
            for (value, block) in branches {
                write!(f, " {value} => bb{},", block.into_raw())?;
            }
            write!(f, " _ => bb{} }}", default.into_raw())
        }
        MIRTerminator::Call {
            callee,
            arguments,
            dest,
            next,
            ..
        } => {
            fmt_local_id(f, *dest)?;
            write!(f, " = call ")?;
            fmt_callee(f, db, callee)?;
            write!(f, "(")?;
            for (i, arg) in arguments.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                fmt_operand(f, db, arg)?;
            }
            write!(f, ") -> bb{}", next.into_raw())
        }
    }
}

fn fmt_callee(
    f: &mut fmt::Formatter<'_>,
    db: &dyn Db,
    callee: &MIRCallee,
) -> fmt::Result {
    match callee {
        MIRCallee::Direct(fref) => {
            write!(f, "{}", fref.id.called_to_string(db))?;
            if !fref.args.is_empty() {
                write!(f, "::<")?;
                for (i, arg) in fref.args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg.to_string(db))?;
                }
                write!(f, ">")?;
            }
            Ok(())
        }
    }
}
