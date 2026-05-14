use super::LexError;
use super::Token;
use super::TokenKind;
use crate::common::location::Span;
use crate::common::symbols::StrLit;
use crate::common::symbols::Symbol;
use regex::Regex;

macro_rules! get_simple_rule {
    ($regex: literal, $kind: expr) => {
        (
            Regex::new($regex).unwrap(),
            |_, _: &str, location: Span| -> Result<Token, LexError> {
                Ok(super::Token {
                    location,
                    kind: $kind,
                })
            },
        )
    };
}

pub fn get_skip_rules() -> Vec<Regex> {
    vec![
        Regex::new(r"\n").unwrap(),
        Regex::new("[ \n\t\r]+").unwrap(),
        Regex::new(r"//[^\n]*").unwrap(),
        Regex::new(r"(?s)/\*.*?\*/").unwrap(),
    ]
}

pub fn get_token_rules<'db>() -> Vec<(
    Regex,
    fn(&'db dyn crate::Db, &str, Span) -> Result<Token, LexError>,
)> {
    vec![
        get_simple_rule!(r#"\$"#, TokenKind::Deref),
        get_simple_rule!(r#"::"#, TokenKind::Access),
        get_simple_rule!(r#"\.\."#, TokenKind::DotDot),
        get_simple_rule!(r#"\."#, TokenKind::Dot),
        get_simple_rule!(r#":"#, TokenKind::Colon),
        get_simple_rule!(r#","#, TokenKind::Comma),
        get_simple_rule!(r#"\("#, TokenKind::OpenPar),
        get_simple_rule!(r#"\)"#, TokenKind::ClosePar),
        get_simple_rule!(r#"\["#, TokenKind::OpenSqr),
        get_simple_rule!(r#"\]"#, TokenKind::CloseSqr),
        get_simple_rule!(r#"\{"#, TokenKind::OpenBra),
        get_simple_rule!(r#"\}"#, TokenKind::CloseBra),
        get_simple_rule!(r#"=>"#, TokenKind::BigArrow),
        get_simple_rule!(r#"->"#, TokenKind::SmallArrow),
        get_simple_rule!(r#"\*"#, TokenKind::Mult),
        get_simple_rule!(r#"/"#, TokenKind::Div),
        get_simple_rule!(r#"\+"#, TokenKind::Plus),
        get_simple_rule!("-", TokenKind::Minus),
        get_simple_rule!(";", TokenKind::Semicolon),
        get_simple_rule!("<=", TokenKind::Leq),
        get_simple_rule!(">=", TokenKind::Geq),
        get_simple_rule!(">", TokenKind::Gt),
        get_simple_rule!("<", TokenKind::Lt),
        get_simple_rule!("!=", TokenKind::Diff),
        get_simple_rule!("==", TokenKind::EqEq),
        get_simple_rule!("=", TokenKind::Eq),
        get_simple_rule!("%", TokenKind::Modulo),
        get_simple_rule!(r"\|\|", TokenKind::Or),
        get_simple_rule!(r"\|", TokenKind::BitOr),
        get_simple_rule!("\\^", TokenKind::BitXor),
        get_simple_rule!(r#"&&"#, TokenKind::And),
        get_simple_rule!("&", TokenKind::BitAnd),
        get_simple_rule!("\\.\\.", TokenKind::DotDot),
        get_simple_rule!("!", TokenKind::Not),
        (
            Regex::new("@[A-Za-z_][A-Za-z0-9_]*").unwrap(),
            |db: &'db dyn crate::Db, lexeme: &str, location: Span| {
                Ok(Token {
                    location,
                    kind: TokenKind::Directive(Symbol::new(db, String::from(&lexeme[1..]))),
                })
            },
        ),
        get_simple_rule!("@", TokenKind::AddressOf),
        (
            Regex::new("[A-Za-z_][A-Za-z0-9_]*").unwrap(),
            |db: &'db dyn crate::Db, lexeme: &str, location: Span| {
                Ok(Token {
                    location,
                    kind: match lexeme {
                        "let" => TokenKind::Let,
                        "mod" => TokenKind::Module,
                        "impl" => TokenKind::Impl,
                        "if" => TokenKind::If,
                        "else" => TokenKind::Else,
                        "type" => TokenKind::Type,
                        "while" => TokenKind::While,
                        "return" => TokenKind::Return,
                        "defer" => TokenKind::Defer,
                        "struct" => TokenKind::Struct,
                        "interface" => TokenKind::Interface,
                        "in" => TokenKind::In,
                        "for" => TokenKind::For,
                        "break" => TokenKind::Break,
                        "use" => TokenKind::Use,
                        "static" => TokenKind::Static,
                        "true" => TokenKind::True,
                        "false" => TokenKind::False,
                        _ => TokenKind::Identifier(Symbol::new(db, String::from(lexeme))),
                    },
                })
            },
        ),
        (
            Regex::new(r#""(\\.|[^"\\])*""#).unwrap(),
            |db: &'db dyn crate::Db, lexeme: &str, location: Span| {
                let len = lexeme.len();
                Ok(Token {
                    location,
                    kind: TokenKind::StrLit(StrLit::new(db, String::from(&lexeme[1..len - 1]))),
                })
            },
        ),
        (
            Regex::new(r#"'(\\.|[^'\\])*'"#).unwrap(),
            |_, lexeme: &str, location: Span| {
                let len = lexeme.len();
                Ok(Token {
                    location,
                    kind: TokenKind::CharLit(
                        rustc_literal_escaper::unescape_char(&lexeme[1..len - 1])
                            .map_err(|_| todo!())?,
                    ),
                })
            },
        ),
        (
            Regex::new("[0-9]+").unwrap(),
            |_, lexeme: &str, location: Span| {
                Ok(Token {
                    location,
                    kind: TokenKind::IntLit(i64::from_str_radix(lexeme, 10).unwrap() as i32),
                })
            },
        ),
    ]
}
