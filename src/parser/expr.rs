use crate::{
    common::symbols::Symbol,
    lexer::TokenKind,
    parse_tree::{
        Spanned,
        expr::{BinaryOperator, CompoundAssignOp, Expr, ExprDesc, StructField},
        type_expr::{AnyTypeExpr, AnyTypeExprDesc, TypeExpr, TypeExprDesc},
    },
    parser::{ParseError, ParseErrorKind, Parser},
};

// ---------------------------------------------------------------------------
// Precedence table (precedence climbing)
// ---------------------------------------------------------------------------
//
// Level  Assoc    Operators
//   1    right    =  +=  -=  (assignment — lowest)
//   2    left     ..       (range)
//   3    left     ||
//   4    left     &&
//   5    left     |
//   6    left     ^
//   7    left     &
//   8    left     ==  !=
//   9    left     <  <=  >  >=
//  10    left     +  -
//  11    left     *  /  %
//
// Then unary prefix (highest among operators): * & - !
// Then postfix / atom (highest): @ $ . -> [] ()

#[derive(Clone, Copy, PartialEq, Eq)]
enum Assoc {
    Left,
    Right,
}

fn infix_binding_power(kind: &TokenKind) -> Option<(u8, u8, Assoc)> {
    // Returns (left_bp, right_bp, assoc)
    // For left-assoc:  left_bp == right_bp, and we use > for left recursion stop
    // For right-assoc: right_bp == left_bp - 1 so right side gets lower threshold
    let (prec, assoc) = match kind {
        // Assignment — right-associative, lowest precedence
        TokenKind::EqEq => (1, Assoc::Right),
        TokenKind::DotDot => (2, Assoc::Left),
        TokenKind::Or => (3, Assoc::Left),
        TokenKind::And => (4, Assoc::Left),
        TokenKind::BitOr => (5, Assoc::Left),
        TokenKind::BitXor => (6, Assoc::Left),
        TokenKind::BitAnd => (7, Assoc::Left),
        TokenKind::Diff => (8, Assoc::Left),
        TokenKind::Lt | TokenKind::Leq | TokenKind::Gt | TokenKind::Geq => (9, Assoc::Left),
        TokenKind::Plus | TokenKind::Minus => (10, Assoc::Left),
        TokenKind::Mult | TokenKind::Div | TokenKind::Modulo => (11, Assoc::Left),
        _ => return None,
    };
    let (lbp, rbp) = match assoc {
        Assoc::Left => (prec * 2, prec * 2),
        Assoc::Right => (prec * 2, prec * 2 - 1),
    };
    Some((lbp, rbp, assoc))
}

fn token_to_binop(kind: &TokenKind) -> Option<BinaryOperator> {
    match kind {
        TokenKind::Plus => Some(BinaryOperator::Plus),
        TokenKind::Minus => Some(BinaryOperator::Minus),
        TokenKind::Mult => Some(BinaryOperator::Times),
        TokenKind::Div => Some(BinaryOperator::Div),
        TokenKind::Modulo => Some(BinaryOperator::Modulo),
        TokenKind::EqEq => None, // handled as Assign
        TokenKind::Diff => Some(BinaryOperator::Diff),
        TokenKind::Lt => Some(BinaryOperator::Lt),
        TokenKind::Leq => Some(BinaryOperator::Leq),
        TokenKind::Gt => Some(BinaryOperator::Gt),
        TokenKind::Geq => Some(BinaryOperator::Geq),
        TokenKind::And => Some(BinaryOperator::And),
        TokenKind::Or => Some(BinaryOperator::Or),
        TokenKind::BitAnd => Some(BinaryOperator::BitAnd),
        TokenKind::BitOr => Some(BinaryOperator::BitOr),
        TokenKind::BitXor => Some(BinaryOperator::BitXor),
        _ => None,
    }
}

impl<'db> Parser<'db> {
    pub(super) fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_binop(0)
    }

    fn parse_binop(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_unary()?;

        loop {
            let tok_kind = match self.peek_n(0) {
                Some(t) => t.kind.clone(),
                None => break,
            };

            if matches!(tok_kind, TokenKind::Plus | TokenKind::Minus) {
                if let Some(next) = self.peek_n(1) {
                    if next.kind == TokenKind::EqEq {
                        let ca_lbp: u8 = 2;
                        let ca_rbp: u8 = 1;
                        if ca_lbp <= min_bp {
                            break;
                        }
                        let op = match tok_kind {
                            TokenKind::Plus => CompoundAssignOp::Plus,
                            TokenKind::Minus => CompoundAssignOp::Minus,
                            _ => unreachable!(),
                        };
                        self.consume();
                        self.consume();
                        let rhs = self.parse_binop(ca_rbp)?;
                        let span = lhs.span.start().span(&self.get_end());
                        lhs = Spanned::new(
                            ExprDesc::CompoundAssign {
                                lhs: Box::new(lhs),
                                op,
                                rhs: Box::new(rhs),
                            },
                            span,
                        );
                        continue;
                    }
                }
            }

            if tok_kind == TokenKind::EqEq {
                let lbp: u8 = 2;
                let rbp: u8 = 1;
                if lbp <= min_bp {
                    break;
                }
                self.consume(); // consume =
                let rhs = self.parse_binop(rbp)?;
                let span = lhs.span.start().span(&self.get_end());
                lhs = Spanned::new(
                    ExprDesc::Assign {
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    span,
                );
                continue;
            }

            if tok_kind == TokenKind::DotDot {
                let lbp: u8 = 4;
                let rbp: u8 = 4;
                if lbp <= min_bp {
                    break;
                }
                self.consume();
                let rhs = self.parse_binop(rbp)?;
                let span = lhs.span.start().span(&self.get_end());
                lhs = Spanned::new(
                    ExprDesc::Range {
                        from: Box::new(lhs),
                        to: Box::new(rhs),
                    },
                    span,
                );
                continue;
            }

            if let Some((lbp, rbp, _)) = infix_binding_power(&tok_kind) {
                if lbp <= min_bp {
                    break;
                }
                let op_kind = tok_kind.clone();
                self.consume();

                if let Some(op) = token_to_binop(&op_kind) {
                    let rhs = self.parse_binop(rbp)?;
                    let span = lhs.span.start().span(&self.get_end());
                    lhs = Spanned::new(
                        ExprDesc::BinOp {
                            lhs: Box::new(lhs),
                            op,
                            rhs: Box::new(rhs),
                        },
                        span,
                    );
                }
                continue;
            }

            break;
        }

        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        let start = self.get_start();
        match self.current_token()?.kind.clone() {
            TokenKind::Minus => {
                self.consume();
                let operand = self.parse_unary()?;
                let end = self.get_end();
                Ok(Spanned::new(
                    ExprDesc::Neg(Box::new(operand)),
                    start.span(&end),
                ))
            }
            TokenKind::Not => {
                self.consume();
                let operand = self.parse_unary()?;
                let end = self.get_end();
                Ok(Spanned::new(
                    ExprDesc::Not(Box::new(operand)),
                    start.span(&end),
                ))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek_n(0).map(|t| t.kind.clone()) {
                Some(TokenKind::AddressOf) => {
                    self.consume();
                    let end = self.get_end();
                    let span = expr.span.start().span(&end);
                    expr = Spanned::new(ExprDesc::AddressOf(Box::new(expr)), span);
                }

                Some(TokenKind::Deref) => {
                    self.consume();
                    let end = self.get_end();
                    let span = expr.span.start().span(&end);
                    expr = Spanned::new(ExprDesc::PostfixDeref(Box::new(expr)), span);
                }

                Some(TokenKind::Dot) => {
                    self.consume();
                    let field = self.parse_symbol()?;
                    if self.peek_n(0).map(|t| &t.kind) == Some(&TokenKind::OpenPar) {
                        self.consume();
                        let args = self.parse_expr_args()?;
                        self.expect(TokenKind::ClosePar)?;
                        self.consume();
                        let end = self.get_end();
                        let span = expr.span.start().span(&end);
                        expr = Spanned::new(
                            ExprDesc::MethodCall {
                                object: Box::new(expr),
                                method: field.data,
                                args,
                            },
                            span,
                        );
                    } else {
                        let end = self.get_end();
                        let span = expr.span.start().span(&end);
                        expr = Spanned::new(
                            ExprDesc::FieldAccess {
                                object: Box::new(expr),
                                field: field.data,
                            },
                            span,
                        );
                    }
                }

                Some(TokenKind::SmallArrow) => {
                    self.consume();
                    let field = self.parse_symbol()?;
                    let end = self.get_end();
                    let span = expr.span.start().span(&end);
                    expr = Spanned::new(
                        ExprDesc::ArrowAccess {
                            object: Box::new(expr),
                            field: field.data,
                        },
                        span,
                    );
                }

                Some(TokenKind::OpenSqr) => {
                    self.consume();
                    let index = self.parse_expr()?;
                    self.expect(TokenKind::CloseSqr)?;
                    self.consume();
                    let end = self.get_end();
                    let span = expr.span.start().span(&end);
                    expr = Spanned::new(
                        ExprDesc::Index {
                            object: Box::new(expr),
                            index: Box::new(index),
                        },
                        span,
                    );
                }

                Some(TokenKind::OpenPar) => {
                    self.consume();
                    let args = self.parse_expr_args()?;
                    self.expect(TokenKind::ClosePar)?;
                    self.consume();
                    let end = self.get_end();
                    let span = expr.span.start().span(&end);
                    expr = Spanned::new(
                        ExprDesc::Call {
                            callee: Box::new(expr),
                            args,
                        },
                        span,
                    );
                }

                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let start = self.get_start();
        let tok = self.current_token()?.clone();

        match tok.kind {
            TokenKind::IntLit(v) => {
                self.consume();
                Ok(Spanned::new(ExprDesc::IntLit(v), tok.location))
            }
            TokenKind::CharLit(c) => {
                self.consume();
                Ok(Spanned::new(ExprDesc::CharLit(c), tok.location))
            }
            TokenKind::StrLit(s) => {
                self.consume();
                Ok(Spanned::new(ExprDesc::StrLit(s), tok.location))
            }
            TokenKind::True => {
                self.consume();
                Ok(Spanned::new(ExprDesc::BoolLit(true), tok.location))
            }
            TokenKind::False => {
                self.consume();
                Ok(Spanned::new(ExprDesc::BoolLit(false), tok.location))
            }
            TokenKind::OpenPar => {
                self.consume();
                let inner = self.parse_expr()?;
                self.expect(TokenKind::ClosePar)?;
                self.consume();
                let end = self.get_end();
                Ok(Spanned::new(
                    ExprDesc::Paren(Box::new(inner)),
                    start.span(&end),
                ))
            }

            TokenKind::Identifier(name) => {
                self.consume();

                match self.peek_n(0).map(|t| t.kind.clone()) {
                    // `Name::…` — could be name resolution OR static method call
                    Some(TokenKind::Access) => {
                        self.consume();
                        let rhs = self.parse_postfix()?;
                        let end = self.get_end();
                        Ok(Spanned::new(
                            ExprDesc::NameResolved {
                                from: name,
                                to: Box::new(rhs),
                            },
                            start.span(&end),
                        ))
                    }

                    // `Name<T, U>::method(args)` — static call on a generic type
                    Some(TokenKind::Lt) => {
                        // We need to disambiguate `a < b` from `Name<T>::…`.
                        // Heuristic: if after parsing a type-arg list we see `>`,
                        // followed by `::`, treat this as a static call.
                        if let Some(static_call) =
                            self.try_parse_static_call(name, start.clone())?
                        {
                            Ok(static_call)
                        } else {
                            let end = self.get_end();
                            Ok(Spanned::new(ExprDesc::Name(name), start.span(&end)))
                        }
                    }

                    // `Name { .field = expr, … }` — struct literal
                    Some(TokenKind::OpenBra) => {
                        // Only interpret as struct literal if the first token inside
                        // is a '.' (field initialiser). Otherwise it's a block and
                        // we return the bare name.
                        if self.peek_n(1).map(|t| &t.kind) == Some(&TokenKind::Dot) {
                            self.consume(); // consume '{'
                            let fields = self.parse_struct_fields()?;
                            self.expect(TokenKind::CloseBra)?;
                            self.consume();
                            let end = self.get_end();
                            // Build a plain Named TypeExpr for the struct name
                            let ty_span = start.span(&start);
                            let ty =
                                Spanned::new(TypeExprDesc::Named { name, args: vec![] }, ty_span);
                            Ok(Spanned::new(
                                ExprDesc::StructLit { ty, fields },
                                start.span(&end),
                            ))
                        } else {
                            let end = self.get_end();
                            Ok(Spanned::new(ExprDesc::Name(name), start.span(&end)))
                        }
                    }

                    _ => {
                        let end = self.get_end();
                        Ok(Spanned::new(ExprDesc::Name(name), start.span(&end)))
                    }
                }
            }

            kind => Err(self.parse_error(ParseErrorKind::ExpectedSymbol(
                kind.display(self.db).to_string(),
            ))),
        }
    }

    // -----------------------------------------------------------------------
    // Static call on a generic type:  Name<T, U>::method(args)
    // -----------------------------------------------------------------------
    //
    // Returns Ok(Some(expr)) if successfully parsed, Ok(None) to signal the
    // caller should fall back to treating '<' as a less-than operator.

    fn try_parse_static_call(
        &mut self,
        type_name: Symbol,
        start: crate::common::location::Location,
    ) -> Result<Option<Expr>, ParseError> {
        // Save position so we can backtrack on failure.
        let saved_pos = self.position;

        // We are sitting on `<`. Consume it and try to parse type args.
        self.consume(); // consume '<'

        let mut type_args: Vec<AnyTypeExpr> = vec![];

        // Parse comma-separated AnyTypeExprs until we hit `>`
        let type_args_result: Result<(), ParseError> = (|| {
            loop {
                match self.peek_n(0).map(|t| t.kind.clone()) {
                    Some(TokenKind::Gt) => break,
                    None => return Err(self.parse_error(ParseErrorKind::UnexpectedEOF)),
                    _ => {}
                }
                // Wildcard `_`
                let arg_start = self.get_start();
                if let TokenKind::Identifier(sym) = self.current_token()?.kind {
                    if sym == Symbol::new(self.db, "_") {
                        self.consume();
                        let end = self.get_end();
                        type_args.push(Spanned::new(AnyTypeExprDesc::Any, arg_start.span(&end)));
                    } else {
                        let ty = self.parse_type_expr()?;
                        let span = ty.span.clone();
                        type_args.push(Spanned::new(AnyTypeExprDesc::Known(ty.data), span));
                    }
                } else {
                    let ty = self.parse_type_expr()?;
                    let span = ty.span.clone();
                    type_args.push(Spanned::new(AnyTypeExprDesc::Known(ty.data), span));
                }
                match self.peek_n(0).map(|t| t.kind.clone()) {
                    Some(TokenKind::Comma) => {
                        self.consume();
                    }
                    _ => break,
                }
            }
            Ok(())
        })();

        if type_args_result.is_err() || self.peek_n(0).map(|t| &t.kind) != Some(&TokenKind::Gt) {
            // Backtrack — this wasn't a type-arg list
            self.position = saved_pos;
            return Ok(None);
        }

        self.consume(); // consume '>'

        // Must be followed by '::'
        if self.peek_n(0).map(|t| &t.kind) != Some(&TokenKind::Access) {
            self.position = saved_pos;
            return Ok(None);
        }
        self.consume(); // consume '::'

        // Build the TypeExpr for the generic type
        let ty_span = start.span(&self.get_end());
        let ty: TypeExpr = Spanned::new(
            TypeExprDesc::Named {
                name: type_name,
                args: type_args,
            },
            ty_span,
        );

        // Now parse the method name
        let method_sym = self.parse_symbol()?;

        // Must be a call
        self.expect(TokenKind::OpenPar)?;
        self.consume(); // consume '('
        let args = self.parse_expr_args()?;
        self.expect(TokenKind::ClosePar)?;
        self.consume();

        let end = self.get_end();
        Ok(Some(Spanned::new(
            ExprDesc::StaticCall {
                ty,
                method: method_sym.data,
                args,
            },
            start.span(&end),
        )))
    }

    // -----------------------------------------------------------------------
    // Struct literal field list: .name = expr { , .name = expr }
    // -----------------------------------------------------------------------

    fn parse_struct_fields(&mut self) -> Result<Vec<StructField>, ParseError> {
        let mut fields = vec![];
        loop {
            match self.peek_n(0).map(|t| t.kind.clone()) {
                Some(TokenKind::CloseBra) | None => break,
                Some(TokenKind::Dot) => {
                    self.consume(); // consume '.'
                    let name = self.parse_symbol()?;
                    self.expect(TokenKind::EqEq)?;
                    self.consume(); // consume '='
                    let value = self.parse_expr()?;
                    fields.push(StructField {
                        name: name.data,
                        value,
                    });
                    match self.peek_n(0).map(|t| t.kind.clone()) {
                        Some(TokenKind::Comma) => {
                            self.consume();
                        }
                        _ => break,
                    }
                }
                _ => break,
            }
        }
        Ok(fields)
    }

    // -----------------------------------------------------------------------
    // Comma-separated expression list (for calls)
    // -----------------------------------------------------------------------

    fn parse_expr_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = vec![];
        loop {
            match self.peek_n(0).map(|t| t.kind.clone()) {
                Some(TokenKind::ClosePar) | None => break,
                _ => {
                    args.push(self.parse_expr()?);
                    match self.peek_n(0).map(|t| t.kind.clone()) {
                        Some(TokenKind::Comma) => {
                            self.consume();
                        }
                        _ => break,
                    }
                }
            }
        }
        Ok(args)
    }
}
