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

use std::sync::Arc;

use salsa::Accumulator;

use crate::{
    Db,
    compiler::{diagnostic::Diag, get_sig_of_function},
    ril::{BuiltinTypeId, TypeDefId, TypeId, TypeRef},
    thir::{
        ExprId, ExprKind, Thir, ThirConstructorArgs,
        stmt::{StmtKind, ThirStmt},
    },
};

pub fn validate_thir(db: &dyn Db, thir: &Thir) {
    check_return(db, thir);
    let span = thir.id.span(db);
    Diag::todo(
        format!("Implement validate_thir ({}:{})", file!(), line!()),
        span,
    )
    .accumulate(db);
}

pub fn check_return(db: &dyn Db, thir: &Thir) {
    check_return_for_block(db, thir, &thir.root);
}

fn get_never_ty(db: &dyn Db) -> TypeRef {
    TypeRef::Concrete(TypeId::new(
        db,
        TypeDefId::Builtin(BuiltinTypeId::never(db)),
        vec![],
    ))
}

pub fn expr_contains_never(db: &dyn Db, thir: &Thir, expr: ExprId) -> bool {
    let thir_expr = &thir.exprs[expr];
    if thir_expr.ty == get_never_ty(db) {
        return true;
    }
    match &thir_expr.kind {
        ExprKind::BinOp { lhs, rhs, .. } => {
            expr_contains_never(db, thir, *lhs) || expr_contains_never(db, thir, *rhs)
        }
        ExprKind::StructLit { fields, .. } => fields
            .iter()
            .any(|item| expr_contains_never(db, thir, item.1)),
        ExprKind::Not(e) | ExprKind::Neg(e) => expr_contains_never(db, thir, *e),
        ExprKind::Call { args: items, .. } | ExprKind::Tuple(items) | ExprKind::SliceLit(items) => {
            items
                .iter()
                .any(|item| expr_contains_never(db, thir, *item))
        }
        ExprKind::Constructor { args, .. } => match args {
            ThirConstructorArgs::Tuple(items) => items
                .iter()
                .any(|item| expr_contains_never(db, thir, *item)),
            ThirConstructorArgs::Struct(items) => items
                .iter()
                .any(|item| expr_contains_never(db, thir, item.1)),
            ThirConstructorArgs::None => false,
        },
        _ => false, // places don't need to be checked as any temporary expression was spilled already
    }
}

pub fn check_return_for_block(db: &dyn Db, thir: &Thir, stmts: &[ThirStmt]) {
    todo!()
}

impl Thir {
    pub fn get_ret_ty(&self, db: &dyn Db) -> TypeRef {
        get_sig_of_function(db, self.id.interned()).ret
    }
}
