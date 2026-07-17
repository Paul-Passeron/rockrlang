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

#![allow(dead_code)]
#![allow(unused_variables)]

use core::fmt;
use regex::Regex;

use crate::{
    Db, SourceFile,
    common::location::{Location, Span},
    lexer::{
        Token,
        rules::{get_skip_rules, get_token_rules},
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LexError {
    pub message: String,
    pub file: SourceFile,
    pub offset: usize,
}

pub type TokenPattern<'db, T> =
    (Regex, fn(db: &'db dyn crate::Db, &str, Span) -> Result<T, LexError>);

pub struct Lexer<'db, T> {
    pub db: &'db dyn crate::Db,
    pub file: SourceFile,
    pub offset: usize,
    pub token_patterns: Vec<TokenPattern<'db, T>>,
    pub skip_patterns: Vec<Regex>,
}

impl<'a, T: fmt::Debug> Iterator for Lexer<'a, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_token().ok()
    }
}

impl<'db, T> Lexer<'db, T> {
    fn advance(&mut self) -> Option<char> {
        if !self.is_done() {
            let c = self.file.content(self.db)[self.offset..].chars().next()?;
            self.offset += 1;
            Some(c)
        } else {
            None
        }
    }

    fn advance_n(&mut self, n: usize) {
        for _ in 0..n {
            self.advance();
        }
    }

    pub fn is_done(&mut self) -> bool {
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
            self.advance_n(skip_count);
        }
    }

    pub fn new_blank(
        source: SourceFile,
        db: &'db dyn Db,
        token_patterns: Vec<TokenPattern<'db, T>>,
        skip_patterns: Vec<Regex>,
    ) -> Lexer<'db, T> {
        let contents = source.content(db);
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
                    let mut end = self.offset;
                    for _ in 0..token_length {
                        end += 1;
                    }
                    let span = Span::new(self.file, start, end);
                    res = Some(func(self.db, token_string, span));
                    break;
                }
            }
            self.advance_n(token_length);
            res.unwrap_or_else(|| {
                Err(LexError {
                    message: format!(
                        "Unexpected character `{}`",
                        contents.chars().nth(self.offset).unwrap_or('\0')
                    ),
                    file: self.file,
                    offset: self.offset,
                })
            })
        }
    }
}

impl<'db> Lexer<'db, Token> {
    pub fn new(db: &'db dyn crate::Db, source: SourceFile) -> Self {
        Self::new_blank(source, db, get_token_rules(), get_skip_rules())
    }
}
