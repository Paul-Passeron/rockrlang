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

mod expr;
mod pattern;
mod stmt;
mod top_level;
mod type_expr;

use salsa::Accumulator;

use crate::{
    Db, SourceFile,
    common::{
        location::{Location, Span},
        symbols::Symbol,
    },
    lexer::{LexError, Token, TokenKind, lex_file},
    parse_tree::{
        Spanned,
        top_level::{Ast, AstAnyTopLevelItemDesc, AstTopLevelItemDesc},
    },
};

pub struct Parser<'db> {
    pub file: SourceFile,
    pub position: usize,
    pub tokens: &'db [Token],
    pub db: &'db dyn Db,
    pub restrict_struct_lit: bool,
}

#[salsa::accumulator]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub file: SourceFile,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParseErrorKind {
    LexError(LexError),
    UnexpectedEOF,
    ExpectedSymbol(String),
    ExpectedToken { expected: TokenKind, found: TokenKind },
    ExpectedIntLit(TokenKind),
    ExpectedTypeName,
    NotATopLevelItem,
}

impl<'db> Parser<'db> {
    pub fn new(db: &'db dyn Db, tokens: &'db [Token], file: SourceFile) -> Self {
        Self {
            position: 0,
            tokens,
            db,
            file,
            restrict_struct_lit: false,
        }
    }

    pub(super) fn with_struct_lit_restriction<T>(
        &mut self,
        restricted: bool,
        f: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        let saved = self.restrict_struct_lit;
        self.restrict_struct_lit = restricted;
        let result = f(self);
        self.restrict_struct_lit = saved;
        result
    }

    pub fn last_span(&self) -> Span {
        let (start, end) = if self.tokens.is_empty() || self.position >= self.tokens.len()
        {
            (0, 0)
        } else {
            let span = &self.tokens[self.position].location;
            (span.start_offset, span.end_offset)
        };
        Span::new(self.file, start, end)
    }

    pub fn current_token(&self) -> Result<&Token, ParseError> {
        if self.position >= self.tokens.len() {
            let last_span = self.last_span();
            Err(ParseError {
                kind: ParseErrorKind::UnexpectedEOF,
                file: last_span.file,
                start: last_span.start_offset,
                end: last_span.end_offset,
            })
        } else {
            Ok(&self.tokens[self.position])
        }
    }

    fn get_last_token(&self) -> Option<&Token> {
        self.tokens.last()
    }

    fn get_start(&self) -> Location {
        Location::new(
            self.file,
            self.tokens
                .get(self.position)
                .or_else(|| self.get_last_token())
                .map_or(0, |tok| tok.location.start_offset),
        )
    }

    fn get_end(&self) -> Location {
        Location::new(
            self.file,
            self.tokens
                .get(self.position - 1)
                .or_else(|| self.get_last_token())
                .map_or(0, |tok| tok.location.end_offset),
        )
    }

    fn consume(&mut self) {
        self.position += 1;
    }

    fn parse_error(&self, kind: ParseErrorKind) -> ParseError {
        let s = self.last_span();
        // panic!("{}:{} {kind:?}", s.file.display(), s.start);
        ParseError { kind, file: s.file, start: s.start_offset, end: s.end_offset }
    }

    fn parse_symbol(&mut self) -> Result<Spanned<Symbol>, ParseError> {
        let current = self.current_token()?.clone();
        match current.kind {
            TokenKind::Identifier(s) => {
                self.consume();
                Ok(Spanned::new(s, current.location))
            }
            kind => Err(self.parse_error(ParseErrorKind::ExpectedSymbol(
                kind.display(self.db).to_string(),
            ))),
        }
    }

    pub fn expect(&mut self, kind: TokenKind) -> Result<(), ParseError> {
        if self.current_token()?.kind != kind {
            Err(self.parse_error(ParseErrorKind::ExpectedToken {
                expected: kind,
                found: self.current_token()?.kind,
            }))
        } else {
            Ok(())
        }
    }

    pub fn peek_n(&self, n: usize) -> Option<&Token> {
        self.tokens.get(self.position + n)
    }

    pub fn speculate<T>(
        &mut self,
        f: impl Fn(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        let saved = self.position;
        f(self).inspect_err(|_| self.position = saved)
    }

    pub fn parse_list<T>(
        &mut self,
        mut elem: impl FnMut(&mut Self) -> Result<T, ParseError>,
        sep: TokenKind,
        mut is_end: impl FnMut(&mut Self) -> bool,
    ) -> Result<Vec<T>, ParseError> {
        let mut res = vec![];
        while !is_end(self) {
            res.push(elem(self)?);
            if self.peek_n(0).is_none_or(|t| t.kind != sep) {
                break;
            }
            self.consume();
        }
        Ok(res)
    }

    fn synchronize(&mut self, is_sync_point: impl Fn(&TokenKind) -> bool) {
        let mut depth = 0;
        while let Some(t) = self.peek_n(0) {
            match &t.kind {
                _ if depth == 0 && is_sync_point(&t.kind) => break,
                TokenKind::OpenBra | TokenKind::OpenPar | TokenKind::OpenSqr => {
                    depth += 1
                }
                TokenKind::CloseBra | TokenKind::ClosePar | TokenKind::CloseSqr
                    if depth > 0 =>
                {
                    depth -= 1
                }
                _ => {}
            }
            self.consume();
        }
    }
}

#[salsa::tracked(returns(copy))]
pub fn parse_file<'db>(db: &'db dyn Db, file: SourceFile) -> Ast<'db> {
    let lex_res = lex_file(db, file);
    let tokens = match lex_res {
        Ok(tokens) => tokens,
        Err(lex_error) => {
            let offset = lex_error.offset;
            let error = ParseError {
                kind: ParseErrorKind::LexError(lex_error),
                file,
                start: offset,
                end: offset + 1,
            };
            error.accumulate(db);
            vec![]
        }
    };

    let mut parser = Parser::new(db, &tokens, file);

    let mut items = vec![];
    let mut includes = vec![];

    while parser.position < tokens.len() {
        let start = parser.get_start();
        let item = parser.parse_any_toplevel_item();
        match item {
            Ok(item) => match item.data {
                AstAnyTopLevelItemDesc::Include(include) => {
                    includes.push(include);
                }
                AstAnyTopLevelItemDesc::Item(x) => {
                    items.push(Spanned::new(*x, item.span));
                }
            },
            Err(err) => {
                err.clone().accumulate(db);
                parser.synchronize(Parser::is_top_level_sync_point);
                items.push(Spanned::new(
                    AstTopLevelItemDesc::Error(err),
                    start.span(parser.get_end()),
                ));
            }
        }
    }

    Ast::new(db, items, includes)
}
