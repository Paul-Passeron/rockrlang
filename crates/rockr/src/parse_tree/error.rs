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

use rockr_base::SourceFile;

use crate::lexer::{LexError, TokenKind};

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
    ExpectedToken {
        expected: TokenKind,
        found: TokenKind,
    },
    ExpectedIntLit(TokenKind),
    ExpectedTypeName,
    NotATopLevelItem,
    /// `[T]` used other than as `&[T]` / `&mut [T]`, e.g. bare or as `*[T]`.
    SliceMustBeBehindRef,
}
