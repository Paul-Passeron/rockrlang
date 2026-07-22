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
        pattern::{
            AstConstructFields, AstNamedPattern, AstPattern, AstPatternDesc,
            StructFieldPattern,
        },
    },
    parser::{ParseError, ParseErrorKind, Parser},
};

impl<'db> Parser<'db> {
    pub(super) fn parse_pattern(&mut self) -> Result<AstPattern, ParseError> {
        let start = self.get_start();

        match self.current_token()?.kind {
            TokenKind::Mut => {
                self.consume();
                let name = self.parse_symbol()?;
                Ok(Spanned::new(
                    AstPatternDesc::Named(AstNamedPattern::Mut { name }),
                    vec![],
                    start.span(self.get_end()),
                ))
            }
            TokenKind::Identifier(name) if name == Symbol::new(self.db, "_") => {
                self.consume();
                let end = self.get_end();
                Ok(Spanned::new(AstPatternDesc::Any, vec![], start.span(end)))
            }

            TokenKind::OpenPar => {
                self.consume(); // consume '('
                let fields = self.parse_pattern_list(TokenKind::ClosePar)?;
                self.expect(TokenKind::ClosePar)?;
                self.consume();
                let end = self.get_end();
                Ok(Spanned::new(
                    AstPatternDesc::Named(AstNamedPattern::Tuple { fields }),
                    vec![],
                    start.span(end),
                ))
            }

            // Named: Constructor, A::B::Constructor, or Constructor(args)
            TokenKind::Identifier(_) => {
                let named = self.parse_named_pattern()?;
                let end = self.get_end();
                Ok(Spanned::new(AstPatternDesc::Named(named), vec![], start.span(end)))
            }

            TokenKind::IntLit(x) => {
                self.consume();
                let end = self.get_end();

                Ok(Spanned::new(AstPatternDesc::IntLiteral(x), vec![], start.span(end)))
            }

            kind => Err(self.parse_error(ParseErrorKind::ExpectedSymbol(
                kind.display(self.db).to_string(),
            ))),
        }
    }

    fn parse_named_pattern(&mut self) -> Result<AstNamedPattern, ParseError> {
        let name = self.parse_symbol()?;

        match self.peek_n(0).map(|t| t.kind) {
            Some(TokenKind::Access) => {
                self.consume();
                let rhs = self.parse_named_pattern()?;
                Ok(AstNamedPattern::NameResolved { from: name, to: Box::new(rhs) })
            }

            Some(TokenKind::OpenPar) => {
                self.consume();
                let args = AstConstructFields::TupleFields(
                    self.parse_pattern_list(TokenKind::ClosePar)?,
                );
                self.expect(TokenKind::ClosePar)?;
                self.consume();
                Ok(AstNamedPattern::Constructor { name: name.data, args })
            }

            Some(TokenKind::OpenBra) => {
                self.consume();
                let fields = self.parse_list(
                    Self::parse_struct_field_pattern,
                    TokenKind::Comma,
                    |p| p.peek_n(0).is_none_or(|t| t.kind == TokenKind::CloseBra),
                )?;
                self.expect(TokenKind::CloseBra)?;
                self.consume();

                let args = AstConstructFields::StructFields(fields);

                Ok(AstNamedPattern::Constructor { name: name.data, args })
            }

            _ => Ok(AstNamedPattern::Bare(name.data)),
        }
    }

    fn parse_struct_field_pattern(&mut self) -> Result<StructFieldPattern, ParseError> {
        let name = self.parse_symbol()?;
        if let Some(t) = self.peek_n(0)
            && matches!(t.kind, TokenKind::Colon)
        {
            self.consume();
            let associated = self.parse_pattern()?;
            Ok(StructFieldPattern::Rebind {
                name: name.data,
                name_span: name.span,
                pattern: associated,
            })
        } else {
            Ok(StructFieldPattern::Name(name.data, name.span))
        }
    }

    fn parse_pattern_list(
        &mut self,
        end_tok: TokenKind,
    ) -> Result<Vec<AstPattern>, ParseError> {
        self.parse_list(Self::parse_pattern, TokenKind::Comma, |p| {
            p.peek_n(0).is_none_or(|t| t.kind == end_tok)
        })
    }
}
