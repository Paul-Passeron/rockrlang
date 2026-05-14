use std::path::PathBuf;

mod expr;
mod pattern;
mod stmt;
mod top_level;
mod type_expr;

use salsa::Accumulator;

use crate::{
    SourceFile, SourceRoot,
    common::{
        location::{Location, Span},
        symbols::Symbol,
    },
    lexer::{LexError, Token, TokenKind, lex_file},
    parse_tree::{Spanned, annotation::Annotation, top_level::Ast},
};

pub struct Parser<'db> {
    pub file: PathBuf,
    pub position: usize,
    pub tokens: &'db [Token],
    pub db: &'db dyn crate::Db,
    pub annotations: Vec<Annotation>,
}

#[allow(dead_code)]
#[salsa::accumulator]
#[derive(Debug)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub file: PathBuf,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum ParseErrorKind {
    LexError(LexError),
    UnexpectedEOF,
    ExpectedSymbol(String),
    ExpectedToken {
        expected: TokenKind,
        found: TokenKind,
    },
    TopLevelLetDecl,
}

impl<'db> Parser<'db> {
    pub fn new(db: &'db dyn crate::Db, tokens: &'db [Token], file: PathBuf) -> Self {
        Self {
            position: 0,
            tokens,
            db,
            file,
            annotations: Vec::new(),
        }
    }

    pub fn annotations(&mut self) -> Vec<Annotation> {
        std::mem::take(&mut self.annotations)
    }

    pub fn last_span(&self) -> Span {
        let (start, end) = if self.tokens.len() == 0 || self.position >= self.tokens.len() {
            (0, 0)
        } else {
            let span = &self.tokens[self.position].location;
            (span.start, span.end)
        };
        Span {
            file: self.file.clone(),
            start,
            end,
        }
    }

    pub fn current_token(&self) -> Result<&Token, ParseError> {
        if self.position >= self.tokens.len() {
            let last_span = self.last_span();
            Err(ParseError {
                kind: ParseErrorKind::UnexpectedEOF,
                file: last_span.file,
                start: last_span.start,
                end: last_span.end,
            })
        } else {
            Ok(&self.tokens[self.position])
        }
    }

    fn get_start(&self) -> Location {
        if self.position >= self.tokens.len() {
            Location {
                offset: 0,
                file: self.file.clone(),
            }
        } else {
            let offset = self.tokens[self.position].location.start;
            Location {
                offset,
                file: self.file.clone(),
            }
        }
    }

    fn get_end(&self) -> Location {
        if self.position > self.tokens.len() || self.position == 0 {
            Location {
                offset: 0,
                file: self.file.clone(),
            }
        } else {
            let offset = self.tokens[self.position - 1].location.end;
            Location {
                offset,
                file: self.file.clone(),
            }
        }
    }

    fn consume(&mut self) {
        self.position += 1;
    }

    fn parse_error(&self, kind: ParseErrorKind) -> ParseError {
        let s = self.last_span();
        ParseError {
            kind,
            file: s.file,
            start: s.start,
            end: s.end,
        }
    }

    fn parse_symbol(&mut self) -> Result<Spanned<Symbol>, ParseError> {
        let current = self.current_token()?.clone();
        match current.kind {
            TokenKind::Identifier(s) => {
                self.consume();
                Ok(Spanned::new(s, vec![], current.location.clone()))
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
                found: self.current_token()?.kind.clone(),
            }))
        } else {
            Ok(())
        }
    }

    pub fn peek_n(&self, n: usize) -> Option<&Token> {
        self.tokens.get(self.position + n)
    }
}

#[salsa::tracked]
pub fn parse_file<'db>(
    db: &'db dyn crate::Db,
    root: SourceRoot,
    file: SourceFile<'db>,
) -> Ast<'db> {
    let lex_res = lex_file(db, root, file);
    let tokens = match lex_res {
        Ok(tokens) => tokens,
        Err(lex_error) => {
            let offset = lex_error.offset;
            let error = ParseError {
                kind: ParseErrorKind::LexError(lex_error),
                file: file.path(db),
                start: offset,
                end: offset + 1,
            };
            error.accumulate(db);
            vec![]
        }
    };

    let mut parser = Parser::new(db, &tokens, file.path(db));

    let mut items = vec![];

    while parser.position < tokens.len() {
        let item = parser.parse_any_toplevel_item();
        match item {
            Ok(item) => {
                items.push(item);
            }
            Err(parse_error) => {
                parse_error.accumulate(db);
                break;
            }
        }
    }

    Ast::new(db, items)
}
