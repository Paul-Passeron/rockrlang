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
    Db, RockrDb, SourceFile,
    common::symbols::Symbol,
    lexer::{Lexer, TokenKind},
};
use std::{path::PathBuf, sync::Arc};

type Lexed = Result<(TokenKind, usize, usize), String>;

fn lex_in(db: &RockrDb, src: &str) -> Vec<Lexed> {
    let file = SourceFile::new(db, PathBuf::from("test.rkr"), Arc::from(src));
    let mut lexer = Lexer::new(db as &dyn Db, file);
    let mut out = vec![];
    loop {
        match lexer.next_token() {
            Ok(tok) => {
                out.push(Ok((tok.kind, tok.span.start_offset, tok.span.end_offset)))
            }
            Err(e) if e.message == "EOF" => return out,
            Err(e) => {
                out.push(Err(e.message));
                return out;
            }
        }
    }
}

fn kinds(src: &str) -> Vec<TokenKind> {
    let db = RockrDb::new();
    lex_in(&db, src).into_iter().map(|r| r.unwrap().0).collect()
}

fn error(src: &str) -> String {
    let db = RockrDb::new();
    lex_in(&db, src).into_iter().find_map(Result::err).expect("expected a lex error")
}

#[test]
fn keywords_and_identifiers() {
    let db = RockrDb::new();
    let toks: Vec<_> =
        lex_in(&db, "let mut letter fun").into_iter().map(|r| r.unwrap().0).collect();
    assert_eq!(
        toks,
        vec![
            TokenKind::Let,
            TokenKind::Mut,
            TokenKind::Identifier(Symbol::new(&db as &dyn Db, "letter")),
            TokenKind::Fun,
        ]
    );
}

#[test]
fn longest_operators_win() {
    assert_eq!(
        kinds("... .. . :: : => == = -> -= - <= <"),
        vec![
            TokenKind::DotDotDot,
            TokenKind::DotDot,
            TokenKind::Dot,
            TokenKind::Access,
            TokenKind::Colon,
            TokenKind::BigArrow,
            TokenKind::EqEq,
            TokenKind::Eq,
            TokenKind::SmallArrow,
            TokenKind::MinusEq,
            TokenKind::Minus,
            TokenKind::Leq,
            TokenKind::Lt,
        ]
    );
}

#[test]
fn comments_and_whitespace_are_skipped() {
    assert_eq!(
        kinds("1 // two\n/* three\n */ 4"),
        vec![TokenKind::IntLit(1), TokenKind::IntLit(4)]
    );
}

#[test]
fn int_literals() {
    assert_eq!(
        kinds("0 42 9223372036854775807"),
        vec![TokenKind::IntLit(0), TokenKind::IntLit(42), TokenKind::IntLit(i64::MAX),]
    );
}

#[test]
fn int_literal_overflow_is_an_error() {
    assert!(error("9223372036854775808").contains("too large"));
}

#[test]
fn float_literals() {
    assert_eq!(
        kinds("1.5 0.25"),
        vec![
            TokenKind::FloatLit(1.5f64.to_bits()),
            TokenKind::FloatLit(0.25f64.to_bits()),
        ]
    );
}

#[test]
fn ranges_are_not_floats() {
    assert_eq!(
        kinds("0..10"),
        vec![TokenKind::IntLit(0), TokenKind::DotDot, TokenKind::IntLit(10)]
    );
}

#[test]
fn char_literals_and_escapes() {
    assert_eq!(
        kinds(r"'a' '\n' '\''"),
        vec![TokenKind::CharLit('a'), TokenKind::CharLit('\n'), TokenKind::CharLit('\''),]
    );
}

#[test]
fn bad_char_escape_is_an_error() {
    assert!(error(r"'\q'").contains("Invalid escape"));
}

#[test]
fn directives_and_hashed() {
    let db = RockrDb::new();
    let d: &dyn Db = &db;
    let toks: Vec<_> =
        lex_in(&db, "@include #foo @ #").into_iter().map(|r| r.unwrap().0).collect();
    assert_eq!(
        toks,
        vec![
            TokenKind::Directive(Symbol::new(d, "include")),
            TokenKind::Hashed(Symbol::new(d, "foo")),
            TokenKind::AddressOf,
            TokenKind::HashPound,
        ]
    );
}

#[test]
fn spans_are_byte_offsets_after_multibyte_text() {
    let db = RockrDb::new();
    let toks = lex_in(&db, "\"héllo\" x");
    let (_, start, end) = toks[1].clone().unwrap();
    assert_eq!((start, end), (9, 10));
    let (_, s0, e0) = toks[0].clone().unwrap();
    assert_eq!((s0, e0), (0, 8));
}

#[test]
fn unexpected_character_names_the_right_char() {
    let msg = error("\"héllo\" ¤");
    assert!(msg.contains('¤'), "{msg}");
}
