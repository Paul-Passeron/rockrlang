use std::fmt::{self, Formatter, FormattingOptions};

use crate::{
    Db,
    common::symbols::{StrLit, Symbol},
    hir::{
        HirBody, HirConstructorArgs, HirExpr, HirExprDesc, HirPattern, HirPatternConstructorArgs,
        HirPatternDesc, HirPlace, HirStmt, HirStmtKind, HirStructFieldPattern, Mutability,
        PartialTypeArg, PartialTypeRef,
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

impl<'db> HirBody<'db> {
    pub fn display(&'db self, db: &'db dyn Db) -> Display<'db, &'db Self> {
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

impl<'a> fmt::Display for Display<'a, &'a HirBody<'a>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "fn {} {{", self.value.owner(self.db).display(self.db))?;
        writeln!(
            f,
            "  params: [{}]",
            self.value
                .params(self.db)
                .iter()
                .map(|p| format!("_{}", p.0))
                .collect::<Vec<_>>()
                .join(", ")
        )?;
        writeln!(f, "  locals:")?;

        let mut sorted_locals = self.value.locals(self.db).clone();
        sorted_locals.sort_by_key(|x| x.id);

        for local in &sorted_locals {
            let mutstr = match local.mutability {
                Mutability::Mutable => "mut ",
                Mutability::Const => "",
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
        for stmt in self.value.stmts(self.db) {
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
            write_pattern(f, pattern, db, depth)?;
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
        HirStmtKind::Match {
            scrutinee,
            branches,
        } => {
            write!(f, "match ")?;
            write_expr(f, scrutinee, db)?;
            writeln!(f, " {{")?;
            for branch in branches {
                write_indent(f, depth + 1)?;
                write_pattern(f, &branch.pattern, db, depth)?;
                if let Some(guard) = &branch.guard {
                    write!(f, " if ")?;
                    write_expr(f, guard, db)?;
                }
                writeln!(f, " =>")?;
                write_stmt(f, &branch.body, db, depth + 2)?;
            }
            write_indent(f, depth)?;
            writeln!(f, "}}")
        }
        HirStmtKind::Defer(hir_stmt) => {
            let mut s = String::new();
            write_stmt(
                &mut Formatter::new(&mut s, FormattingOptions::default()),
                hir_stmt,
                db,
                depth,
            )?;
            write!(f, "defer {}", s.trim_start())
        }
        HirStmtKind::Break => {
            writeln!(f, "break;")
        }
    }
}

fn write_pattern(
    f: &mut fmt::Formatter<'_>,
    pat: &HirPattern,
    db: &dyn Db,
    depth: usize,
) -> fmt::Result {
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
                write_pattern(f, p, db, depth)?;
            }
            write!(f, ")")
        }
        HirPatternDesc::Constructor {
            resolution,
            name,
            fields,
        } => {
            write!(f, "{}::{}", resolution.display(db), name.display(db))?;
            match fields {
                HirPatternConstructorArgs::None => Ok(()),
                HirPatternConstructorArgs::StructFields(hir_struct_field_patterns) => {
                    writeln!(f, "{{")?;
                    for p in hir_struct_field_patterns.iter() {
                        write_indent(f, depth + 2)?;
                        match p {
                            HirStructFieldPattern::Rebind { name, pattern } => {
                                write!(f, "{}: ", name.display(db))?;
                                write_pattern(f, pattern, db, depth + 2)?;
                            }
                            HirStructFieldPattern::Name { id, name } => {
                                write!(f, "{}/*_{}*/", name.display(db), id.0)?;
                            }
                        }
                        writeln!(f, ", ")?;
                    }
                    write_indent(f, depth + 1)?;
                    write!(f, "}}")
                }
                HirPatternConstructorArgs::TupleFields(hir_patterns) => {
                    write!(f, "(")?;
                    for (i, p) in hir_patterns.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write_pattern(f, p, db, depth)?;
                    }
                    write!(f, ")")
                }
            }
        }
        HirPatternDesc::DestructureBinding { .. } => todo!(),
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
        HirExprDesc::CStrLit(s) => write!(f, "c\"{}\"", s.display(db)),
        HirExprDesc::BoolLit(b) => write!(f, "{}", b),

        HirExprDesc::Use(place) => write_place(f, place, db),
        HirExprDesc::AddressOf { place, mutability } => {
            match mutability {
                Mutability::Mutable => write!(f, "&mut ")?,
                Mutability::Const => write!(f, "&")?,
            }
            write_place(f, place, db)
        }
        HirExprDesc::Ref { place, mutability } => {
            match mutability {
                Mutability::Mutable => write!(f, "ref mut ")?,
                Mutability::Const => write!(f, "ref ")?,
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
        HirExprDesc::Constructor {
            name,
            enum_def,
            args,
            template_hints,
        } => {
            write!(f, "{}", enum_def.name(db).display(db))?;
            if !template_hints.is_empty() {
                write!(f, "<")?;
                for (i, hint) in template_hints.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    match hint {
                        PartialTypeArg::Known(tref) => write!(f, "{}", tref.display(db))?,
                        PartialTypeArg::Partial(p) => write_partial_type(f, p, db)?,
                        PartialTypeArg::Infer => write!(f, "_")?,
                    }
                }
                write!(f, ">")?;
            }
            write!(f, "::{}", name.display(db))?;
            match args {
                HirConstructorArgs::TupleLike(hir_exprs) => {
                    write!(f, "(")?;
                    for (i, expr) in hir_exprs.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write_expr(f, expr, db)?;
                    }
                    write!(f, ")")
                }
                HirConstructorArgs::StructLike { fields } => {
                    write!(f, "{{")?;
                    for (i, (name, expr)) in fields.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, ".{}: ", name.display(db))?;
                        write_expr(f, expr, db)?;
                    }
                    write!(f, "}}")
                }
                HirConstructorArgs::None => Ok(()),
            }
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
