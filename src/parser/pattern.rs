use crate::{
    common::symbols::Symbol,
    lexer::TokenKind,
    parse_tree::{
        Spanned,
        pattern::{
            AstConstructFields, AstNamedPattern, AstPattern, AstPatternDesc, StructFieldPattern,
        },
    },
    parser::{ParseError, ParseErrorKind, Parser},
};

impl<'db> Parser<'db> {
    pub(super) fn parse_pattern(&mut self) -> Result<AstPattern, ParseError> {
        let start = self.get_start();

        match self.current_token()?.kind.clone() {
            TokenKind::Mut => {
                self.consume();
                let name = self.parse_symbol()?.data;
                Ok(Spanned::new(
                    AstPatternDesc::Named(AstNamedPattern::Mut { name }),
                    vec![],
                    start.span(&self.get_end()),
                ))
            }
            TokenKind::Identifier(name) if name == Symbol::new(self.db, "_") => {
                self.consume();
                let end = self.get_end();
                Ok(Spanned::new(AstPatternDesc::Any, vec![], start.span(&end)))
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
                    start.span(&end),
                ))
            }

            // Named: Constructor, A::B::Constructor, or Constructor(args)
            TokenKind::Identifier(_) => {
                let named = self.parse_named_pattern()?;
                let end = self.get_end();
                Ok(Spanned::new(
                    AstPatternDesc::Named(named),
                    vec![],
                    start.span(&end),
                ))
            }

            kind => Err(self.parse_error(ParseErrorKind::ExpectedSymbol(
                kind.display(self.db).to_string(),
            ))),
        }
    }

    fn parse_named_pattern(&mut self) -> Result<AstNamedPattern, ParseError> {
        let name = self.parse_symbol()?.data;

        match self.peek_n(0).map(|t| t.kind.clone()) {
            Some(TokenKind::Access) => {
                self.consume();
                let rhs = self.parse_named_pattern()?;
                Ok(AstNamedPattern::NameResolved {
                    from: name,
                    to: Box::new(rhs),
                })
            }

            Some(TokenKind::OpenPar) => {
                self.consume();
                let args =
                    AstConstructFields::TupleFields(self.parse_pattern_list(TokenKind::ClosePar)?);
                self.expect(TokenKind::ClosePar)?;
                self.consume();
                Ok(AstNamedPattern::Constructor { name, args })
            }

            Some(TokenKind::OpenBra) => {
                self.consume();
                let mut fields = vec![];

                while let Some(t) = self.peek_n(0)
                    && !matches!(t.kind, TokenKind::CloseBra)
                {
                    let name = self.parse_symbol()?.data;
                    if let Some(t) = self.peek_n(0)
                        && matches!(t.kind, TokenKind::Colon)
                    {
                        self.consume();
                        let associated = self.parse_pattern()?;
                        fields.push(StructFieldPattern::Rebind {
                            name,
                            pattern: associated,
                        })
                    } else {
                        fields.push(StructFieldPattern::Name(name));
                    }
                    if let Some(t) = self.peek_n(0)
                        && matches!(t.kind, TokenKind::Comma)
                    {
                        self.consume();
                    } else {
                        break;
                    }
                }
                self.expect(TokenKind::CloseBra)?;
                self.consume();
                

                let args = AstConstructFields::StructFields(fields);

                Ok(AstNamedPattern::Constructor { name, args })
            }

            _ => Ok(AstNamedPattern::Constructor {
                name,
                args: AstConstructFields::None,
            }),
        }
    }

    fn parse_pattern_list(&mut self, end_tok: TokenKind) -> Result<Vec<AstPattern>, ParseError> {
        let mut pats = vec![];

        loop {
            match self.peek_n(0).map(|t| t.kind.clone()) {
                Some(ref k) if *k == end_tok => break,
                None => break,
                _ => {
                    pats.push(self.parse_pattern()?);
                    match self.peek_n(0).map(|t| t.kind.clone()) {
                        Some(TokenKind::Comma) => {
                            self.consume();
                        }
                        _ => break,
                    }
                }
            }
        }

        Ok(pats)
    }
}
