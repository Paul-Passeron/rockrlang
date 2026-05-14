#![allow(dead_code)]
#![allow(unused_variables)]

use core::fmt;
use std::{path::PathBuf, sync::Arc};

use regex::Regex;

use crate::{
    SourceFile,
    common::location::{Location, Span},
    lexer::{
        Token,
        rules::{get_skip_rules, get_token_rules},
    },
};

#[derive(Debug)]
pub struct LexError {
    pub message: String,
    pub file: PathBuf,
    pub offset: usize,
}

pub type TokenPattern<'db, T> = (
    Regex,
    fn(db: &'db dyn crate::Db, &str, Span) -> Result<T, LexError>,
);

pub struct Lexer<'db, T> {
    pub db: &'db dyn crate::Db,
    pub file: PathBuf,
    pub contents: Arc<String>,
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
            let c = self.contents[self.offset..].chars().next()?;
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
        self.offset >= self.contents.len()
    }

    fn skip(&mut self) {
        loop {
            if self.is_done() {
                break;
            }
            let mut skip_count = 0;
            let mut could_skip = false;
            for regex in &self.skip_patterns {
                let to_match: &str = &self.contents[self.offset..];
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
        source: SourceFile<'db>,
        db: &'db dyn crate::Db,
        token_patterns: Vec<TokenPattern<'db, T>>,
        skip_patterns: Vec<Regex>,
    ) -> Lexer<'db, T> {
        let contents = source.content(db);
        Self {
            db,
            offset: 0,
            contents,
            file: source.path(db),
            token_patterns,
            skip_patterns,
        }
    }

    pub fn loc(&self) -> Location {
        Location::new(self.offset, self.file.clone())
    }

    pub fn next_token(&mut self) -> Result<T, LexError> {
        self.skip();
        if self.is_done() {
            Err(LexError {
                message: String::from("EOF"),
                file: self.file.clone(),
                offset: self.offset,
            })
        } else {
            let mut token_length = 0;
            let contents = Arc::clone(&self.contents);
            let mut res: Result<T, LexError> = Err(LexError {
                message: format!(
                    "Unexpected character `{}`",
                    contents.chars().nth(self.offset).unwrap_or('\0')
                ),
                file: self.file.clone(),
                offset: self.offset,
            });
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
                    let span = Span::new(start, end, self.file.clone());
                    res = func(self.db, token_string, span);
                    break;
                }
            }
            self.advance_n(token_length);
            res
        }
    }
}

impl<'db> Lexer<'db, Token> {
    pub fn new(db: &'db dyn crate::Db, source: SourceFile<'db>) -> Self {
        Self::new_blank(source, db, get_token_rules(), get_skip_rules())
    }
}
