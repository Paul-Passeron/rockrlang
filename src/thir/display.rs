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

use super::stmt::{StmtKind, ThirStmt};
use super::{
    Dispatch, ExprId, ExprKind, LocalId, PlaceBase, PlaceId, Projection, ScopeId, Thir,
    ThirConstructorArgs, ThirExprWithSetup, ThirMatchBranch, ThirPattern,
    ThirPatternKind,
};
use crate::resolved::TypeDefId;
use crate::thir::ThirStructField;
use crate::thir::stmt::BlockSemanticInfo;
use crate::{Db, hir::Mutability, resolved::TypeRef};

const INDENT: &str = "    ";

pub fn display_thir<'a>(db: &'a dyn Db, thir: &'a Thir) -> String {
    let mut printer = ThirPrinter { db, thir, out: String::new(), indent: 0 };
    printer.run();
    printer.out
}

impl Thir {
    pub fn display(&self, db: &dyn Db) -> String {
        display_thir(db, self)
    }
}

struct ThirPrinter<'a> {
    db: &'a dyn Db,
    thir: &'a Thir,
    out: String,
    indent: usize,
}

impl<'a> ThirPrinter<'a> {
    fn run(&mut self) {
        let thir = self.thir;

        let name = thir.id.sig_to_string(self.db);
        self.line(&format!("{}", thir.id.span(self.db).start().loc_info(self.db)));
        self.line(&format!("thir fun {name} {{"));
        self.indent += 1;
        self.print_stmts(&thir.root);
        self.indent -= 1;
        self.line("}");
    }

    fn print_stmts(&mut self, stmts: &'a [ThirStmt]) {
        for stmt in stmts {
            self.print_stmt(stmt);
        }
    }

    fn print_stmt(&mut self, stmt: &'a ThirStmt) {
        match &stmt.kind {
            StmtKind::Block { scope, stmts, semantic_infos } => {
                let lbl = Self::scope_label(*scope);
                match semantic_infos {
                    Some(BlockSemanticInfo::StructDestructure(struct_ref)) => {
                        self.line(&format!(
                            "{lbl} (Destructuring `{}`): {{",
                            struct_ref.clone().into_type_ref(self.db).to_string(self.db)
                        ));
                    }
                    Some(BlockSemanticInfo::ForLoop) => {
                        self.line(&format!("{lbl} (for-loop): {{"));
                    }

                    None => self.line(&format!("{lbl}: {{")),
                }
                self.indent += 1;
                self.print_stmts(stmts);
                self.indent -= 1;
                self.line("}");
            }

            StmtKind::If { cond, then, then_scope, else_, else_scope } => {
                let c = self.emit_cond(cond);
                let lbl = Self::scope_label(*then_scope);
                self.line(&format!("if {c} {lbl}: {{"));
                self.indent += 1;
                self.print_stmts(then);
                self.indent -= 1;
                match else_ {
                    Some(else_body) => {
                        match else_scope {
                            Some(s) => {
                                let elbl = Self::scope_label(*s);
                                self.line(&format!("}} else {elbl}: {{"));
                            }
                            None => self.line("} else {"),
                        }
                        self.indent += 1;
                        self.print_stmts(else_body);
                        self.indent -= 1;
                        self.line("}");
                    }
                    None => self.line("}"),
                }
            }

            StmtKind::While { scope, cond, body } => {
                let c = self.emit_cond(cond);
                let lbl = Self::scope_label(*scope);
                self.line(&format!("{lbl}: while {c} {{"));
                self.indent += 1;
                self.print_stmts(body);
                self.indent -= 1;
                self.line("}");
            }

            StmtKind::Let { local, init } => {
                let m = Self::mut_prefix(self.thir.locals[*local].mutability);
                let ty = self.thir.locals[*local].ty.to_string(self.db);
                let nm = self.local_name(*local);
                let e = self.render_expr(*init);
                self.line(&format!("let {m}{nm}: {ty} = {e};"));
            }

            StmtKind::Assign { place, rhs } => {
                let p = self.render_place(*place);
                let e = self.render_expr(*rhs);
                self.line(&format!("{p} = {e};"));
            }

            StmtKind::Return(opt) => match opt {
                Some(e) => {
                    let s = self.render_expr(*e);
                    self.line(&format!("return {s};"));
                }
                None => self.line("return;"),
            },

            StmtKind::Break(s) => {
                let l = Self::scope_label(*s);
                self.line(&format!("break {l};"));
            }

            StmtKind::Continue(s) => {
                let l = Self::scope_label(*s);
                self.line(&format!("continue {l};"));
            }

            StmtKind::Match { scrutinee, branches } => {
                let s = self.emit_cond(scrutinee);
                self.line(&format!("match {s} {{"));
                self.indent += 1;
                for branch in branches {
                    self.print_branch(branch);
                }
                self.indent -= 1;
                self.line("}");
            }

            StmtKind::Expr(e) => {
                let s = self.render_expr(*e);
                self.line(&format!("{s};"));
            }

            StmtKind::Error => self.line("<error stmt>;"),
            StmtKind::Drop(local) => {
                self.line(&format!("drop {};", self.local_name(*local)))
            }
        }
    }

    fn print_branch(&mut self, branch: &'a ThirMatchBranch) {
        let pat = self.render_pattern(&branch.pattern);
        let guard_inline = branch.guard.as_ref().map(|g| self.render_expr(g.expr));
        let lbl = Self::scope_label(branch.body_scope);
        let header = match guard_inline {
            Some(g) => format!("{pat} if {g} => {lbl}: {{"),
            None => format!("{pat} => {lbl}: {{"),
        };
        self.line(&header);
        self.indent += 1;
        if let Some(g) = &branch.guard
            && !g.stmts.is_empty()
        {
            self.line("// guard setup:");
            self.print_stmts(&g.stmts);
        }
        self.print_stmts(&branch.body);
        self.indent -= 1;
        self.line("}");
    }

    fn emit_cond(&mut self, ews: &'a ThirExprWithSetup) -> String {
        if !ews.stmts.is_empty() {
            self.line("// setup:");
            self.print_stmts(&ews.stmts);
        }
        self.render_expr(ews.expr)
    }

    fn render_expr(&self, id: ExprId) -> String {
        let expr = &self.thir.exprs[id];
        match &expr.kind {
            ExprKind::IntLit(n) => n.to_string(),
            ExprKind::Charlit(c) => format!("{c:?}"),
            ExprKind::StrLit(s) => {
                format!("\"{}\"", s.display(self.db))
            }
            ExprKind::CStrLit(s) => {
                format!("c\"{}\\0\"", s.display(self.db))
            }
            ExprKind::BoolLit(b) => b.to_string(),

            ExprKind::Use(p) => self.render_place(*p),

            ExprKind::AddressOf { place, mutability } => {
                let m = Self::mut_prefix(*mutability);
                format!("&raw {m}{}", self.render_place(*place))
            }
            ExprKind::Ref { place, mutability } => {
                let m = Self::mut_prefix(*mutability);
                format!("&{m}{}", self.render_place(*place))
            }

            ExprKind::Call { called, args } => {
                let mut head = called.id.called_to_string(self.db);
                if let Some(self_ty) = &called.self_ty {
                    head = format!("<{}>::{head}", self_ty.to_string(self.db));
                }
                head.push_str(&self.targs(&called.args));
                let arg_s = self.render_exprs(args);
                let disp = match &called.dispatch {
                    Dispatch::Direct => String::new(),
                    Dispatch::Interface(i) => {
                        format!(" /* via {} */", i.to_string(self.db))
                    }
                };
                format!("{head}({arg_s}){disp}")
            }

            ExprKind::BinOp { op, lhs, rhs } => {
                let l = self.render_expr(*lhs);
                let r = self.render_expr(*rhs);
                format!("({l} {op} {r})")
            }

            ExprKind::StructLit { struct_def, fields } => {
                let name = TypeDefId::Struct(struct_def.def).to_string(self.db);
                let targs = self.targs(&struct_def.args);
                if fields.is_empty() {
                    format!("{name}{targs} {{}}")
                } else {
                    let fs = fields
                        .iter()
                        .map(|field| {
                            format!(
                                "{}: {}",
                                field.field.to_string(self.db),
                                self.render_expr(field.expr)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{name}{targs} {{ {fs} }}")
                }
            }

            ExprKind::Neg(e) => format!("-{}", self.render_expr(*e)),
            ExprKind::Not(e) => format!("!{}", self.render_expr(*e)),

            ExprKind::Tuple(es) => {
                let s = self.render_exprs(es);
                if es.len() == 1 { format!("({s},)") } else { format!("({s})") }
            }
            ExprKind::SliceLit(es) => format!("[{}]", self.render_exprs(es)),
            ExprKind::SizeOf(ty) => {
                format!("sizeof({})", ty.to_string(self.db))
            }
            ExprKind::TypeName(ty) => {
                format!("typename({})", ty.to_string(self.db))
            }

            ExprKind::Constructor { enum_def, idx, args } => {
                let name = TypeDefId::Enum(enum_def.def).to_string(self.db);
                let targs = self.targs(&enum_def.args);
                let body = self.ctor_args_expr(args);
                format!("{name}{targs}::#{idx}{body}")
            }

            ExprKind::Error => "<error>".to_owned(),
            ExprKind::Metadata(id) => {
                format!("@metadata({})", self.render_expr(*id))
            }
            ExprKind::Cast(idx, type_ref) => format!(
                "cast {} to {}",
                self.render_expr(*idx),
                type_ref.to_string(self.db),
            ),
        }
    }

    fn render_exprs(&self, ids: &[ExprId]) -> String {
        ids.iter().map(|e| self.render_expr(*e)).collect::<Vec<_>>().join(", ")
    }

    fn ctor_args_expr(&self, args: &ThirConstructorArgs<ExprId>) -> String {
        match args {
            ThirConstructorArgs::Tuple(items) => {
                format!("({})", self.render_exprs(items))
            }
            ThirConstructorArgs::Struct(items) => {
                let s = items
                    .iter()
                    .map(|ThirStructField { field, expr, .. }| {
                        format!(
                            "{}: {}",
                            field.to_string(self.db),
                            self.render_expr(*expr)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(" {{ {s} }}")
            }
            ThirConstructorArgs::None => String::new(),
        }
    }

    fn render_place(&self, id: PlaceId) -> String {
        let place = &self.thir.places[id];
        let mut s = match &place.base {
            PlaceBase::Local(l) => self.local_name(*l),
        };
        for proj in &place.projections {
            match proj {
                Projection::Deref => s = format!("(*{s})"),
                Projection::Field(sym, _) => {
                    s = format!("{s}.{}", sym.to_string(self.db));
                }
                Projection::TupleField(n, _) => s = format!("{s}.{n}"),
                Projection::Index(e) => s = format!("{s}[{}]", self.render_expr(*e)),
            }
        }
        s
    }

    fn render_pattern(&self, pat: &ThirPattern) -> String {
        match &pat.kind {
            ThirPatternKind::Any => "_".to_owned(),
            ThirPatternKind::Bind { local, mutable } => {
                let m = if *mutable { "mut " } else { "" };
                format!("{m}{}", self.local_name(*local))
            }
            ThirPatternKind::Tuple(ps) => {
                let s = ps
                    .iter()
                    .map(|p| self.render_pattern(p))
                    .collect::<Vec<_>>()
                    .join(", ");
                if ps.len() == 1 { format!("({s},)") } else { format!("({s})") }
            }
            ThirPatternKind::Struct { def, fields } => {
                let name = TypeDefId::Struct(def.def).to_string(self.db);
                let targs = self.targs(&def.args);
                if fields.is_empty() {
                    format!("{name}{targs} {{}}")
                } else {
                    let fs = fields
                        .iter()
                        .map(|(s, p)| {
                            format!(
                                "{}: {}",
                                s.to_string(self.db),
                                self.render_pattern(p)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{name}{targs} {{ {fs} }}")
                }
            }
            ThirPatternKind::Constructor { def, idx, args } => {
                let name = TypeDefId::Enum(def.def).to_string(self.db);
                let targs = self.targs(&def.args);
                let body = self.ctor_args_pat(args);
                format!("{name}{targs}::#{idx}{body}")
            }
            ThirPatternKind::IntLit(n) => n.to_string(),
            ThirPatternKind::Error => "<error pat>".to_owned(),
        }
    }

    fn ctor_args_pat(&self, args: &ThirConstructorArgs<ThirPattern>) -> String {
        match args {
            ThirConstructorArgs::Tuple(items) => {
                let s = items
                    .iter()
                    .map(|p| self.render_pattern(p))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({s})")
            }
            ThirConstructorArgs::Struct(items) => {
                let s = items
                    .iter()
                    .map(|ThirStructField { field, expr, .. }| {
                        format!(
                            "{}: {}",
                            field.to_string(self.db),
                            self.render_pattern(expr)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(" {{ {s} }}")
            }
            ThirConstructorArgs::None => String::new(),
        }
    }

    fn local_name(&self, local: LocalId) -> String {
        let l = &self.thir.locals[local];
        match &l.source {
            Some((_, sym)) => sym.to_string(self.db),
            None => format!("_{}", local.into_raw()),
        }
    }

    fn scope_label(s: ScopeId) -> String {
        format!("'s{}", s.into_raw())
    }

    fn targs(&self, args: &[TypeRef]) -> String {
        if args.is_empty() {
            String::new()
        } else {
            let s =
                args.iter().map(|t| t.to_string(self.db)).collect::<Vec<_>>().join(", ");
            format!("::<{s}>")
        }
    }

    fn mut_prefix(m: Mutability) -> &'static str {
        match m {
            Mutability::Const => "",
            Mutability::Mutable => "mut ",
        }
    }

    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str(INDENT);
        }
        self.out.push_str(s);
        self.out.push('\n');
    }
}
