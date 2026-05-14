use std::fmt;

use crate::{
    Db,
    common::symbols::{StrLit, Symbol},
    hir::{
        HirBody, HirExpr, HirExprDesc, HirPattern, HirPatternDesc, HirPlace, HirStmt, HirStmtKind,
        Mutability, PartialTypeArg, PartialTypeRef,
    },
    parse_tree::expr::BinaryOperator,
    ril::display::{Display, RilDisplay},
};

impl Symbol {
    pub fn display(&self, db: &dyn Db) -> String {
        self.interned().contents(db)
    }
}

impl StrLit {
    pub fn display(&self, db: &dyn Db) -> String {
        self.interned().contents(db)
        // format!("\"{}\"", self.interned().contents(db).escape_debug())
    }
}

impl HirBody {
    pub fn display<'db>(&'db self, db: &'db dyn Db) -> Display<'db, &'db Self> {
        Display { value: self, db }
    }
}

impl fmt::Display for BinaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinaryOperator::Plus => write!(f, "+"),
            BinaryOperator::Minus => write!(f, "-"),
            BinaryOperator::Times => write!(f, "*"),
            BinaryOperator::Div => write!(f, "/"),
            BinaryOperator::Modulo => write!(f, "%"),
            BinaryOperator::Eq => write!(f, "=="),
            BinaryOperator::Diff => write!(f, "!="),
            BinaryOperator::Lt => write!(f, "<"),
            BinaryOperator::Leq => write!(f, "<="),
            BinaryOperator::Gt => write!(f, ">"),
            BinaryOperator::Geq => write!(f, ">="),
            BinaryOperator::And => write!(f, "&&"),
            BinaryOperator::Or => write!(f, "||"),
            BinaryOperator::BitAnd => write!(f, "&"),
            BinaryOperator::BitOr => write!(f, "|"),
            BinaryOperator::BitXor => write!(f, "^"),
        }
    }
}

impl<'a> fmt::Display for Display<'a, &'a HirBody> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "fn {} {{", self.value.owner.display(self.db))?;
        writeln!(
            f,
            "  params: [{}]",
            self.value
                .params
                .iter()
                .map(|p| format!("_{}", p.0))
                .collect::<Vec<_>>()
                .join(", ")
        )?;
        writeln!(f, "  locals:")?;

        let mut sorted_locals = self.value.locals.clone();
        sorted_locals.sort_by_key(|x| x.id);

        for local in &sorted_locals {
            let mutstr = match local.mutability {
                Mutability::Mutable => "mut ",
                Mutability::Immutable => "",
            };
            writeln!(
                f,
                "    _{}: {}{}",
                local.id.0,
                mutstr,
                local.name.display(self.db)
            )?;
        }
        writeln!(f, "  body:")?;
        for stmt in &self.value.stmts {
            write_stmt(f, stmt, self.db, 2)?;
        }
        writeln!(f, "}}")
    }
}

fn write_indent(f: &mut fmt::Formatter<'_>, depth: usize) -> fmt::Result {
    for _ in 0..depth {
        write!(f, "    ")?;
    }
    Ok(())
}

fn write_stmt(
    f: &mut fmt::Formatter<'_>,
    stmt: &HirStmt,
    db: &dyn Db,
    depth: usize,
) -> fmt::Result {
    write_indent(f, depth)?;
    match &stmt.kind {
        HirStmtKind::Let {
            pattern,
            locals: _,
            ty_annotation,
            init,
        } => {
            write!(f, "let ")?;
            write_pattern(f, pattern, db)?;
            if ty_annotation.is_some() {
                write!(f, ": <type>")?;
            }
            write!(f, " = ")?;
            write_expr(f, init, db)?;
            writeln!(f, ";")
        }
        HirStmtKind::Assign { lhs, rhs } => {
            write_place(f, lhs, db)?;
            write!(f, " = ")?;
            write_expr(f, rhs, db)?;
            writeln!(f, ";")
        }
        HirStmtKind::Expr(expr) => {
            write_expr(f, expr, db)?;
            writeln!(f, ";")
        }
        HirStmtKind::Return(None) => {
            writeln!(f, "return;")
        }
        HirStmtKind::Return(Some(expr)) => {
            write!(f, "return ")?;
            write_expr(f, expr, db)?;
            writeln!(f, ";")
        }
        HirStmtKind::If { cond, then, else_ } => {
            write!(f, "if ")?;
            write_expr(f, cond, db)?;
            writeln!(f, " {{")?;
            write_stmt(f, then, db, depth + 1)?;
            write_indent(f, depth)?;
            if let Some(else_branch) = else_ {
                writeln!(f, "}} else {{")?;
                write_stmt(f, else_branch, db, depth + 1)?;
                write_indent(f, depth)?;
            }
            writeln!(f, "}}")
        }
        HirStmtKind::While { cond, body } => {
            write!(f, "while ")?;
            write_expr(f, cond, db)?;
            writeln!(f, " {{")?;
            write_stmt(f, body, db, depth + 1)?;
            write_indent(f, depth)?;
            writeln!(f, "}}")
        }
        HirStmtKind::Block(stmts) => {
            writeln!(f, "{{")?;
            for s in stmts {
                write_stmt(f, s, db, depth + 1)?;
            }
            write_indent(f, depth)?;
            writeln!(f, "}}")
        }
    }
}

fn write_pattern(f: &mut fmt::Formatter<'_>, pat: &HirPattern, db: &dyn Db) -> fmt::Result {
    match &pat.data {
        HirPatternDesc::Bind { id, name, mutable } => {
            if *mutable {
                write!(f, "mut ")?;
            }
            write!(f, "{}/*_{}*/", name.display(db), id.0)
        }
        HirPatternDesc::Any => write!(f, "_"),
        HirPatternDesc::Tuple(pats) => {
            write!(f, "(")?;
            for (i, p) in pats.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write_pattern(f, p, db)?;
            }
            write!(f, ")")
        }
        HirPatternDesc::Constructor { resolution, fields } => {
            if let Some(def) = resolution {
                write!(f, "{}", def.display(db))?;
            } else {
                write!(f, "<?>")?;
            }
            write!(f, "(")?;
            for (i, p) in fields.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write_pattern(f, p, db)?;
            }
            write!(f, ")")
        }
    }
}

fn write_place(f: &mut fmt::Formatter<'_>, place: &HirPlace, db: &dyn Db) -> fmt::Result {
    match place {
        HirPlace::Local(id) => write!(f, "_{}", id.0),
        HirPlace::Field { base, field } => {
            write_place(f, base, db)?;
            write!(f, ".{}", field.display(db))
        }
        HirPlace::TupleField { base, index } => {
            write_place(f, base, db)?;
            write!(f, ".{}", index)
        }
        HirPlace::Deref(base) => {
            write!(f, "*")?;
            write_place(f, base, db)
        }
        HirPlace::Index { base, index } => {
            write_place(f, base, db)?;
            write!(f, "[")?;
            write_expr(f, index, db)?;
            write!(f, "]")
        }
        HirPlace::Temporary(expr) => {
            write!(f, "<tmp:")?;
            write_expr(f, expr, db)?;
            write!(f, ">")
        }
    }
}

fn write_expr(f: &mut fmt::Formatter<'_>, expr: &HirExpr, db: &dyn Db) -> fmt::Result {
    match &expr.data {
        HirExprDesc::IntLit(n) => write!(f, "{}", n),
        HirExprDesc::CharLit(c) => write!(f, "'{}'", c),
        HirExprDesc::StrLit(s) => write!(f, "\"{}\"", s.display(db)),
        HirExprDesc::BoolLit(b) => write!(f, "{}", b),

        HirExprDesc::Use(place) => write_place(f, place, db),
        HirExprDesc::AddressOf { place, mutability } => {
            match mutability {
                Mutability::Mutable => write!(f, "&mut ")?,
                Mutability::Immutable => write!(f, "&")?,
            }
            write_place(f, place, db)
        }
        HirExprDesc::Ref { place, mutability } => {
            match mutability {
                Mutability::Mutable => write!(f, "ref mut ")?,
                Mutability::Immutable => write!(f, "ref ")?,
            }
            write_place(f, place, db)
        }

        HirExprDesc::CallDirect { target, args } => {
            write!(f, "{}(", target.display(db))?;
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write_expr(f, arg, db)?;
            }
            write!(f, ")")
        }
        HirExprDesc::CallMethod {
            receiver,
            method,
            args,
            interface_hint,
        } => match interface_hint {
            Some(id) => {
                write!(f, "{}::{}(", id.display(db), method.display(db))?;
                write_expr(f, receiver, db)?;
                for arg in args {
                    write!(f, ", ")?;
                    write_expr(f, arg, db)?;
                }
                write!(f, ")")
            }
            None => {
                write_expr(f, receiver, db)?;
                write!(f, ".{}(", method.display(db))?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write_expr(f, arg, db)?;
                }
                write!(f, ")")
            }
        },
        HirExprDesc::CallStatic { ty, method, args } => {
            write_partial_type(f, ty, db)?;
            write!(f, "::{}(", method.display(db))?;
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write_expr(f, arg, db)?;
            }
            write!(f, ")")
        }

        HirExprDesc::BinOp { lhs, op, rhs } => {
            write!(f, "(")?;
            write_expr(f, lhs, db)?;
            write!(f, " {} ", op)?; // assumes BinaryOperator: Display
            write_expr(f, rhs, db)?;
            write!(f, ")")
        }

        HirExprDesc::StructLit { ty, fields } => {
            write_partial_type(f, ty, db)?;
            write!(f, " {{ ")?;
            for (i, (name, val)) in fields.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}: ", name.display(db))?;
                write_expr(f, val, db)?;
            }
            write!(f, " }}")
        }

        HirExprDesc::Neg(inner) => {
            write!(f, "-")?;
            write_expr(f, inner, db)
        }
        HirExprDesc::Not(inner) => {
            write!(f, "!")?;
            write_expr(f, inner, db)
        }
        HirExprDesc::Tuple(elems) => {
            write!(f, "(")?;
            for (i, e) in elems.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write_expr(f, e, db)?;
            }
            write!(f, ")")
        }
        HirExprDesc::SliceLit(elems) => {
            write!(f, "[")?;
            for (i, e) in elems.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write_expr(f, e, db)?;
            }
            write!(f, "]")
        }
        HirExprDesc::SizeOf(ty) => {
            write!(f, "sizeof(")?;
            write_partial_type(f, ty, db)?;
            write!(f, ")")
        }
        HirExprDesc::Constructor { ty, name } => {
            write_partial_type(f, ty, db)?;
            write!(f, "::{}", name.display(db))
        }
    }
}

fn write_partial_type(f: &mut fmt::Formatter<'_>, ty: &PartialTypeRef, db: &dyn Db) -> fmt::Result {
    match ty {
        PartialTypeRef::Resolved(tref) => write!(f, "{}", tref.display(db)),
        PartialTypeRef::WithHoles { def, args } => {
            write!(f, "{}", def.display(db))?;
            if !args.is_empty() {
                write!(f, "<")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    match arg {
                        PartialTypeArg::Known(tref) => write!(f, "{}", tref.display(db))?,
                        PartialTypeArg::Partial(p) => write_partial_type(f, p, db)?,
                        PartialTypeArg::Infer => write!(f, "_")?,
                    }
                }
                write!(f, ">")?;
            }
            Ok(())
        }
    }
}
