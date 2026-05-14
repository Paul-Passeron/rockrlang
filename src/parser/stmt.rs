use crate::{
    lexer::TokenKind,
    parse_tree::stmt::{Stmt, StmtDesc},
    parser::{ParseError, Parser},
};

impl<'db> Parser<'db> {
    pub(super) fn parse_block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        self.expect(TokenKind::OpenBra)?;
        self.consume();

        let mut res = vec![];

        while let Some(t) = self.peek_n(0)
            && !matches!(t.kind, TokenKind::CloseBra)
        {
            res.push(self.parse_stmt()?);
        }

        self.expect(TokenKind::CloseBra)?;
        self.consume();

        Ok(res)
    }

    pub(super) fn parse_block_as_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.get_end();
        let stmts = self.parse_block()?;
        let end = self.get_end();
        Ok(Stmt::new(StmtDesc::Block { stmts }, start.span(&end)))
    }

    fn parse_let_decl(&mut self) -> Result<Stmt, ParseError> {
        let start = self.get_start();
        self.expect(TokenKind::Let)?;
        self.consume();
        let pat = self.parse_pattern()?;
        let type_constraint = if let Some(t) = self.peek_n(0)
            && matches!(t.kind, TokenKind::Colon)
        {
            self.consume();
            Some(self.parse_any_type_expr()?)
        } else {
            None
        };
        self.expect(TokenKind::Eq)?;
        self.consume();
        let value = self.parse_expr()?;
        self.expect(TokenKind::Semicolon)?;
        self.consume();
        let end = self.get_end();
        Ok(Stmt::new(
            StmtDesc::LetDecl {
                pat,
                type_constraint,
                value,
            },
            start.span(&end),
        ))
    }

    fn parse_for_stmt(&mut self) -> Result<Stmt, ParseError> {
        let start = self.get_start();
        self.expect(TokenKind::For)?;
        self.consume();
        let element = self.parse_pattern()?;
        self.expect(TokenKind::In)?;
        self.consume();
        let iterator = self.parse_expr()?;
        let body = self.parse_block_as_stmt()?;
        let end = self.get_end();
        Ok(Stmt::new(
            StmtDesc::For {
                element,
                iterator,
                body: Box::new(body),
            },
            start.span(&end),
        ))
    }

    fn parse_return(&mut self) -> Result<Stmt, ParseError> {
        let start = self.get_start();
        self.expect(TokenKind::Return)?;
        self.consume();
        let value = if let Some(t) = self.peek_n(0)
            && matches!(t.kind, TokenKind::Semicolon)
        {
            None
        } else {
            let value = self.parse_expr()?;
            Some(value)
        };
        self.expect(TokenKind::Semicolon)?;
        self.consume();
        let end = self.get_end();
        Ok(Stmt::new(StmtDesc::Return { value }, start.span(&end)))
    }

    pub(super) fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.current_token()?.kind {
            TokenKind::OpenBra => self.parse_block_as_stmt(),
            TokenKind::Return => self.parse_return(),
            TokenKind::Let => self.parse_let_decl(),
            TokenKind::If => todo!(),
            TokenKind::For => self.parse_for_stmt(),
            TokenKind::While => todo!(),
            _ => {
                let expr = self.parse_expr()?;
                self.expect(TokenKind::Semicolon)?;
                self.consume();
                let span = expr.span.clone();
                Ok(Stmt::new(StmtDesc::Expr(expr), span))
            }
        }
    }
}
