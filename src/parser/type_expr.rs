use crate::{
    common::symbols::Symbol,
    lexer::TokenKind,
    parse_tree::{
        Spanned,
        type_expr::{AnyTypeExpr, AnyTypeExprDesc, TypeExpr, TypeExprDesc},
    },
    parser::{ParseError, ParseErrorKind, Parser},
};

impl<'db> Parser<'db> {
    pub(super) fn parse_type_expr(&mut self) -> Result<TypeExpr, ParseError> {
        let start = self.get_start();

        match self.current_token()?.kind.clone() {
            TokenKind::Mult => {
                self.consume();
                let inner = self.parse_type_expr()?;
                let end = self.get_end();
                Ok(Spanned::new(
                    TypeExprDesc::Pointer(Box::new(inner)),
                    vec![],
                    start.span(&end),
                ))
            }

            TokenKind::OpenSqr => {
                self.consume();
                let ty = self.parse_type_expr()?;
                let len = if self.peek_n(0).map(|t| t.kind.clone()) == Some(TokenKind::Semicolon) {
                    self.consume();
                    Some(self.parse_int_lit()?.data)
                } else {
                    None
                };
                self.expect(TokenKind::CloseSqr)?;
                self.consume();
                let end = self.get_end();
                Ok(Spanned::new(
                    TypeExprDesc::Slice {
                        ty: Box::new(ty),
                        len,
                    },
                    vec![],
                    start.span(&end),
                ))
            }

            TokenKind::Identifier(name) => {
                self.consume();

                match self.peek_n(0).map(|t| t.kind.clone()) {
                    Some(TokenKind::Access) => {
                        self.consume();
                        let rhs = self.parse_type_expr()?;
                        let end = self.get_end();
                        Ok(Spanned::new(
                            TypeExprDesc::NameResolved {
                                from: name,
                                to: Box::new(rhs.data),
                            },
                            vec![],
                            start.span(&end),
                        ))
                    }

                    Some(TokenKind::Lt) => {
                        self.consume();
                        let args = self.parse_any_type_args()?;
                        self.expect(TokenKind::Gt)?;
                        self.consume();
                        let end = self.get_end();
                        Ok(Spanned::new(
                            TypeExprDesc::Named { name, args },
                            vec![],
                            start.span(&end),
                        ))
                    }

                    _ => {
                        let end = self.get_end();
                        Ok(Spanned::new(
                            TypeExprDesc::Named { name, args: vec![] },
                            vec![],
                            start.span(&end),
                        ))
                    }
                }
            }

            kind => Err(self.parse_error(ParseErrorKind::ExpectedSymbol(
                kind.display(self.db).to_string(),
            ))),
        }
    }

    pub(super) fn parse_any_type_expr(&mut self) -> Result<AnyTypeExpr, ParseError> {
        let start = self.get_start();

        if let TokenKind::Identifier(name) = self.current_token()?.kind {
            if name == Symbol::new(self.db, "_") {
                self.consume();
                let end = self.get_end();
                return Ok(Spanned::new(AnyTypeExprDesc::Any, vec![], start.span(&end)));
            }
        }

        let ty = self.parse_type_expr()?;
        let span = ty.span.clone();
        Ok(Spanned::new(AnyTypeExprDesc::Known(ty.data), vec![], span))
    }

    fn parse_any_type_args(&mut self) -> Result<Vec<AnyTypeExpr>, ParseError> {
        let mut args = vec![];

        while let Some(t) = self.peek_n(0) {
            if matches!(t.kind, TokenKind::Gt) {
                break;
            }
            args.push(self.parse_any_type_expr()?);
            match self.peek_n(0) {
                Some(t) if matches!(t.kind, TokenKind::Comma) => {
                    self.consume();
                }
                _ => break,
            }
        }

        Ok(args)
    }
}
