use crate::{
    common::symbols::Symbol,
    lexer::TokenKind,
    parse_tree::{
        Spanned,
        pattern::{NamedPattern, Pattern, PatternDesc},
    },
    parser::{ParseError, ParseErrorKind, Parser},
};

impl<'db> Parser<'db> {
    pub(super) fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let start = self.get_start();

        match self.current_token()?.kind.clone() {
            TokenKind::Identifier(name) if name == Symbol::new(self.db, "_") => {
                self.consume();
                let end = self.get_end();
                Ok(Spanned::new(PatternDesc::Any, vec![], start.span(&end)))
            }

            TokenKind::OpenPar => {
                self.consume(); // consume '('
                let fields = self.parse_pattern_list(TokenKind::ClosePar)?;
                self.expect(TokenKind::ClosePar)?;
                self.consume();
                let end = self.get_end();
                Ok(Spanned::new(
                    PatternDesc::Named(NamedPattern::Tuple { fields }),
                    vec![],
                    start.span(&end),
                ))
            }

            // Named: Constructor, A::B::Constructor, or Constructor(args)
            TokenKind::Identifier(_) => {
                let named = self.parse_named_pattern()?;
                let end = self.get_end();
                Ok(Spanned::new(
                    PatternDesc::Named(named),
                    vec![],
                    start.span(&end),
                ))
            }

            kind => Err(self.parse_error(ParseErrorKind::ExpectedSymbol(
                kind.display(self.db).to_string(),
            ))),
        }
    }

    fn parse_named_pattern(&mut self) -> Result<NamedPattern, ParseError> {
        let name = self.parse_symbol()?.data;

        match self.peek_n(0).map(|t| t.kind.clone()) {
            Some(TokenKind::Access) => {
                self.consume();
                let rhs = self.parse_named_pattern()?;
                Ok(NamedPattern::NameResolved {
                    from: name,
                    to: Box::new(rhs),
                })
            }

            Some(TokenKind::OpenPar) => {
                self.consume();
                let args = self.parse_pattern_list(TokenKind::ClosePar)?;
                self.expect(TokenKind::ClosePar)?;
                self.consume();
                Ok(NamedPattern::Constructor { name, args })
            }

            _ => Ok(NamedPattern::Constructor { name, args: vec![] }),
        }
    }

    fn parse_pattern_list(&mut self, end_tok: TokenKind) -> Result<Vec<Pattern>, ParseError> {
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
