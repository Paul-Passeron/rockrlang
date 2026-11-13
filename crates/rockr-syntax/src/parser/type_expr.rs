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

use rockr_base::Location;

use crate::{
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
                let two =
                    self.current_token().is_ok_and(|t| matches!(t.kind, TokenKind::And));
                self.consume();
                let mutable = if let Some(t) = self.peek_n(0)
                    && matches!(t.kind, TokenKind::Mut)
                {
                    self.consume();
                    true
                } else {
                    false
                };

                if two {
                    let inner = self.parse_pointer_like(
                        start.advance(self.db, 1),
                        false,
                        mutable,
                    )?;
                    let end = self.get_end();
                    Ok(Spanned::new(
                        AstTypeExprDesc::Ref {
                            mutable: false,
                            pointee: Box::new(inner.into()),
                        },
                        start.span(end),
                    ))
                } else {
                    self.parse_pointer_like(start, is_ptr, mutable)
                }
            }

            TokenKind::OpenSqr => {
                let (elem, len) = self.parse_bracketed_type()?;
                let Some(len) = len else {
                    return Err(self.parse_error(ParseErrorKind::SliceMustBeBehindRef));
                };
                let end = self.get_end();
                Ok(Spanned::new(
                    AstTypeExprDesc::Array { elem: Box::new(elem), len },
                    start.span(end),
                ))
            }

            TokenKind::OpenPar => {
                self.consume();
                let tys = self.parse_any_type_args()?;
                self.eat(TokenKind::ClosePar)?;
                let end = self.get_end();

                Ok(AstTypeExpr::new(AstTypeExprDesc::Tuple(tys), start.span(end)))
            }

            TokenKind::Identifier(name) => {
                let span = tok.span;
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
                        self.eat(TokenKind::Gt)?;
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

    /// Parses what follows `&`, `&mut`, `*` or `*mut` (already consumed;
    /// `start` is where that prefix began).
    ///
    /// `&[T]` / `&mut [T]` become a `Slice`, `&[T; N]` a `Ref` to an `Array`.
    /// For `*`, the pointee is parsed as a normal type, so `*[T; N]` is a
    /// pointer to an array and `*[T]` is rejected.
    fn parse_pointer_like(
        &mut self,
        start: Location,
        is_ptr: bool,
        mutable: bool,
    ) -> Result<AstTypeExpr, ParseError> {
        if !is_ptr && self.peek_n(0).is_some_and(|t| t.kind == TokenKind::OpenSqr) {
            let array_start = self.get_start();
            let (elem, len) = self.parse_bracketed_type()?;
            let end = self.get_end();
            let desc = match len {
                None => AstTypeExprDesc::Slice { mutable, elem: Box::new(elem) },
                Some(len) => AstTypeExprDesc::Ref {
                    mutable,
                    pointee: Box::new(
                        Spanned::new(
                            AstTypeExprDesc::Array { elem: Box::new(elem), len },
                            array_start.span(end),
                        )
                        .into(),
                    ),
                },
            };
            return Ok(Spanned::new(desc, start.span(end)));
        }

        let pointee = Box::new(self.parse_any_type_expr()?);
        let end = self.get_end();
        Ok(Spanned::new(
            if is_ptr {
                AstTypeExprDesc::Pointer { mutable, pointee }
            } else {
                AstTypeExprDesc::Ref { mutable, pointee }
            },
            start.span(end),
        ))
    }

    /// Parses `[T]` or `[T; N]`, returning `T` and `N` if present.
    fn parse_bracketed_type(
        &mut self,
    ) -> Result<(AstAnyTypeExpr, Option<usize>), ParseError> {
        self.eat(TokenKind::OpenSqr)?;
        let elem = self.parse_any_type_expr()?;
        let len = if self.peek_n(0).is_some_and(|t| t.kind == TokenKind::Semicolon) {
            self.consume();
            Some(self.parse_int_lit()?.data)
        } else {
            None
        };
        self.eat(TokenKind::CloseSqr)?;
        Ok((elem, len))
    }

    pub(super) fn parse_any_type_expr(&mut self) -> Result<AstAnyTypeExpr, ParseError> {
        let start = self.get_start();

        if let TokenKind::Identifier(name) = self.current_token()?.kind
            && name.is(self.db, "_")
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
