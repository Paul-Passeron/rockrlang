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

mod lexer_impl;
mod rules;
pub mod token;
pub use lexer_impl::*;
pub use token::{Token, TokenKind};

use crate::SourceFile;

pub fn lex_file<'db>(
    db: &'db dyn crate::Db,
    file: SourceFile<'db>,
) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer::new(db, file);
    let mut v = vec![];
    while !lexer.is_done() {
        match lexer.next_token() {
            Ok(token) => {
                v.push(token);
            }
            Err(x) if x.message == "EOF" => {
                break;
            }
            Err(x) => return Err(x),
        }
    }
    Ok(v)
}
