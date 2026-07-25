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

use crate::{
    common::{
        location::{self, Span},
        symbols::Symbol,
    },
    lexer::TokenKind,
    parse_tree::{
        Spanned,
        expr::{AstExpr, AstExprDesc, AstStructField, BinaryOperator},
        type_expr::{AstAnyTypeExpr, AstTypeExpr, AstTypeExprDesc},
    },
    parser::{ParseError, ParseErrorKind, Parser},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum Assoc {
    Left,
    Right, // Will be used later, maybe
}

fn infix_binding_power(kind: &TokenKind) -> Option<(u8, u8, Assoc)> {
    let (prec, assoc) = match kind {
        TokenKind::DotDot => (2, Assoc::Left),
        TokenKind::Or => (3, Assoc::Left),
        TokenKind::And => (4, Assoc::Left),
        TokenKind::BitOr => (5, Assoc::Left),
        TokenKind::BitXor => (6, Assoc::Left),
        TokenKind::BitAnd => (7, Assoc::Left),
        TokenKind::Diff => (8, Assoc::Left),
        TokenKind::EqEq => (8, Assoc::Left),
        TokenKind::Lt | TokenKind::Leq | TokenKind::Gt | TokenKind::Geq => {
            (9, Assoc::Left)
        }
        TokenKind::Plus | TokenKind::Minus => (10, Assoc::Left),
        TokenKind::Mult | TokenKind::Div | TokenKind::Modulo => (11, Assoc::Left),
        TokenKind::As => (12, Assoc::Left),
        _ => return None,
    };
    let (lbp, rbp) = match assoc {
        Assoc::Left => (prec * 2, prec * 2),
        Assoc::Right => (prec * 2, prec * 2 - 1),
    };
    Some((lbp, rbp, assoc))
}

fn token_to_binop(kind: &TokenKind) -> Option<BinaryOperator> {
    match kind {
        TokenKind::Plus => Some(BinaryOperator::Plus),
        TokenKind::Minus => Some(BinaryOperator::Minus),
        TokenKind::Mult => Some(BinaryOperator::Times),
        TokenKind::Div => Some(BinaryOperator::Div),
        TokenKind::Modulo => Some(BinaryOperator::Modulo),
        TokenKind::EqEq => Some(BinaryOperator::Eq),
        TokenKind::Diff => Some(BinaryOperator::Diff),
        TokenKind::Lt => Some(BinaryOperator::Lt),
        TokenKind::Leq => Some(BinaryOperator::Leq),
        TokenKind::Gt => Some(BinaryOperator::Gt),
        TokenKind::Geq => Some(BinaryOperator::Geq),
        TokenKind::And => Some(BinaryOperator::And),
        TokenKind::Or => Some(BinaryOperator::Or),
        TokenKind::BitAnd => Some(BinaryOperator::BitAnd),
        TokenKind::BitOr => Some(BinaryOperator::BitOr),
        TokenKind::BitXor => Some(BinaryOperator::BitXor),
        _ => None,
    }
}

impl Parser<'_> {
    pub(super) fn parse_expr(&mut self) -> Result<AstExpr, ParseError> {
        self.parse_binop(0)
    }

    fn parse_expr_unrestricted(&mut self) -> Result<AstExpr, ParseError> {
        self.with_struct_lit_restriction(false, Self::parse_expr)
    }

    fn parse_binop(&mut self, min_bp: u8) -> Result<AstExpr, ParseError> {
        let mut lhs = self.parse_unary()?;

        while let Some(tok_kind) = self.peek_n(0).map(|t| t.kind) {
            if tok_kind == TokenKind::DotDot {
                let lbp: u8 = 4;
                let rbp: u8 = 4;
                if lbp <= min_bp {
                    break;
                }
                self.consume();
                let rhs = self.parse_binop(rbp)?;
                let span = lhs.span.start().span(self.get_end());
                lhs = Spanned::new(
                    AstExprDesc::Range { from: Box::new(lhs), to: Box::new(rhs) },
                    span,
                );
                continue;
            }

            if let Some((lbp, rbp, _)) = infix_binding_power(&tok_kind) {
                if lbp <= min_bp {
                    break;
                }
                let op_kind = tok_kind;
                self.consume();

                if let Some(op) = token_to_binop(&op_kind) {
                    let rhs = self.parse_binop(rbp)?;
                    let span = lhs.span.start().span(self.get_end());
                    lhs = Spanned::new(
                        AstExprDesc::BinOp { lhs: Box::new(lhs), op, rhs: Box::new(rhs) },
                        span,
                    );
                }

                if op_kind == TokenKind::As {
                    let ty = self.parse_type_expr()?;
                    let span = lhs.span.start().span(self.get_end());
                    lhs = Spanned::new(AstExprDesc::As { expr: Box::new(lhs), ty }, span);
                }
                continue;
            }

            break;
        }

        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<AstExpr, ParseError> {
        let start = self.get_start();
        match self.current_token()?.kind {
            TokenKind::Minus => {
                self.consume();
                let operand = self.parse_unary()?;
                let end = self.get_end();
                Ok(Spanned::new(AstExprDesc::Neg(Box::new(operand)), start.span(end)))
            }
            TokenKind::Not => {
                self.consume();
                let operand = self.parse_unary()?;
                let end = self.get_end();
                Ok(Spanned::new(AstExprDesc::Not(Box::new(operand)), start.span(end)))
            }
            TokenKind::BitAnd => {
                self.consume();
                let mutable = if self
                    .peek_n(0)
                    .is_some_and(|t| matches!(&t.kind, TokenKind::Mut))
                {
                    self.consume();
                    true
                } else {
                    false
                };
                let operand = self.parse_unary()?;
                let end = self.get_end();
                Ok(Spanned::new(
                    AstExprDesc::Ref(mutable, Box::new(operand)),
                    start.span(end),
                ))
            }
            TokenKind::Mult => {
                self.consume();
                let operand = self.parse_unary()?;
                let end = self.get_end();
                Ok(Spanned::new(
                    AstExprDesc::PrefixDeref(Box::new(operand)),
                    start.span(end),
                ))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<AstExpr, ParseError> {
        self.parse_postfix_inner(false)
    }

    fn parse_postfix_inner(&mut self, path_segment: bool) -> Result<AstExpr, ParseError> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek_n(0).map(|t| t.kind) {
                Some(
                    TokenKind::Dot
                    | TokenKind::OpenSqr
                    | TokenKind::AddressOf
                    | TokenKind::Deref
                    | TokenKind::OpenBra,
                ) if path_segment => {
                    break;
                }
                Some(TokenKind::AddressOf) => {
                    self.consume();
                    let end = self.get_end();
                    let span = expr.span.start().span(end);
                    expr = Spanned::new(AstExprDesc::AddressOf(Box::new(expr)), span);
                }

                Some(TokenKind::Deref) => {
                    self.consume();
                    let end = self.get_end();
                    let span = expr.span.start().span(end);
                    expr = Spanned::new(AstExprDesc::PostfixDeref(Box::new(expr)), span);
                }

                Some(TokenKind::Access)
                    if self.peek_n(1).map(|t| &t.kind) == Some(&TokenKind::Lt) =>
                {
                    let start = expr.span.start();
                    self.consume(); // consume `::`; current token is now `<`
                    let type_args = self.parse_turbofish_args()?;

                    match self.peek_n(0).map(|t| t.kind) {
                        // `expr::<T>(args)` — expr is a generic *value* callee
                        // (free function, or path to one).
                        Some(TokenKind::OpenPar) => {
                            self.consume();
                            let args = self.parse_expr_args()?;
                            self.expect(TokenKind::ClosePar)?;
                            self.consume();
                            let end = self.get_end();
                            let span = start.span(end);
                            expr = Spanned::new(
                                AstExprDesc::Call {
                                    callee: Box::new(expr),
                                    args,
                                    type_args,
                                },
                                span,
                            );
                        }
                        // `expr::<T>::name...` — expr is a *type*; produce the
                        // same node shapes as the
                        // `expr<T>::name` spelling.
                        Some(TokenKind::Access) => {
                            self.consume();
                            let lhs_ty = self.reinterpret_expr_as_ty(expr)?;
                            let ty_span = start.span(self.get_end());
                            let ty = self.apply_type_args(lhs_ty, type_args, ty_span)?;
                            expr = self.finish_qualified_path(ty, start)?;
                        }
                        _ => {
                            return Err(self.parse_error(
                                ParseErrorKind::ExpectedSymbol(
                                    "`(` or `::` after turbofish type arguments"
                                        .to_owned(),
                                ),
                            ));
                        }
                    }
                }

                Some(TokenKind::Dot) => {
                    self.consume();
                    if let Some(TokenKind::IntLit(index)) = self.peek_n(0).map(|t| t.kind)
                    {
                        self.consume();
                        let end = self.get_end();
                        let span = expr.span.start().span(end);
                        expr = Spanned::new(
                            AstExprDesc::TupleAccess {
                                object: Box::new(expr),
                                index: index as u32,
                            },
                            span,
                        );
                        continue;
                    }
                    let field = self.parse_symbol()?;
                    let type_args =
                        if self.peek_n(0).map(|t| &t.kind) == Some(&TokenKind::Access) {
                            self.consume();
                            self.parse_turbofish_args()?
                        } else {
                            vec![]
                        };
                    if self.peek_n(0).map(|t| &t.kind) == Some(&TokenKind::OpenPar) {
                        self.consume();
                        let args = self.parse_expr_args()?;
                        self.expect(TokenKind::ClosePar)?;
                        self.consume();
                        let end = self.get_end();
                        let span = expr.span.start().span(end);
                        expr = Spanned::new(
                            AstExprDesc::MethodCall {
                                object: Box::new(expr),
                                method: field.data,
                                args,
                                type_args,
                            },
                            span,
                        );
                    } else {
                        let end = self.get_end();
                        let span = expr.span.start().span(end);
                        expr = Spanned::new(
                            AstExprDesc::FieldAccess {
                                object: Box::new(expr),
                                field: field.data,
                            },
                            span,
                        );
                    }
                }

                Some(TokenKind::OpenSqr) => {
                    self.consume();
                    let index = self.parse_expr_unrestricted()?;
                    self.expect(TokenKind::CloseSqr)?;
                    self.consume();
                    let end = self.get_end();
                    let span = expr.span.start().span(end);
                    expr = Spanned::new(
                        AstExprDesc::Index {
                            object: Box::new(expr),
                            index: Box::new(index),
                        },
                        span,
                    );
                }

                Some(TokenKind::OpenPar) => {
                    self.consume();
                    let args = self.parse_expr_args()?;
                    self.expect(TokenKind::ClosePar)?;
                    self.consume();
                    let end = self.get_end();
                    let span = expr.span.start().span(end);
                    expr = Spanned::new(
                        AstExprDesc::Call {
                            callee: Box::new(expr),
                            args,
                            type_args: vec![],
                        },
                        span,
                    );
                }

                Some(TokenKind::Lt) => {
                    // Try to parse as `expr<T>::method(...)` or
                    // `expr<T>::Variant`. This handles
                    // qualified paths like `a::b::c<T>::Variant { ... }`
                    // where `a::b::c` has already been built up as a
                    // NameResolved expr.
                    let start = expr.span.start();
                    if let Ok(static_call) = self.try_parse_static_call(&expr, start) {
                        expr = static_call;
                    } else if let Ok(qualified_cons) =
                        self.try_parse_qualified_cons(&expr, start)
                    {
                        expr = qualified_cons;
                    } else {
                        // Not a static call or qualified cons — `<` is a
                        // comparison operator.
                        break;
                    }
                }

                Some(TokenKind::OpenBra) => {
                    // Only parse as struct literal if the expression can be
                    // reinterpreted as a type (e.g. a name or qualified path)
                    // AND the brace content starts with '.' (struct field
                    // syntax). This avoids conflicts with
                    // `match expr { ... }` and other
                    // brace-delimited constructs.
                    let is_type_like = matches!(
                        &expr.data,
                        AstExprDesc::Name(_)
                            | AstExprDesc::NameResolved { .. }
                            | AstExprDesc::QualifiedPath { .. }
                    );
                    let has_field_syntax =
                        self.peek_n(1).map(|t| &t.kind) == Some(&TokenKind::Dot);

                    if !self.restrict_struct_lit
                        && is_type_like
                        && (has_field_syntax
                            || self.peek_n(1).map(|t| &t.kind)
                                == Some(&TokenKind::CloseBra))
                    {
                        self.consume();
                        let fields = self.parse_struct_fields()?;
                        self.expect(TokenKind::CloseBra)?;
                        self.consume();

                        let end = self.get_end();
                        let span = expr.span.start().span(end);

                        let (ty, variant) = match expr.data.clone() {
                            AstExprDesc::QualifiedPath { ty, name } => (ty, Some(name)),

                            AstExprDesc::NameResolved { from, ref to }
                                if matches!(to.data, AstExprDesc::Name(_)) =>
                            {
                                let AstExprDesc::Name(variant_name) = to.data else {
                                    unreachable!()
                                };
                                let base_expr =
                                    AstExpr::new(AstExprDesc::Name(from.data), expr.span);
                                (
                                    self.reinterpret_expr_as_ty(base_expr)?,
                                    Some(variant_name),
                                )
                            }

                            _ => (self.reinterpret_expr_as_ty(expr)?, None),
                        };

                        expr = AstExpr::new(
                            AstExprDesc::StructLit { ty, variant, fields },
                            span,
                        );
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn reinterpret_expr_as_ty(&mut self, e: AstExpr) -> Result<AstTypeExpr, ParseError> {
        let Spanned { data, span } = e;

        let data = match data {
            AstExprDesc::Name(symbol) => AstTypeExprDesc::Named {
                name: Spanned { data: symbol, span },
                args: vec![],
            },
            AstExprDesc::NameResolved { from, to } => {
                let to = self.reinterpret_expr_as_ty(*to)?;
                AstTypeExprDesc::NameResolved { from, to: Box::new(to) }
            }
            _ => return Err(self.parse_error(ParseErrorKind::ExpectedTypeName)),
        };

        Ok(Spanned::new(data, span))
    }

    pub(super) fn parse_int_lit(&mut self) -> Result<Spanned<usize>, ParseError> {
        let tok = self.current_token()?.clone();
        match tok.kind {
            TokenKind::IntLit(v) => {
                self.consume();
                Ok(Spanned::new(v as usize, tok.location))
            }
            x => Err(self.parse_error(ParseErrorKind::ExpectedIntLit(x))),
        }
    }

    fn parse_primary(&mut self) -> Result<AstExpr, ParseError> {
        let start = self.get_start();
        let tok = self.current_token()?.clone();

        match tok.kind {
            TokenKind::IntLit(v) => {
                self.consume();
                Ok(Spanned::new(AstExprDesc::IntLit(v), tok.location))
            }
            TokenKind::CharLit(c) => {
                self.consume();
                Ok(Spanned::new(AstExprDesc::CharLit(c), tok.location))
            }
            TokenKind::StrLit(s) => {
                self.consume();
                Ok(Spanned::new(AstExprDesc::StrLit(s), tok.location))
            }
            TokenKind::CStrLit(s) => {
                self.consume();
                Ok(Spanned::new(AstExprDesc::CStrLit(s), tok.location))
            }
            TokenKind::True => {
                self.consume();
                Ok(Spanned::new(AstExprDesc::BoolLit(true), tok.location))
            }
            TokenKind::False => {
                self.consume();
                Ok(Spanned::new(AstExprDesc::BoolLit(false), tok.location))
            }
            TokenKind::OpenBra => {
                self.consume();
                let exprs = self.parse_list(
                    Self::parse_expr_unrestricted,
                    TokenKind::Comma,
                    |p| p.peek_n(0).is_none_or(|t| t.kind == TokenKind::CloseBra),
                )?;
                self.expect(TokenKind::CloseBra)?;
                self.consume();
                Ok(Spanned::new(AstExprDesc::SliceLit(exprs), tok.location))
            }
            TokenKind::OpenPar => {
                self.consume();
                let exprs = self.parse_expr_args()?;
                self.expect(TokenKind::ClosePar)?;
                self.consume();
                let end = self.get_end();
                Ok(Spanned::new(AstExprDesc::Tuple(exprs), start.span(end)))
            }
            TokenKind::Directive(dir) if dir == Symbol::new(self.db, "sizeof") => {
                self.consume();
                self.expect(TokenKind::OpenPar)?;
                self.consume();
                let ty = self.parse_type_expr()?;
                self.expect(TokenKind::ClosePar)?;
                self.consume();
                let end = self.get_end();
                Ok(AstExpr::new(AstExprDesc::SizeOf(ty), start.span(end)))
            }

            TokenKind::Directive(dir) if dir == Symbol::new(self.db, "metadata") => {
                self.consume();

                self.expect(TokenKind::OpenPar)?;
                self.consume();

                let expr = self.parse_expr_unrestricted()?;

                self.expect(TokenKind::ClosePar)?;
                self.consume();

                let end = self.get_end();

                Ok(AstExpr::new(AstExprDesc::Metadata(Box::new(expr)), start.span(end)))
            }

            TokenKind::Directive(dir) if dir == Symbol::new(self.db, "type_name") => {
                self.consume();

                self.expect(TokenKind::OpenPar)?;
                self.consume();

                let ty = self.parse_type_expr()?;

                self.expect(TokenKind::ClosePar)?;
                self.consume();

                let end = self.get_end();

                Ok(AstExpr::new(AstExprDesc::TypeName(ty), start.span(end)))
            }

            TokenKind::Identifier(name) => {
                self.consume();

                match self.peek_n(0).map(|t| t.kind) {
                    Some(TokenKind::Access)
                        if self.peek_n(1).map(|t| &t.kind) != Some(&TokenKind::Lt) =>
                    {
                        self.consume();
                        let rhs = self.parse_postfix_inner(true)?;
                        let end = self.get_end();
                        Ok(Spanned::new(
                            AstExprDesc::NameResolved {
                                from: Spanned::new(name, tok.location),
                                to: Box::new(rhs),
                            },
                            start.span(end),
                        ))
                    }

                    Some(TokenKind::OpenBra)
                        if !self.restrict_struct_lit
                            && self.peek_n(1).map(|t| &t.kind)
                                == Some(&TokenKind::Dot) =>
                    {
                        self.consume();
                        let fields = self.parse_struct_fields()?;
                        self.expect(TokenKind::CloseBra)?;
                        self.consume();
                        let end = self.get_end();
                        let ty_span = start.span(start);
                        let ty = Spanned::new(
                            AstTypeExprDesc::Named {
                                name: Spanned { data: name, span: tok.location },
                                args: vec![],
                            },
                            ty_span,
                        );
                        Ok(Spanned::new(
                            AstExprDesc::StructLit { ty, variant: None, fields },
                            start.span(end),
                        ))
                    }

                    _ => {
                        let end = self.get_end();
                        Ok(Spanned::new(AstExprDesc::Name(name), start.span(end)))
                    }
                }
            }
            kind => Err(self.parse_error(ParseErrorKind::ExpectedSymbol(
                kind.display(self.db).to_string(),
            ))),
        }
    }

    fn parse_static_call(
        &mut self,
        lhs: AstExpr,
        start: location::Location,
    ) -> Result<AstExpr, ParseError> {
        let lhs_ty = self.reinterpret_expr_as_ty(lhs)?;

        let type_args = self.parse_turbofish_args()?;

        self.expect(TokenKind::Access)?;
        self.consume();

        let ty_span = start.span(self.get_end());
        let ty = self.apply_type_args(lhs_ty, type_args, ty_span)?;

        let method_sym = self.parse_symbol()?;

        self.expect(TokenKind::OpenPar)?;
        self.consume();
        let args = self.parse_expr_args()?;
        self.expect(TokenKind::ClosePar)?;
        self.consume();

        let end = self.get_end();
        Ok(Spanned::new(
            AstExprDesc::StaticCall {
                ty,
                method: method_sym.data,
                args,
                type_args: vec![], // TODO
            },
            start.span(end),
        ))
    }

    fn try_parse_static_call(
        &mut self,
        lhs: &AstExpr,
        start: location::Location,
    ) -> Result<AstExpr, ParseError> {
        self.speculate(|p| p.parse_static_call(lhs.clone(), start))
    }

    fn parse_qualified_cons(
        &mut self,
        lhs: AstExpr,
        start: location::Location,
    ) -> Result<AstExpr, ParseError> {
        let lhs_ty = self.reinterpret_expr_as_ty(lhs)?;

        let type_args = self.parse_turbofish_args()?;

        self.expect(TokenKind::Access)?;
        self.consume();

        // Parse variant name first — it's part of the qualified path,
        // not part of a struct literal. This gives us:
        //   (Type<Args>::Variant) { .fields }
        // instead of:
        //   Type<Args>::(Variant { .fields })
        let variant_sym = self.parse_symbol()?;

        let ty_span = start.span(self.get_end());
        let ty = self.apply_type_args(lhs_ty, type_args, ty_span)?;

        if !self.restrict_struct_lit
            && self.peek_n(0).is_some_and(|t| matches!(t.kind, TokenKind::OpenBra))
        {
            self.consume();
            let fields = self.parse_struct_fields()?;
            self.expect(TokenKind::CloseBra)?;
            self.consume();
            let end = self.get_end();
            Ok(Spanned::new(
                AstExprDesc::StructLit { ty, variant: Some(variant_sym.data), fields },
                start.span(end),
            ))
        } else if self.peek_n(0).is_some_and(|t| matches!(t.kind, TokenKind::OpenPar)) {
            self.consume();
            let args = self.parse_expr_args()?;
            self.expect(TokenKind::ClosePar)?;
            self.consume();
            let end = self.get_end();
            Ok(Spanned::new(
                AstExprDesc::StaticCall {
                    ty,
                    method: variant_sym.data,
                    args,
                    type_args: vec![], // TODO
                },
                start.span(end),
            ))
        } else {
            let end = self.get_end();
            Ok(Spanned::new(
                AstExprDesc::QualifiedPath { ty, name: variant_sym.data },
                start.span(end),
            ))
        }
    }

    fn parse_turbofish_args(&mut self) -> Result<Vec<AstAnyTypeExpr>, ParseError> {
        self.expect(TokenKind::Lt)?;
        self.consume();
        let type_args = self.parse_any_type_args()?;
        self.expect(TokenKind::Gt)?;
        self.consume();
        Ok(type_args)
    }

    fn finish_qualified_path(
        &mut self,
        ty: AstTypeExpr,
        start: location::Location,
    ) -> Result<AstExpr, ParseError> {
        let name_sym = self.parse_symbol()?;

        // Struct literal only with field syntax (or empty braces) — this path
        // is committed (no backtracking), so it must not swallow a `match`
        // block by accident.
        let is_struct_lit = !self.restrict_struct_lit
            && self.peek_n(0).is_some_and(|t| matches!(t.kind, TokenKind::OpenBra))
            && self
                .peek_n(1)
                .is_some_and(|t| matches!(t.kind, TokenKind::Dot | TokenKind::CloseBra));

        if is_struct_lit {
            self.consume();
            let fields = self.parse_struct_fields()?;
            self.expect(TokenKind::CloseBra)?;
            self.consume();
            let end = self.get_end();
            Ok(Spanned::new(
                AstExprDesc::StructLit { ty, variant: Some(name_sym.data), fields },
                start.span(end),
            ))
        } else {
            // Optional method turbofish before the call parens.
            let type_args =
                if self.peek_n(0).is_some_and(|t| matches!(t.kind, TokenKind::Access))
                    && self.peek_n(1).map(|t| &t.kind) == Some(&TokenKind::Lt)
                {
                    self.consume(); // `::`
                    let ta = self.parse_turbofish_args()?;
                    // Method turbofish is only meaningful on a call.
                    self.expect(TokenKind::OpenPar)?;
                    ta
                } else {
                    vec![]
                };

            if self.peek_n(0).is_some_and(|t| matches!(t.kind, TokenKind::OpenPar)) {
                self.consume();
                let args = self.parse_expr_args()?;
                self.expect(TokenKind::ClosePar)?;
                self.consume();
                let end = self.get_end();
                Ok(Spanned::new(
                    AstExprDesc::StaticCall {
                        ty,
                        method: name_sym.data,
                        args,
                        type_args,
                    },
                    start.span(end),
                ))
            } else {
                let end = self.get_end();
                Ok(Spanned::new(
                    AstExprDesc::QualifiedPath { ty, name: name_sym.data },
                    start.span(end),
                ))
            }
        }
    }

    fn try_parse_qualified_cons(
        &mut self,
        lhs: &AstExpr,
        start: location::Location,
    ) -> Result<AstExpr, ParseError> {
        self.speculate(|p| p.parse_qualified_cons(lhs.clone(), start))
    }

    /// Given a base type expression and newly parsed type args, produce the
    /// final type expression. For a `Named` type this fills in the args; for
    /// other shapes it errors.
    fn apply_type_args(
        &self,
        base: AstTypeExpr,
        args: Vec<AstAnyTypeExpr>,
        span: Span,
    ) -> Result<AstTypeExpr, ParseError> {
        match base.data {
            AstTypeExprDesc::Named { name, args: _ } => {
                Ok(Spanned::new(AstTypeExprDesc::Named { name, args }, span))
            }
            _ => Err(self.parse_error(ParseErrorKind::ExpectedTypeName)),
        }
    }

    fn parse_struct_fields(&mut self) -> Result<Vec<AstStructField>, ParseError> {
        self.parse_list(
            |p| {
                p.expect(TokenKind::Dot)?;
                p.consume();
                let name = p.parse_symbol()?;
                p.expect(TokenKind::Colon)?;
                p.consume();
                let value = p.parse_expr_unrestricted()?;
                Ok(AstStructField { name: name.data, name_span: name.span, value })
            },
            TokenKind::Comma,
            |p| p.peek_n(0).is_none_or(|t| t.kind == TokenKind::CloseBra),
        )
    }

    fn parse_expr_args(&mut self) -> Result<Vec<AstExpr>, ParseError> {
        self.parse_list(Self::parse_expr_unrestricted, TokenKind::Comma, |p| {
            p.peek_n(0).is_none_or(|t| t.kind == TokenKind::ClosePar)
        })
    }
}
