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

use crate::lexer::{
    Token,
    rules::{get_skip_rules, get_token_rules},
};
use core::fmt;
use regex::Regex;
use rockr_base::{Db, Location, SourceFile, Span};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LexError {
    pub message: String,
    pub file: SourceFile,
    pub offset: usize,
}

pub type TokenPattern<'db, T> =
    (Regex, fn(db: &'db dyn Db, &str, Span) -> Result<T, LexError>);

pub struct Lexer<'db, T> {
    pub db: &'db dyn Db,
    pub file: SourceFile,
    pub offset: usize,
    pub token_patterns: Vec<TokenPattern<'db, T>>,
    pub skip_patterns: Vec<Regex>,
}

impl<T: fmt::Debug> Iterator for Lexer<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_token().ok()
    }
}

impl<'db, T> Lexer<'db, T> {
    fn advance_n_bytes(&mut self, n: usize) {
        if self.is_done() {
            return;
        }
        let mut count = 0;
        while count < n {
            let Some(c) =
                self.file.content(self.db)[self.offset + count..].chars().next()
            else {
                return;
            };
            count += c.len_utf8();
        }
        self.offset += count;
    }

    pub fn is_done(&self) -> bool {
        self.offset >= self.file.content(self.db).len()
    }

    fn skip(&mut self) {
        loop {
            if self.is_done() {
                break;
            }
            let mut skip_count = 0;
            let mut could_skip = false;
            for regex in &self.skip_patterns {
                let to_match: &str = &self.file.content(self.db)[self.offset..];
                if let Some(mat) = regex.find(to_match)
                    && mat.start() == 0
                {
                    could_skip = true;
                    skip_count += mat.len();
                    break;
                }
            }
            if !could_skip {
                break;
            }
            self.advance_n_bytes(skip_count);
        }
    }

    pub fn new_blank(
        source: SourceFile,
        db: &'db dyn Db,
        token_patterns: Vec<TokenPattern<'db, T>>,
        skip_patterns: Vec<Regex>,
    ) -> Self {
        Self { db, offset: 0, file: source, token_patterns, skip_patterns }
    }

    pub fn loc(&self) -> Location {
        Location::new(self.file, self.offset)
    }

    pub fn next_token(&mut self) -> Result<T, LexError> {
        self.skip();
        if self.is_done() {
            Err(LexError {
                message: String::from("EOF"),
                file: self.file,
                offset: self.offset,
            })
        } else {
            let mut token_length = 0;
            let contents = self.file.content(self.db);
            let mut res: Option<Result<T, LexError>> = None;
            for i in 0..self.token_patterns.len() {
                let start = self.offset;
                let to_match = &contents[self.offset..];
                let (ref regex, func) = self.token_patterns[i];
                if let Some(mat) = regex.find(to_match)
                    && mat.start() == 0
                {
                    token_length = mat.end();
                    let token_string: &str = &to_match[..token_length];
                    let span = Span::new(self.file, start, start + token_length);
                    res = Some(func(self.db, token_string, span));
                    break;
                }
            }
            self.advance_n_bytes(token_length);
            res.unwrap_or_else(|| {
                Err(LexError {
                    message: format!(
                        "Unexpected character `{}`",
                        contents[self.offset..].chars().next().unwrap_or('\0')
                    ),
                    file: self.file,
                    offset: self.offset,
                })
            })
        }
    }
}

impl<'db> Lexer<'db, Token> {
    pub fn new(db: &'db dyn Db, source: SourceFile) -> Self {
        Self::new_blank(source, db, get_token_rules(), get_skip_rules())
    }
}
