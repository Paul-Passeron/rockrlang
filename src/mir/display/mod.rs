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

use crate::mir::{
    MIRLocalID,
    basic_block::{MIRBasicBlock, MIRTerminator, Stmt},
    operand::{
        MIRCallee, MIRConstant, MIRConstructorArgs, MIROperand, MIRPlace, MIRProjection,
        MIRRValue, MIRRValueKind,
    },
};
use crate::{Db, mir::operand::UnaryOperator, parse_tree::expr::BinaryOperator};
use std::fmt;

pub mod graphviz;
pub mod writer;

/// Minimal write trait that both `fmt::Formatter` and `String` satisfy,
/// letting all formatting logic be written once.
pub trait MIRWrite {
    fn write_str(&mut self, s: &str) -> fmt::Result;

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        // Default: format to a temporary String, then write it.
        self.write_str(&args.to_string())
    }
}

pub struct FmtWriter<'a, 'b>(pub &'a mut fmt::Formatter<'b>);

impl<'a, 'b> MIRWrite for FmtWriter<'a, 'b> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.write_str(s)
    }
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        self.0.write_fmt(args)
    }
}

pub struct StringWriter(pub String);

impl MIRWrite for StringWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.push_str(s);
        Ok(())
    }
}

macro_rules! mwrite {
    ($dst:expr, $($arg:tt)*) => {
        $dst.write_fmt(format_args!($($arg)*))
    };
}

pub(crate) use mwrite;

pub(crate) fn fmt_binop(op: &BinaryOperator) -> &'static str {
    match op {
        BinaryOperator::Plus => "+",
        BinaryOperator::Minus => "-",
        BinaryOperator::Times => "*",
        BinaryOperator::Div => "/",
        BinaryOperator::Modulo => "%",
        BinaryOperator::Eq => "==",
        BinaryOperator::Diff => "!=",
        BinaryOperator::Lt => "<",
        BinaryOperator::Leq => "<=",
        BinaryOperator::Gt => ">",
        BinaryOperator::Geq => ">=",
        BinaryOperator::And => "&&",
        BinaryOperator::Or => "||",
        BinaryOperator::BitAnd => "&",
        BinaryOperator::BitOr => "|",
        BinaryOperator::BitXor => "^",
    }
}

pub fn fmt_local_id<W: MIRWrite>(w: &mut W, id: MIRLocalID) -> fmt::Result {
    mwrite!(w, "_{}", id.into_raw())
}

pub fn fmt_place<W: MIRWrite>(w: &mut W, db: &dyn Db, place: &MIRPlace) -> fmt::Result {
    fmt_local_id(w, place.local)?;
    for proj in &place.projections {
        fmt_projection(w, db, proj)?;
    }
    Ok(())
}

pub fn fmt_projection<W: MIRWrite>(
    w: &mut W,
    db: &dyn Db,
    proj: &MIRProjection,
) -> fmt::Result {
    match proj {
        MIRProjection::Deref => w.write_str(".*"),
        MIRProjection::Field { name, .. } => {
            mwrite!(w, ".{}", name.to_string(db))
        }
        MIRProjection::TupleField { index, .. } => mwrite!(w, ".{index}"),
        MIRProjection::Index { index } => {
            w.write_str("[")?;
            fmt_operand(w, db, index)?;
            w.write_str("]")
        }
        MIRProjection::Downcast { variant } => {
            mwrite!(w, ".downcast#{variant}")
        }
    }
}

pub fn fmt_operand<W: MIRWrite>(
    w: &mut W,
    db: &dyn Db,
    operand: &MIROperand,
) -> fmt::Result {
    match operand {
        MIROperand::Constant(c, _) => fmt_constant(w, db, c),
        MIROperand::Move(place) => {
            w.write_str("move ")?;
            fmt_place(w, db, place)
        }
        MIROperand::Copy(place) => {
            w.write_str("copy ")?;
            fmt_place(w, db, place)
        }
    }
}

pub fn fmt_constant<W: MIRWrite>(
    w: &mut W,
    db: &dyn Db,
    constant: &MIRConstant,
) -> fmt::Result {
    match constant {
        MIRConstant::Integer { value, ty } => {
            mwrite!(w, "{value}_{}", ty.to_string(db))
        }
        MIRConstant::Bool(b) => mwrite!(w, "{b}"),
        MIRConstant::CString { contents, null_terminated } => {
            if *null_terminated {
                mwrite!(w, "c\"{contents}\"")
            } else {
                mwrite!(w, "\"{contents}\"")
            }
        }
    }
}

pub fn fmt_rvalue<W: MIRWrite>(
    w: &mut W,
    db: &dyn Db,
    rvalue: &MIRRValue,
) -> fmt::Result {
    match &rvalue.kind {
        MIRRValueKind::Use(operand) => fmt_operand(w, db, operand),
        MIRRValueKind::Ref(place, mutability) => {
            w.write_str(if mutability.is_mut() { "&mut " } else { "&" })?;
            fmt_place(w, db, place)
        }
        MIRRValueKind::AddressOf(place, mutability) => {
            w.write_str(if mutability.is_mut() { "addr_of_mut " } else { "addr_of " })?;
            fmt_place(w, db, place)
        }
        MIRRValueKind::BinOp(op, lhs, rhs) => {
            fmt_operand(w, db, lhs)?;
            mwrite!(w, " {} ", fmt_binop(op))?;
            fmt_operand(w, db, rhs)
        }
        MIRRValueKind::UnaryOp(op, operand) => {
            w.write_str(match op {
                UnaryOperator::Neg => "-",
                UnaryOperator::LNot => "!",
            })?;
            fmt_operand(w, db, operand)
        }
        MIRRValueKind::Discriminant(place) => {
            w.write_str("discriminant(")?;
            fmt_place(w, db, place)?;
            w.write_str(")")
        }
        MIRRValueKind::Metadata(operand) => {
            w.write_str("@metadata(")?;
            fmt_operand(w, db, operand)?;
            w.write_str(")")
        }
        MIRRValueKind::SizeOf(ty) => {
            mwrite!(w, "@sizeof({})", ty.to_string(db))
        }
        MIRRValueKind::Constructor { enum_ref, idx, args, .. } => {
            mwrite!(w, "{}::#{idx}(", enum_ref.def.name(db).to_string(db))?;
            fmt_constructor_args(w, db, args)?;
            w.write_str(")")
        }
        MIRRValueKind::StructLit { struct_ref, fields, .. } => {
            mwrite!(w, "{} {{", struct_ref.def.name(db).to_string(db))?;
            let mut first = true;
            for (name, op) in fields {
                if !first {
                    w.write_str(", ")?;
                }
                first = false;
                mwrite!(w, "{}: ", name.to_string(db))?;
                fmt_operand(w, db, op)?;
            }
            w.write_str("}")
        }
        MIRRValueKind::Tuple(operands, _) => {
            w.write_str("(")?;
            for (i, op) in operands.iter().enumerate() {
                if i > 0 {
                    w.write_str(", ")?;
                }
                fmt_operand(w, db, op)?;
            }
            w.write_str(")")
        }
        MIRRValueKind::Cast(operand, type_ref) => {
            w.write_str("cast ")?;
            fmt_operand(w, db, operand)?;
            w.write_str(" as ")?;
            w.write_str(type_ref.to_string(db).as_str())
        }
    }
}

pub fn fmt_constructor_args<W: MIRWrite>(
    w: &mut W,
    db: &dyn Db,
    args: &MIRConstructorArgs,
) -> fmt::Result {
    match args {
        MIRConstructorArgs::None => Ok(()),
        MIRConstructorArgs::Tuple(operands) => {
            for (i, op) in operands.iter().enumerate() {
                if i > 0 {
                    w.write_str(", ")?;
                }
                fmt_operand(w, db, op)?;
            }
            Ok(())
        }
        MIRConstructorArgs::Struct(fields) => {
            let mut first = true;
            for (name, op) in fields {
                if !first {
                    w.write_str(", ")?;
                }
                first = false;
                mwrite!(w, "{}: ", name.to_string(db))?;
                fmt_operand(w, db, op)?;
            }
            Ok(())
        }
    }
}

pub fn fmt_callee<W: MIRWrite>(
    w: &mut W,
    db: &dyn Db,
    callee: &MIRCallee,
) -> fmt::Result {
    match callee {
        MIRCallee::Direct(fref) => {
            mwrite!(w, "{}", fref.id.called_to_string(db))?;
            if !fref.args.is_empty() {
                w.write_str("::<")?;
                for (i, ty) in fref.args.iter().enumerate() {
                    if i > 0 {
                        w.write_str(", ")?;
                    }
                    mwrite!(w, "{}", ty.to_string(db))?;
                }
                w.write_str(">")?;
            }
            Ok(())
        }
    }
}

pub fn fmt_terminator<W: MIRWrite>(
    w: &mut W,
    db: &dyn Db,
    terminator: &MIRTerminator,
) -> fmt::Result {
    match terminator {
        MIRTerminator::Diverge => w.write_str("diverge"),
        MIRTerminator::Goto { next, .. } => {
            mwrite!(w, "goto bb{}", next.into_raw())
        }
        MIRTerminator::Return { value, .. } => match value {
            Some(operand) => {
                w.write_str("return ")?;
                fmt_operand(w, db, operand)
            }
            None => w.write_str("return"),
        },
        MIRTerminator::Branch { cond, then, else_, .. } => {
            w.write_str("branch ")?;
            fmt_operand(w, db, cond)?;
            mwrite!(w, " ? bb{} : bb{}", then.into_raw(), else_.into_raw())
        }
        MIRTerminator::Switch { discriminant, branches, default, .. } => {
            w.write_str("switch ")?;
            fmt_operand(w, db, discriminant)?;
            w.write_str(" {")?;
            for (value, block) in branches {
                mwrite!(w, " {value} => bb{},", block.into_raw())?;
            }
            mwrite!(w, " _ => bb{} }}", default.into_raw())
        }
        MIRTerminator::Call { callee, arguments, dest, next, .. } => {
            fmt_local_id(w, *dest)?;
            w.write_str(" = call ")?;
            fmt_callee(w, db, callee)?;
            w.write_str("(")?;
            for (i, arg) in arguments.iter().enumerate() {
                if i > 0 {
                    w.write_str(", ")?;
                }
                fmt_operand(w, db, arg)?;
            }
            mwrite!(w, ") -> bb{}", next.into_raw())
        }
    }
}

pub fn fmt_stmt<W: MIRWrite>(w: &mut W, db: &dyn Db, stmt: &Stmt) -> fmt::Result {
    match stmt {
        Stmt::Assign { dest, rvalue } => {
            fmt_place(w, db, dest)?;
            w.write_str(" = ")?;
            fmt_rvalue(w, db, rvalue)
        }
    }
}

pub fn fmt_block<W: MIRWrite>(
    w: &mut W,
    db: &dyn Db,
    block: &MIRBasicBlock,
) -> fmt::Result {
    for stmt in &block.stmts {
        w.write_str("    ")?;
        fmt_stmt(w, db, stmt)?;
        w.write_str("\n")?;
    }
    w.write_str("    ")?;
    fmt_terminator(w, db, &block.terminator)?;
    w.write_str("\n")
}
