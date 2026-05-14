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
