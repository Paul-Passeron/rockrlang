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
    lexer::TokenKind,
    parse_tree::stmt::{AstMatchBranch, AstStmt, AstStmtDesc, CompoundAssignOp},
    parser::{ParseError, Parser},
};

impl<'db> Parser<'db> {
    pub(super) fn parse_block(&mut self) -> Result<Vec<AstStmt>, ParseError> {
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

    pub(super) fn parse_block_as_stmt(&mut self) -> Result<AstStmt, ParseError> {
        let start = self.get_end();
        let stmts = self.parse_block()?;
        let end = self.get_end();
        Ok(AstStmt::new(
            AstStmtDesc::Block { stmts },
            vec![],
            start.span(&end),
        ))
    }

    fn parse_let_decl(&mut self) -> Result<AstStmt, ParseError> {
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
        Ok(AstStmt::new(
            AstStmtDesc::LetDecl {
                pat,
                type_constraint,
                value,
            },
            vec![],
            start.span(&end),
        ))
    }

    fn parse_for_stmt(&mut self) -> Result<AstStmt, ParseError> {
        let start = self.get_start();
        self.expect(TokenKind::For)?;
        self.consume();
        let element = self.parse_pattern()?;
        self.expect(TokenKind::In)?;
        self.consume();
        let iterator = self.parse_expr()?;
        let body = self.parse_block_as_stmt()?;
        let end = self.get_end();
        Ok(AstStmt::new(
            AstStmtDesc::For {
                element,
                iterator,
                body: Box::new(body),
            },
            vec![],
            start.span(&end),
        ))
    }

    fn parse_return(&mut self) -> Result<AstStmt, ParseError> {
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
        Ok(AstStmt::new(
            AstStmtDesc::Return { value },
            vec![],
            start.span(&end),
        ))
    }

    fn parse_while_stmt(&mut self) -> Result<AstStmt, ParseError> {
        let start = self.get_start();
        self.expect(TokenKind::While)?;
        self.consume();
        let cond = self.parse_expr()?;
        let body = self.parse_block_as_stmt()?;
        let end = self.get_end();
        Ok(AstStmt::new(
            AstStmtDesc::While {
                cond,
                body: Box::new(body),
            },
            vec![],
            start.span(&end),
        ))
    }

    fn try_parse_assign(&mut self) -> Option<AstStmt> {
        let saved_pos = self.position;
        let start = self.get_start();

        // Parse the LHS as an expression — postfix/field/index chains are valid LHS
        let lhs = match self.parse_expr() {
            Ok(e) => e,
            Err(_) => {
                self.position = saved_pos;
                return None;
            }
        };

        // Match directly on the dedicated compound-assign tokens or plain `=`
        let compound_op: Option<CompoundAssignOp> = match self.peek_n(0).map(|t| t.kind) {
            Some(TokenKind::PlusEq) => Some(CompoundAssignOp::Plus),
            Some(TokenKind::MinusEq) => Some(CompoundAssignOp::Minus),
            Some(TokenKind::MultEq) => Some(CompoundAssignOp::Times),
            Some(TokenKind::DivEq) => Some(CompoundAssignOp::Div),
            Some(TokenKind::ModuloEq) => Some(CompoundAssignOp::Modulo),
            _ => None,
        };

        let is_plain_assign = self.peek_n(0).map(|t| t.kind) == Some(TokenKind::Eq);

        if compound_op.is_none() && !is_plain_assign {
            self.position = saved_pos;
            return None;
        }

        self.consume(); // consume = or +=/-=/*=//=/%=

        let rhs = match self.parse_expr() {
            Ok(e) => e,
            Err(_) => {
                self.position = saved_pos;
                return None;
            }
        };

        if self.expect(TokenKind::Semicolon).is_err() {
            self.position = saved_pos;
            return None;
        }
        self.consume();

        let end = self.get_end();
        let desc = if let Some(op) = compound_op {
            AstStmtDesc::CompoundAssign { lhs, op, rhs }
        } else {
            AstStmtDesc::Assign { lhs, rhs }
        };
        Some(AstStmt::new(desc, vec![], start.span(&end)))
    }

    fn parse_match_stmt(&mut self) -> Result<AstStmt, ParseError> {
        let start = self.get_start();
        self.expect(TokenKind::Match)?;
        self.consume();
        let scrutinee = self.parse_expr()?;
        self.expect(TokenKind::OpenBra)?;
        self.consume();
        let mut branches = Vec::new();
        while self
            .peek_n(0)
            .is_some_and(|t| !matches!(t.kind, TokenKind::CloseBra))
        {
            let pat = self.parse_pattern()?;
            let guard = if self
                .peek_n(0)
                .is_some_and(|t| matches!(t.kind, TokenKind::If))
            {
                self.consume();
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect(TokenKind::BigArrow)?;
            self.consume();
            let body = self.parse_block_as_stmt()?;
            branches.push(AstMatchBranch {
                pat,
                guard,
                body: Box::new(body),
            });
        }
        self.expect(TokenKind::CloseBra)?;
        self.consume();

        Ok(AstStmt::new(
            AstStmtDesc::Match {
                scrutinee,
                branches,
            },
            vec![],
            start.span(&self.get_end()),
        ))
    }

    fn parse_if_stmt(&mut self) -> Result<AstStmt, ParseError> {
        let start = self.get_start();
        self.expect(TokenKind::If)?;
        self.consume();
        let cond = self.parse_expr()?;
        let then = self.parse_block_as_stmt()?;
        let else_ = if let Some(t) = self.peek_n(0)
            && matches!(t.kind, TokenKind::Else)
        {
            self.consume();
            Some(self.parse_block_as_stmt()?)
        } else {
            None
        };
        let end = self.get_end();
        Ok(AstStmt::new(
            AstStmtDesc::If {
                cond,
                then: Box::new(then),
                else_: else_.map(Box::new),
            },
            vec![],
            start.span(&end),
        ))
    }

    pub(super) fn parse_stmt(&mut self) -> Result<AstStmt, ParseError> {
        match self.current_token()?.kind {
            TokenKind::OpenBra => self.parse_block_as_stmt(),
            TokenKind::Return => self.parse_return(),
            TokenKind::Let => self.parse_let_decl(),
            TokenKind::If => self.parse_if_stmt(),
            TokenKind::For => self.parse_for_stmt(),
            TokenKind::While => self.parse_while_stmt(),
            TokenKind::Match => self.parse_match_stmt(),
            TokenKind::Break => {
                let start = self.get_start();
                self.consume();
                self.expect(TokenKind::Semicolon)?;
                self.consume();
                let span = start.span(&self.get_end());
                Ok(AstStmt::new(AstStmtDesc::Break, vec![], span))
            }
            TokenKind::Defer => {
                let start = self.get_start();
                self.consume();
                let stmt = self.parse_stmt()?;
                let span = start.span(&stmt.span.end());
                Ok(AstStmt::new(
                    AstStmtDesc::Defer(Box::new(stmt)),
                    vec![],
                    span,
                ))
            }
            _ => {
                if let Some(assignement) = self.try_parse_assign() {
                    Ok(assignement)
                } else {
                    let expr = self.parse_expr()?;
                    self.expect(TokenKind::Semicolon)?;
                    self.consume();
                    let span = expr.span.clone();
                    Ok(AstStmt::new(AstStmtDesc::Expr(expr), vec![], span))
                }
            }
        }
    }
}
