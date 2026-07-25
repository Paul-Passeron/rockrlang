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
    common::symbols::Symbol,
    lexer::TokenKind,
    parse_tree::{
        Spanned,
        type_expr::{AstAnyTypeExpr, AstAnyTypeExprDesc, AstTypeExpr, AstTypeExprDesc},
    },
    parser::{ParseError, ParseErrorKind, Parser},
};

impl Parser<'_> {
    pub(super) fn parse_type_expr(&mut self) -> Result<AstTypeExpr, ParseError> {
        let start = self.get_start();
        let tok = self.current_token()?;
        match tok.kind {
            TokenKind::Mult | TokenKind::BitAnd | TokenKind::And => {
                let is_ptr = if let Some(t) = self.peek_n(0)
                    && matches!(t.kind, TokenKind::Mult)
                {
                    true
                } else {
                    false
                };
                let two = self
                    .current_token()
                    .is_ok_and(|t| matches!(t.kind, TokenKind::And));
                self.consume();
                let mutable = if let Some(t) = self.peek_n(0)
                    && matches!(t.kind, TokenKind::Mut)
                {
                    self.consume();
                    true
                } else {
                    false
                };

                let inner = self.parse_any_type_expr()?;
                let end = self.get_end();
                if two {
                    Ok(Spanned::new(
                        AstTypeExprDesc::Ref {
                            // &&<mut|""> so mutable only on the inner ref
                            mutable: false,
                            pointee: Box::new(
                                Spanned::new(
                                    AstTypeExprDesc::Ref {
                                        mutable,
                                        pointee: Box::new(inner),
                                    },
                                    start.advance(1).span(end),
                                )
                                .into(),
                            ),
                        },
                        start.span(end),
                    ))
                } else {
                    Ok(Spanned::new(
                        if is_ptr {
                            AstTypeExprDesc::Pointer { mutable, pointee: Box::new(inner) }
                        } else {
                            AstTypeExprDesc::Ref { mutable, pointee: Box::new(inner) }
                        },
                        start.span(end),
                    ))
                }
            }

            TokenKind::OpenSqr => {
                self.consume();
                let ty = self.parse_any_type_expr()?;
                let len = if self.peek_n(0).map(|t| t.kind) == Some(TokenKind::Semicolon)
                {
                    self.consume();
                    Some(self.parse_int_lit()?.data)
                } else {
                    None
                };
                self.expect(TokenKind::CloseSqr)?;
                self.consume();
                let end = self.get_end();
                Ok(Spanned::new(
                    AstTypeExprDesc::Slice { ty: Box::new(ty), len },
                    start.span(end),
                ))
            }

            TokenKind::OpenPar => {
                self.consume();
                let tys = self.parse_any_type_args()?;
                self.expect(TokenKind::ClosePar)?;
                self.consume();
                let end = self.get_end();

                Ok(AstTypeExpr::new(AstTypeExprDesc::Tuple(tys), start.span(end)))
            }

            TokenKind::Identifier(name) => {
                let span = tok.location;
                self.consume();

                match self.peek_n(0).map(|t| t.kind) {
                    Some(TokenKind::Access) => {
                        self.consume();
                        let rhs = self.parse_type_expr()?;
                        let end = self.get_end();
                        Ok(Spanned::new(
                            AstTypeExprDesc::NameResolved {
                                from: Spanned { data: name, span },
                                to: Box::new(rhs),
                            },
                            start.span(end),
                        ))
                    }

                    Some(TokenKind::Lt) => {
                        self.consume();
                        let args = self.parse_any_type_args()?;
                        self.expect(TokenKind::Gt)?;
                        self.consume();
                        let end = self.get_end();
                        Ok(Spanned::new(
                            AstTypeExprDesc::Named {
                                name: Spanned { data: name, span },
                                args,
                            },
                            start.span(end),
                        ))
                    }

                    _ => {
                        let end = self.get_end();
                        Ok(Spanned::new(
                            AstTypeExprDesc::Named {
                                name: Spanned { data: name, span },
                                args: vec![],
                            },
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

    pub(super) fn parse_any_type_expr(&mut self) -> Result<AstAnyTypeExpr, ParseError> {
        let start = self.get_start();

        if let TokenKind::Identifier(name) = self.current_token()?.kind
            && name == Symbol::new(self.db, "_")
        {
            self.consume();
            let end = self.get_end();
            return Ok(Spanned::new(AstAnyTypeExprDesc::Any, start.span(end)));
        }

        let ty = self.parse_type_expr()?;
        let span = ty.span;
        Ok(Spanned::new(AstAnyTypeExprDesc::Known(ty.data), span))
    }

    pub fn parse_any_type_args(&mut self) -> Result<Vec<AstAnyTypeExpr>, ParseError> {
        self.parse_list(Self::parse_any_type_expr, TokenKind::Comma, |p| {
            p.peek_n(0).is_none_or(|t| t.kind == TokenKind::ClosePar)
        })
    }
}
