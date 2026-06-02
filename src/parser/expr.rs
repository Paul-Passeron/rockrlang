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
        TokenKind::Mult | TokenKind::Div | TokenKind::Modulo => {
            (11, Assoc::Left)
        }
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

impl<'db> Parser<'db> {
    pub(super) fn parse_expr(&mut self) -> Result<AstExpr, ParseError> {
        self.parse_binop(0)
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
                    AstExprDesc::Range {
                        from: Box::new(lhs),
                        to: Box::new(rhs),
                    },
                    vec![],
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
                        AstExprDesc::BinOp {
                            lhs: Box::new(lhs),
                            op,
                            rhs: Box::new(rhs),
                        },
                        vec![],
                        span,
                    );
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
                Ok(Spanned::new(
                    AstExprDesc::Neg(Box::new(operand)),
                    vec![],
                    start.span(end),
                ))
            }
            TokenKind::Not => {
                self.consume();
                let operand = self.parse_unary()?;
                let end = self.get_end();
                Ok(Spanned::new(
                    AstExprDesc::Not(Box::new(operand)),
                    vec![],
                    start.span(end),
                ))
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
                    vec![],
                    start.span(end),
                ))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<AstExpr, ParseError> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek_n(0).map(|t| t.kind) {
                Some(TokenKind::AddressOf) => {
                    self.consume();
                    let end = self.get_end();
                    let span = expr.span.start().span(end);
                    expr = Spanned::new(
                        AstExprDesc::AddressOf(Box::new(expr)),
                        vec![],
                        span,
                    );
                }

                Some(TokenKind::Deref) => {
                    self.consume();
                    let end = self.get_end();
                    let span = expr.span.start().span(end);
                    expr = Spanned::new(
                        AstExprDesc::PostfixDeref(Box::new(expr)),
                        vec![],
                        span,
                    );
                }

                Some(TokenKind::Dot) => {
                    self.consume();
                    if let Some(TokenKind::IntLit(index)) =
                        self.peek_n(0).map(|t| t.kind)
                    {
                        self.consume();
                        let end = self.get_end();
                        let span = expr.span.start().span(end);
                        expr = Spanned::new(
                            AstExprDesc::TupleAccess {
                                object: Box::new(expr),
                                index: index as u32,
                            },
                            vec![],
                            span,
                        );
                        continue;
                    }
                    let field = self.parse_symbol()?;
                    if self.peek_n(0).map(|t| &t.kind)
                        == Some(&TokenKind::OpenPar)
                    {
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
                            },
                            vec![],
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
                            vec![],
                            span,
                        );
                    }
                }

                Some(TokenKind::OpenSqr) => {
                    self.consume();
                    let index = self.parse_expr()?;
                    self.expect(TokenKind::CloseSqr)?;
                    self.consume();
                    let end = self.get_end();
                    let span = expr.span.start().span(end);
                    expr = Spanned::new(
                        AstExprDesc::Index {
                            object: Box::new(expr),
                            index: Box::new(index),
                        },
                        vec![],
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
                        },
                        vec![],
                        span,
                    );
                }

                Some(TokenKind::Lt) => {
                    // Try to parse as `expr<T>::method(...)` or `expr<T>::Variant`.
                    // This handles qualified paths like `a::b::c<T>::Variant { ... }`
                    // where `a::b::c` has already been built up as a NameResolved expr.
                    let start = expr.span.start();
                    if let Some(static_call) =
                        self.try_parse_static_call(&expr, start.clone())
                    {
                        expr = static_call;
                    } else if let Some(qualified_cons) =
                        self.try_parse_qualified_cons(&expr, start.clone())
                    {
                        expr = qualified_cons;
                    } else {
                        // Not a static call or qualified cons — `<` is a comparison operator.
                        break;
                    }
                }

                Some(TokenKind::OpenBra) => {
                    // Only parse as struct literal if the expression can be
                    // reinterpreted as a type (e.g. a name or qualified path)
                    // AND the brace content starts with '.' (struct field syntax).
                    // This avoids conflicts with `match expr { ... }` and other
                    // brace-delimited constructs.
                    let is_type_like = matches!(
                        &expr.data,
                        AstExprDesc::Name(_)
                            | AstExprDesc::NameResolved { .. }
                            | AstExprDesc::QualifiedPath { .. }
                    );
                    let has_field_syntax = self.peek_n(1).map(|t| &t.kind)
                        == Some(&TokenKind::Dot);

                    if is_type_like && has_field_syntax {
                        self.consume();
                        let fields = self.parse_struct_fields()?;
                        self.expect(TokenKind::CloseBra)?;
                        self.consume();

                        let end = self.get_end();
                        let span = expr.span.start().span(end);

                        let (ty, variant) = match expr.data.clone() {
                            AstExprDesc::QualifiedPath { ty, name } => {
                                (ty, Some(name))
                            }

                            AstExprDesc::NameResolved { from, ref to }
                                if matches!(to.data, AstExprDesc::Name(_)) =>
                            {
                                let AstExprDesc::Name(variant_name) = to.data
                                else {
                                    unreachable!()
                                };
                                let base_expr = AstExpr::new(
                                    AstExprDesc::Name(from),
                                    vec![],
                                    expr.span.clone(),
                                );
                                (
                                    self.reinterpret_expr_as_ty(base_expr)?,
                                    Some(variant_name),
                                )
                            }

                            _ => (self.reinterpret_expr_as_ty(expr)?, None),
                        };

                        expr = AstExpr::new(
                            AstExprDesc::StructLit {
                                ty,
                                variant,
                                fields,
                            },
                            vec![],
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

    fn reinterpret_expr_as_ty(
        &mut self,
        e: AstExpr,
    ) -> Result<AstTypeExpr, ParseError> {
        let Spanned {
            data,
            annotations,
            span,
        } = e;

        let data = match data {
            AstExprDesc::Name(symbol) => AstTypeExprDesc::Named {
                name: symbol,
                args: vec![],
            },
            AstExprDesc::NameResolved { from, to } => {
                let to = self.reinterpret_expr_as_ty(*to)?;
                AstTypeExprDesc::NameResolved {
                    from,
                    to: Box::new(to),
                }
            }
            _ => unreachable!(),
        };

        Ok(Spanned::new(data, annotations, span))
    }

    pub(super) fn parse_int_lit(
        &mut self,
    ) -> Result<Spanned<usize>, ParseError> {
        let tok = self.current_token()?.clone();
        match tok.kind {
            TokenKind::IntLit(v) => {
                self.consume();
                Ok(Spanned::new(v as usize, vec![], tok.location))
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
                Ok(Spanned::new(AstExprDesc::IntLit(v), vec![], tok.location))
            }
            TokenKind::CharLit(c) => {
                self.consume();
                Ok(Spanned::new(AstExprDesc::CharLit(c), vec![], tok.location))
            }
            TokenKind::StrLit(s) => {
                self.consume();
                Ok(Spanned::new(AstExprDesc::StrLit(s), vec![], tok.location))
            }
            TokenKind::CStrLit(s) => {
                self.consume();
                Ok(Spanned::new(AstExprDesc::CStrLit(s), vec![], tok.location))
            }
            TokenKind::True => {
                self.consume();
                Ok(Spanned::new(
                    AstExprDesc::BoolLit(true),
                    vec![],
                    tok.location,
                ))
            }
            TokenKind::False => {
                self.consume();
                Ok(Spanned::new(
                    AstExprDesc::BoolLit(false),
                    vec![],
                    tok.location,
                ))
            }
            TokenKind::OpenBra => {
                self.consume();
                let mut exprs = vec![];
                while let Some(t) = self.peek_n(0)
                    && !matches!(t.kind, TokenKind::CloseBra)
                {
                    exprs.push(self.parse_expr()?);
                    if self.peek_n(0).map(|t| &t.kind)
                        == Some(&TokenKind::Comma)
                    {
                        self.consume();
                    } else {
                        break;
                    }
                }
                self.expect(TokenKind::CloseBra)?;
                self.consume();
                Ok(Spanned::new(
                    AstExprDesc::SliceLit(exprs),
                    vec![],
                    tok.location,
                ))
            }
            TokenKind::OpenPar => {
                self.consume();
                let mut exprs = vec![];
                while let Some(t) = self.peek_n(0)
                    && !matches!(t.kind, TokenKind::ClosePar)
                {
                    exprs.push(self.parse_expr()?);
                    if self.peek_n(0).map(|t| &t.kind)
                        == Some(&TokenKind::Comma)
                    {
                        self.consume();
                    } else {
                        break;
                    }
                }
                self.expect(TokenKind::ClosePar)?;
                self.consume();
                let end = self.get_end();
                Ok(Spanned::new(
                    AstExprDesc::Tuple(exprs),
                    vec![],
                    start.span(end),
                ))
            }
            TokenKind::Directive(dir)
                if dir == Symbol::new(self.db, "sizeof") =>
            {
                self.consume();

                self.expect(TokenKind::OpenPar)?;
                self.consume();

                let ty = self.parse_type_expr()?;

                self.expect(TokenKind::ClosePar)?;
                self.consume();

                let end = self.get_end();

                Ok(AstExpr::new(
                    AstExprDesc::SizeOf(ty),
                    vec![],
                    start.span(end),
                ))
            }

            TokenKind::Identifier(name) => {
                self.consume();

                match self.peek_n(0).map(|t| t.kind) {
                    Some(TokenKind::Access) => {
                        self.consume();
                        let rhs = self.parse_postfix()?;
                        let end = self.get_end();
                        Ok(Spanned::new(
                            AstExprDesc::NameResolved {
                                from: name,
                                to: Box::new(rhs),
                            },
                            vec![],
                            start.span(end),
                        ))
                    }

                    Some(TokenKind::OpenBra) => {
                        if self.peek_n(1).map(|t| &t.kind)
                            == Some(&TokenKind::Dot)
                        {
                            self.consume();
                            let fields = self.parse_struct_fields()?;
                            self.expect(TokenKind::CloseBra)?;
                            self.consume();
                            let end = self.get_end();
                            let ty_span = start.span(start);
                            let ty = Spanned::new(
                                AstTypeExprDesc::Named { name, args: vec![] },
                                vec![],
                                ty_span,
                            );
                            Ok(Spanned::new(
                                AstExprDesc::StructLit {
                                    ty,
                                    variant: None,
                                    fields,
                                },
                                vec![],
                                start.span(end),
                            ))
                        } else {
                            let end = self.get_end();
                            Ok(Spanned::new(
                                AstExprDesc::Name(name),
                                vec![],
                                start.span(end),
                            ))
                        }
                    }

                    // `<` is not handled here — just return the Name and let
                    // parse_postfix handle it for static calls / qualified paths.
                    // This way `a::b::c<T>::Variant` works correctly: `a::b::c`
                    // is built up as NameResolved through the Access arm, then
                    // postfix sees `<` and tries static_call / qualified_cons
                    // with the full expression (not just the last identifier).
                    _ => {
                        let end = self.get_end();
                        Ok(Spanned::new(
                            AstExprDesc::Name(name),
                            vec![],
                            start.span(end),
                        ))
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

        self.consume(); // consume `<`

        let mut type_args: Vec<AstAnyTypeExpr> = vec![];

        while let Some(t) = self.peek_n(0)
            && t.kind != TokenKind::Gt
        {
            type_args.push(self.parse_any_type_expr()?);

            if let Some(t) = self.peek_n(0)
                && t.kind == TokenKind::Comma
            {
                self.consume();
            } else {
                break;
            }
        }

        self.expect(TokenKind::Gt)?;
        self.consume();

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
            },
            vec![],
            start.span(end),
        ))
    }

    fn try_parse_static_call(
        &mut self,
        lhs: &AstExpr,
        start: location::Location,
    ) -> Option<AstExpr> {
        let saved = self.position;
        if let Ok(res) = self.parse_static_call(lhs.clone(), start) {
            return Some(res);
        }
        self.position = saved;
        None
    }

    fn parse_qualified_cons(
        &mut self,
        lhs: AstExpr,
        start: location::Location,
    ) -> Result<AstExpr, ParseError> {
        let lhs_ty = self.reinterpret_expr_as_ty(lhs)?;

        self.consume(); // consume `<`

        let mut type_args: Vec<AstAnyTypeExpr> = vec![];

        while let Some(t) = self.peek_n(0)
            && t.kind != TokenKind::Gt
        {
            type_args.push(self.parse_any_type_expr()?);
            if let Some(t) = self.peek_n(0)
                && t.kind == TokenKind::Comma
            {
                self.consume();
            } else {
                break;
            }
        }

        self.expect(TokenKind::Gt)?;
        self.consume();

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

        if self
            .peek_n(0)
            .is_some_and(|t| matches!(t.kind, TokenKind::OpenBra))
        {
            self.consume();
            let fields = self.parse_struct_fields()?;
            self.expect(TokenKind::CloseBra)?;
            self.consume();
            let end = self.get_end();
            Ok(Spanned::new(
                AstExprDesc::StructLit {
                    ty,
                    variant: Some(variant_sym.data),
                    fields,
                },
                vec![],
                start.span(end),
            ))
        } else if self
            .peek_n(0)
            .is_some_and(|t| matches!(t.kind, TokenKind::OpenPar))
        {
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
                },
                vec![],
                start.span(end),
            ))
        } else {
            let end = self.get_end();
            Ok(Spanned::new(
                AstExprDesc::QualifiedPath {
                    ty,
                    name: variant_sym.data,
                },
                vec![],
                start.span(end),
            ))
        }
    }

    fn try_parse_qualified_cons(
        &mut self,
        lhs: &AstExpr,
        start: location::Location,
    ) -> Option<AstExpr> {
        let saved = self.position;
        if let Ok(expr) = self.parse_qualified_cons(lhs.clone(), start) {
            return Some(expr);
        }
        self.position = saved;
        None
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
            AstTypeExprDesc::Named { name, args: _ } => Ok(Spanned::new(
                AstTypeExprDesc::Named { name, args },
                vec![],
                span,
            )),
            _ => Err(self.parse_error(ParseErrorKind::ExpectedSymbol(
                "type name".to_string(),
            ))),
        }
    }

    fn parse_struct_fields(
        &mut self,
    ) -> Result<Vec<AstStructField>, ParseError> {
        let mut fields = vec![];
        loop {
            match self.peek_n(0).map(|t| t.kind) {
                Some(TokenKind::CloseBra) | None => break,
                Some(TokenKind::Dot) => {
                    self.consume(); // consume '.'
                    let name = self.parse_symbol()?;
                    self.expect(TokenKind::Colon)?;
                    self.consume(); // consume ':'
                    let value = self.parse_expr()?;
                    fields.push(AstStructField {
                        name: name.data,
                        value,
                    });
                    match self.peek_n(0).map(|t| t.kind) {
                        Some(TokenKind::Comma) => {
                            self.consume();
                        }
                        _ => break,
                    }
                }
                _ => break,
            }
        }
        Ok(fields)
    }

    fn parse_expr_args(&mut self) -> Result<Vec<AstExpr>, ParseError> {
        let mut args = vec![];
        while let Some(t) = self.peek_n(0)
            && !matches!(t.kind, TokenKind::ClosePar)
        {
            args.push(self.parse_expr()?);
            if let Some(t) = self.peek_n(0)
                && matches!(t.kind, TokenKind::Comma)
            {
                self.consume();
            } else {
                break;
            }
        }
        Ok(args)
    }
}
