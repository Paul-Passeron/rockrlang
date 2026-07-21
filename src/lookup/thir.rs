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

use std::ops::Not;

use crate::{
    Db,
    common::{
        location::{Location, Span},
        symbols::Symbol,
    },
    thir::{
        EnumRef, ExprId, ExprKind, LocalId, PlaceBase, PlaceId, StructRef, Thir,
        ThirConstructorArgs, ThirExprWithSetup, ThirMatchBranch, ThirPattern,
        ThirPatternKind,
        stmt::{StmtKind, ThirStmt},
    },
};

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum ThirNode<'thir> {
    Expr { id: ExprId, setup: Option<&'thir [ThirStmt]> },
    Place(PlaceId),
    Local(LocalId),
    Stmt(&'thir ThirStmt),
    Pattern(&'thir ThirPattern),
    MatchBranch(&'thir ThirMatchBranch),
    EnumVariant { enum_def: EnumRef, variant: usize, span: Span },
    StructField { struct_def: StructRef, field: Symbol, span: Span },
}

impl Thir {
    fn local_at<'a>(&'a self, local: LocalId, loc: Location) -> Option<ThirNode<'a>> {
        let thir_loc = &self.locals[local];
        thir_loc.span.encloses(loc).then_some(())?;
        thir_loc.is_synthetic.not().then_some(ThirNode::Local(local))
    }

    fn place_at<'a>(&'a self, place: PlaceId, loc: Location) -> Option<ThirNode<'a>> {
        // TODO: handle projections
        let pl = &self.places[place];
        pl.span.encloses(loc).then_some(())?;
        match pl.base {
            PlaceBase::Local(idx) => self
                .local_at(idx, loc)
                .or(pl.is_synthetic.not().then_some(ThirNode::Place(place))),
        }
    }

    fn expr_at<'a>(
        &'a self,
        db: &dyn Db,
        expr: ExprId,
        loc: Location,
    ) -> Option<ThirNode<'a>> {
        let e = &self.exprs[expr];
        e.span.encloses(loc).then_some(())?;
        let res = match &e.kind {
            ExprKind::Error
            | ExprKind::IntLit(_)
            | ExprKind::Charlit(_)
            | ExprKind::StrLit(_)
            | ExprKind::CStrLit(_)
            | ExprKind::BoolLit(_)
            | ExprKind::SizeOf(_)
            | ExprKind::TypeName(_) => None,
            ExprKind::Use(place)
            | ExprKind::AddressOf { place, .. }
            | ExprKind::Ref { place, .. } => self.place_at(*place, loc),
            ExprKind::Tuple(items)
            | ExprKind::SliceLit(items)
            | ExprKind::Call { args: items, .. } => {
                items.iter().find_map(|e| self.expr_at(db, *e, loc))
            }
            ExprKind::BinOp { lhs, rhs, .. } => {
                self.expr_at(db, *lhs, loc).or_else(|| self.expr_at(db, *rhs, loc))
            }
            ExprKind::StructLit { fields, struct_def } => fields
                .iter()
                .find_map(|field| {
                    field.field_span.encloses(loc).then(|| ThirNode::StructField {
                        struct_def: struct_def.clone(),
                        field: field.field,
                        span: field.field_span,
                    })
                })
                .or_else(|| {
                    fields.iter().find_map(|field| self.expr_at(db, field.expr, loc))
                }),
            ExprKind::Neg(idx)
            | ExprKind::Not(idx)
            | ExprKind::Metadata(idx)
            | ExprKind::Cast(idx, _) => self.expr_at(db, *idx, loc),
            ExprKind::Constructor { args, .. } => match args {
                ThirConstructorArgs::Tuple(items) => {
                    items.iter().find_map(|e| self.expr_at(db, *e, loc))
                }
                ThirConstructorArgs::Struct(fields) => {
                    fields.iter().find_map(|field| self.expr_at(db, field.expr, loc))
                }
                ThirConstructorArgs::None => None,
            },
        };
        res.or(e.is_synthetic.not().then_some(ThirNode::Expr { id: expr, setup: None }))
    }

    fn expr_with_setup_at<'a>(
        &'a self,
        db: &dyn Db,
        setup: &'a ThirExprWithSetup,
        loc: Location,
    ) -> Option<ThirNode<'a>> {
        if let Some(node) =
            setup.stmts.iter().find_map(|stmt| self.stmt_at(db, stmt, loc))
        {
            return Some(node);
        }
        Some(match self.expr_at(db, setup.expr, loc)? {
            ThirNode::Expr { id, setup: None } if id == setup.expr => {
                ThirNode::Expr { id, setup: Some(setup.stmts.as_slice()) }
            }
            other => other,
        })
    }

    fn pattern_at<'a>(
        &'a self,
        db: &dyn Db,
        pat: &'a ThirPattern,
        loc: Location,
    ) -> Option<ThirNode<'a>> {
        pat.span.encloses(loc).then_some(())?;
        let res = match &pat.kind {
            ThirPatternKind::Any => None,
            ThirPatternKind::Bind { local, .. } => self.local_at(*local, loc),
            ThirPatternKind::Tuple(pats) => {
                pats.iter().find_map(|pat| self.pattern_at(db, pat, loc))
            }
            ThirPatternKind::Struct { fields, .. } => {
                fields.iter().find_map(|(_, pat)| self.pattern_at(db, pat, loc))
            }
            ThirPatternKind::Constructor { args, .. } => match args {
                ThirConstructorArgs::Tuple(items) => {
                    items.iter().find_map(|pat| self.pattern_at(db, pat, loc))
                }
                ThirConstructorArgs::Struct(fields) => fields
                    .iter()
                    .find_map(|fields| self.pattern_at(db, &fields.expr, loc)),
                ThirConstructorArgs::None => None,
            },
            ThirPatternKind::IntLit(_) => None,
            ThirPatternKind::Error => None,
        };
        res.or(pat.is_synthetic.not().then_some(ThirNode::Pattern(pat)))
    }

    fn branch_at<'a>(
        &'a self,
        db: &dyn Db,
        branch: &'a ThirMatchBranch,
        loc: Location,
    ) -> Option<ThirNode<'a>> {
        branch.get_whole_span(self).encloses(loc).then_some(())?;

        let res = self
            .pattern_at(db, &branch.pattern, loc)
            .or_else(|| self.expr_with_setup_at(db, branch.guard.as_ref()?, loc))
            .or_else(|| branch.body.iter().find_map(|stmt| self.stmt_at(db, stmt, loc)));

        res.or(branch.is_synthetic.not().then_some(ThirNode::MatchBranch(branch)))
    }

    fn stmt_at<'a>(
        &'a self,
        db: &dyn Db,
        stmt: &'a ThirStmt,
        loc: Location,
    ) -> Option<ThirNode<'a>> {
        stmt.span.encloses(loc).then_some(())?;
        let res = match &stmt.kind {
            StmtKind::Block { stmts, .. } => {
                stmts.iter().find_map(|stmt| self.stmt_at(db, stmt, loc))
            }
            StmtKind::If { cond, then, else_, .. } => self
                .expr_with_setup_at(db, cond, loc)
                .or_else(|| then.iter().find_map(|stmt| self.stmt_at(db, stmt, loc)))
                .or_else(|| {
                    else_.as_ref()?.iter().find_map(|stmt| self.stmt_at(db, stmt, loc))
                }),
            StmtKind::While { cond, body, .. } => self
                .expr_with_setup_at(db, cond, loc)
                .or_else(|| body.iter().find_map(|stmt| self.stmt_at(db, stmt, loc))),
            StmtKind::Let { local, init } => {
                self.local_at(*local, loc).or_else(|| self.expr_at(db, *init, loc))
            }
            StmtKind::Assign { place, rhs } => {
                self.place_at(*place, loc).or_else(|| self.expr_at(db, *rhs, loc))
            }
            StmtKind::Return(idx) => idx.and_then(|expr| self.expr_at(db, expr, loc)),
            StmtKind::Break(_) => None,
            StmtKind::Continue(_) => None,
            StmtKind::Match { scrutinee, branches } => self
                .expr_with_setup_at(db, scrutinee, loc)
                .or_else(|| branches.iter().find_map(|br| self.branch_at(db, br, loc))),
            StmtKind::Expr(idx) => self.expr_at(db, *idx, loc),
            StmtKind::Error => None,
        };
        res.or(stmt.is_synthetic.not().then_some(ThirNode::Stmt(stmt)))
    }

    pub fn node_at<'a>(&'a self, db: &dyn Db, loc: Location) -> Option<ThirNode<'a>> {
        self.root.iter().find_map(|stmt| self.stmt_at(db, stmt, loc))
    }
}

impl ThirNode<'_> {
    pub fn span(&self, thir: &Thir) -> Span {
        match self {
            ThirNode::Expr { id, .. } => thir.exprs[*id].span,
            ThirNode::Place(idx) => thir.places[*idx].span,
            ThirNode::Local(idx) => thir.locals[*idx].span,
            ThirNode::Stmt(thir_stmt) => thir_stmt.span,
            ThirNode::Pattern(thir_pattern) => thir_pattern.span,
            ThirNode::MatchBranch(thir_match_branch) => {
                thir_match_branch.get_whole_span(thir)
            }
            ThirNode::EnumVariant {  span, .. } |
            ThirNode::StructField {  span, .. } => *span,
        }
    }
}
