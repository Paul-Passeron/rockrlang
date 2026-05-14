use nonempty::nonempty;

use crate::{
    common::symbols::Symbol,
    lexer::TokenKind,
    parse_tree::{
        annotation::{AstAnnotation, AstAnnotationArg, AstAnnotationItem},
        top_level::{
            AstAnyTopLevelItem, AstAnyTopLevelItemDesc, AstFundef, AstFundefArg, AstFundefDesc,
            AstFunsig, AstFunsigDesc, AstImplBlock, AstImplItem, AstIncludePath, AstInterface,
            AstInterfaceItem, AstMethodDef, AstMethodDefDesc, AstMethodsig, AstMethodsigDesc,
            AstModule, AstModuleDesc, AstReceiver, AstStructDef, AstStructDefField, AstTemplateArg,
            AstTopLevelItem, AstTopLevelItemDesc,
        },
    },
    parser::{ParseError, ParseErrorKind, Parser},
};

impl<'db> Parser<'db> {
    fn parse_include_path(&mut self) -> Result<AstIncludePath, ParseError> {
        let mut symbols = nonempty![self.parse_symbol()?];
        while self.current_token()?.kind == TokenKind::Access {
            self.consume();
            symbols.push(self.parse_symbol()?);
        }
        Ok(AstIncludePath::from(symbols))
    }

    pub(super) fn parse_any_toplevel_item(&mut self) -> Result<AstAnyTopLevelItem, ParseError> {
        let start = self.get_start();
        match &self.current_token()?.kind {
            TokenKind::Directive(dir) if *dir == Symbol::new(self.db, "include") => {
                self.consume();
                let include_path = self.parse_include_path()?;
                let end = self.get_end();
                Ok(AstAnyTopLevelItem::new(
                    AstAnyTopLevelItemDesc::Include(include_path),
                    vec![],
                    start.span(&end),
                ))
            }
            _ => self.parse_toplevel_item().map(|item| {
                AstAnyTopLevelItem::new(
                    AstAnyTopLevelItemDesc::Item(item.data),
                    item.annotations,
                    item.span,
                )
            }),
        }
    }

    pub fn parse_module(&mut self) -> Result<AstModule, ParseError> {
        let annotations = self.annotations();
        let start = self.get_start();
        self.expect(TokenKind::Module)?;
        self.consume();
        let name = self.parse_symbol()?.data;
        self.expect(TokenKind::OpenBra)?;
        self.consume();
        let mut items = vec![];
        let mut includes = vec![];
        while self.current_token()?.kind != TokenKind::CloseBra {
            if let TokenKind::Directive(s) = self.current_token()?.kind
                && s.interned().contents(self.db) == "include"
            {
                self.consume();
                let include = self.parse_include_path()?;
                includes.push(include);
            } else {
                let item = self.parse_toplevel_item()?;
                items.push(item);
            }
        }
        self.expect(TokenKind::CloseBra)?;
        self.consume();
        let end = self.get_end();
        Ok(AstModule::new(
            AstModuleDesc {
                name,
                items,
                includes,
            },
            annotations,
            start.span(&end),
        ))
    }

    fn parse_fundef_args(&mut self) -> Result<Vec<AstFundefArg>, ParseError> {
        let mut args = vec![];

        while let Some(t) = self.peek_n(0)
            && !matches!(t.kind, TokenKind::ClosePar)
        {
            args.push(self.parse_fundef_arg()?);
            if let Some(t) = self.peek_n(0)
                && matches!(t.kind, TokenKind::Comma)
            {
                self.consume();
            } else {
                break;
            }
        }

        Ok(args)
    }

    fn parse_fundef_arg(&mut self) -> Result<AstFundefArg, ParseError> {
        // TODO: handle patterns
        let name = self.parse_symbol()?;
        self.expect(TokenKind::Colon)?;
        self.consume();
        let ty = self.parse_type_expr()?;
        Ok(AstFundefArg {
            name: name.data,
            ty,
        })
    }

    fn parse_template_arg(&mut self) -> Result<AstTemplateArg, ParseError> {
        let name = self.parse_symbol()?.data;
        let constraints = if let Some(t) = self.peek_n(0)
            && matches!(t.kind, TokenKind::Colon)
        {
            self.consume();
            let mut cs = vec![self.parse_type_expr()?];
            while let Some(t) = self.peek_n(0)
                && matches!(t.kind, TokenKind::Plus)
            {
                self.consume();
                cs.push(self.parse_type_expr()?);
            }
            cs
        } else {
            vec![]
        };
        Ok(AstTemplateArg { name, constraints })
    }

    fn parse_template_args(&mut self) -> Result<Vec<AstTemplateArg>, ParseError> {
        let mut args = vec![];

        while let Some(t) = self.peek_n(0)
            && !matches!(t.kind, TokenKind::Gt)
        {
            args.push(self.parse_template_arg()?);
            if let Some(t) = self.peek_n(0)
                && matches!(t.kind, TokenKind::Comma)
            {
                self.consume();
            } else {
                break;
            }
        }

        Ok(args)
    }

    fn parse_receiver(&mut self) -> AstReceiver {
        let position = self.position;
        if let Some(r) = self.parse_receiver_aux() {
            r
        } else {
            self.position = position;
            AstReceiver::None
        }
    }

    fn expect_self(&mut self) -> Option<()> {
        if let Some(t) = self.peek_n(0)
            && matches!(t.kind, TokenKind::Identifier(symbol) if symbol == Symbol::new(self.db, "self"))
        {
            self.consume();
            Some(())
        } else {
            None
        }
    }

    fn parse_receiver_aux(&mut self) -> Option<AstReceiver> {
        if let Some(t) = self.peek_n(0) {
            match t.kind {
                TokenKind::Identifier(symbol) if symbol == Symbol::new(self.db, "self") => {
                    self.consume();
                    Some(AstReceiver::Zelf)
                }
                TokenKind::BitAnd => {
                    self.consume();
                    if let Some(t) = self.peek_n(0)
                        && matches!(t.kind, TokenKind::Mut)
                    {
                        self.consume();
                        self.expect_self()?;
                        Some(AstReceiver::MutRefZelf)
                    } else {
                        self.expect_self()?;
                        Some(AstReceiver::RefZelf)
                    }
                }
                TokenKind::Mult => {
                    self.consume();
                    if let Some(t) = self.peek_n(0)
                        && matches!(t.kind, TokenKind::Mut)
                    {
                        self.consume();
                        self.expect_self()?;
                        Some(AstReceiver::MutPtrZelf)
                    } else {
                        self.expect_self()?;
                        Some(AstReceiver::PtrZelf)
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    }

    fn parse_funsig(&mut self) -> Result<AstFunsig, ParseError> {
        Ok(self.parse_any_funsig(false)?.0)
    }

    fn parse_methodsig(&mut self) -> Result<AstMethodsig, ParseError> {
        let (
            AstFunsig {
                data:
                    AstFunsigDesc {
                        name,
                        args,
                        template_args,
                        return_type,
                    },
                annotations,
                span,
            },
            receiver,
        ) = self.parse_any_funsig(true)?;
        let receiver = receiver.unwrap();
        Ok(AstMethodsig::new(
            AstMethodsigDesc {
                name,
                receiver,
                args,
                template_args,
                return_type,
            },
            annotations,
            span,
        ))
    }

    fn parse_any_funsig(
        &mut self,
        accept_receiver: bool,
    ) -> Result<(AstFunsig, Option<AstReceiver>), ParseError> {
        let annotations = self.annotations();
        let start = self.get_start();
        let name = self.parse_symbol()?;

        let template_args = if let Some(t) = self.peek_n(0)
            && t.kind == TokenKind::Lt
        {
            self.consume();
            let templates = self.parse_template_args()?;
            self.expect(TokenKind::Gt)?;
            self.consume();
            templates
        } else {
            vec![]
        };

        self.expect(TokenKind::OpenPar)?;
        self.consume();

        let mut has_args = true;

        let receiver = if accept_receiver {
            let r = self.parse_receiver();
            if let Some(t) = self.peek_n(0)
                && matches!(t.kind, TokenKind::Comma)
            {
                self.consume();
            } else {
                if r != AstReceiver::None {
                    has_args = false;
                }
            }
            Some(r)
        } else {
            None
        };

        let args = if has_args {
            self.parse_fundef_args()?
        } else {
            vec![]
        };
        self.expect(TokenKind::ClosePar)?;
        self.consume();

        self.expect(TokenKind::Colon)?;
        self.consume();

        let return_type = self.parse_type_expr()?;

        let end = self.get_end();

        Ok((
            AstFunsig::new(
                AstFunsigDesc {
                    name: name.data,
                    args,
                    template_args,
                    return_type,
                },
                annotations,
                start.span(&end),
            ),
            receiver,
        ))
    }

    fn parse_methoddef(&mut self) -> Result<AstMethodDef, ParseError> {
        self.expect(TokenKind::Fun)?;
        self.consume();
        let AstMethodsig {
            data:
                AstMethodsigDesc {
                    name,
                    receiver,
                    args,
                    template_args,
                    return_type,
                },
            annotations,
            span,
        } = self.parse_methodsig()?;

        let body = self.parse_block()?;

        let span = span.start().span(&self.get_end());
        Ok(AstMethodDef::new(
            AstMethodDefDesc {
                name,
                receiver,
                args,
                template_args,
                return_type,
                body,
            },
            annotations,
            span,
        ))
    }

    fn parse_fundef(&mut self) -> Result<AstFundef, ParseError> {
        self.expect(TokenKind::Fun)?;
        self.consume();
        let AstFunsig {
            data:
                AstFunsigDesc {
                    name,
                    args,
                    template_args,
                    return_type,
                },
            annotations,
            span,
        } = self.parse_funsig()?;

        let body = self.parse_block()?;

        let span = span.start().span(&self.get_end());

        Ok(AstFundef::new(
            AstFundefDesc {
                name,
                args,
                template_args,
                return_type,
                body,
            },
            annotations,
            span,
        ))
    }

    pub fn parse_annotation(&mut self) -> Result<AstAnnotation, ParseError> {
        self.expect(TokenKind::AddressOf)?;
        self.consume();
        self.expect(TokenKind::OpenSqr)?;
        self.consume();
        let mut items = vec![];
        while let Some(t) = self.peek_n(0)
            && !matches!(t.kind, TokenKind::CloseSqr)
        {
            let name = self.parse_symbol()?.data;
            // Check if this is a Call form: `name(arg1, arg2, ...)`
            let item = if let Some(t) = self.peek_n(0)
                && matches!(t.kind, TokenKind::OpenPar)
            {
                self.consume(); // consume `(`
                let mut args = vec![];
                while let Some(t) = self.peek_n(0)
                    && !matches!(t.kind, TokenKind::ClosePar)
                {
                    // Try to parse as a type expression; fall back to bare symbol
                    let arg = self.parse_type_expr().map(AstAnnotationArg::Type)?;
                    args.push(arg);
                    if let Some(t) = self.peek_n(0)
                        && matches!(t.kind, TokenKind::Comma)
                    {
                        self.consume();
                    } else {
                        break;
                    }
                }
                self.expect(TokenKind::ClosePar)?;
                self.consume();
                AstAnnotationItem::Call { name, args }
            } else {
                AstAnnotationItem::Flag(name)
            };
            items.push(item);
            if let Some(t) = self.peek_n(0)
                && matches!(t.kind, TokenKind::Comma)
            {
                self.consume();
            } else {
                break;
            }
        }
        self.expect(TokenKind::CloseSqr)?;
        self.consume();
        Ok(AstAnnotation { items })
    }

    pub fn collect_annotations(&mut self) -> Result<(), ParseError> {
        assert!(self.annotations().is_empty());
        while let Some(t) = self.peek_n(0)
            && matches!(t.kind, TokenKind::AddressOf)
        {
            let annotation = self.parse_annotation()?;
            self.annotations.push(annotation)
        }
        Ok(())
    }

    fn parse_impl_item(&mut self) -> Result<AstImplItem, ParseError> {
        self.collect_annotations()?;
        let annotations = self.annotations();
        match self.current_token()?.kind {
            TokenKind::Type => {
                self.consume();
                let name = self.parse_symbol()?.data;
                self.expect(TokenKind::Eq)?;
                self.consume();
                let ty = self.parse_type_expr()?;
                Ok(AstImplItem::Type { name, ty })
            }
            TokenKind::Fun => {
                let mut fdef = self.parse_methoddef()?;
                fdef.annotations = annotations;
                Ok(AstImplItem::Fundef(fdef))
            }
            found => Err(self.parse_error(ParseErrorKind::ExpectedToken {
                expected: TokenKind::Fun,
                found,
            })),
        }
    }

    fn parse_optional_template_args(&mut self) -> Result<Vec<AstTemplateArg>, ParseError> {
        Ok(
            if let Some(t) = self.peek_n(0)
                && matches!(t.kind, TokenKind::Lt)
            {
                self.consume();
                let args = self.parse_template_args()?;
                self.expect(TokenKind::Gt)?;
                self.consume();
                args
            } else {
                vec![]
            },
        )
    }

    fn parse_impl_block(&mut self) -> Result<AstImplBlock, ParseError> {
        let start = self.get_start();
        self.expect(TokenKind::Impl)?;
        self.consume();
        let template_args = self.parse_optional_template_args()?;
        let first_ty = self.parse_type_expr()?;
        let (interface, implemented) = if let Some(t) = self.peek_n(0)
            && matches!(t.kind, TokenKind::For)
        {
            self.consume();
            let implemented = self.parse_type_expr()?;
            (Some(first_ty), implemented)
        } else {
            (None, first_ty)
        };

        self.expect(TokenKind::OpenBra)?;
        self.consume();

        let mut items = Vec::new();
        while let Some(t) = self.peek_n(0)
            && !matches!(t.kind, TokenKind::CloseBra)
        {
            items.push(self.parse_impl_item()?);
        }
        self.expect(TokenKind::CloseBra)?;
        self.consume();

        let span = start.span(&self.get_end());

        Ok(AstImplBlock {
            template_args,
            interface,
            implemented,
            items,
            span,
        })
    }

    fn parse_struct_def_field(&mut self) -> Result<AstStructDefField, ParseError> {
        let name = self.parse_symbol()?.data;
        self.expect(TokenKind::Colon)?;
        self.consume();
        let ty = self.parse_type_expr()?;
        Ok(AstStructDefField { name, ty })
    }

    fn parse_struct_def_fields(&mut self) -> Result<Vec<AstStructDefField>, ParseError> {
        let mut fields = Vec::new();
        while let Some(t) = self.peek_n(0)
            && matches!(t.kind, TokenKind::Identifier(_))
        {
            fields.push(self.parse_struct_def_field()?);
            if let Some(t) = self.peek_n(0)
                && matches!(t.kind, TokenKind::Semicolon)
            {
                self.consume();
            } else {
                break;
            }
        }
        Ok(fields)
    }

    fn parse_struct_def(&mut self) -> Result<AstStructDef, ParseError> {
        self.expect(TokenKind::Struct)?;
        self.consume();
        let name = self.parse_symbol()?.data;
        let template_args = self.parse_optional_template_args()?;
        self.expect(TokenKind::OpenBra)?;
        self.consume();
        let fields = self.parse_struct_def_fields()?;
        self.expect(TokenKind::CloseBra)?;
        self.consume();

        Ok(AstStructDef {
            name,
            template_args,
            fields,
        })
    }

    fn parse_interface_item(&mut self) -> Result<AstInterfaceItem, ParseError> {
        match self.current_token()?.kind {
            TokenKind::Type => {
                self.consume();
                let arg = self.parse_template_arg()?;
                self.expect(TokenKind::Semicolon)?;
                self.consume();
                Ok(AstInterfaceItem::Type(arg))
            }
            TokenKind::Fun => {
                self.consume();
                let sig = self.parse_methodsig()?;
                self.expect(TokenKind::Semicolon)?;
                self.consume();
                Ok(AstInterfaceItem::Sig(sig))
            }
            x => Err(ParseError {
                kind: ParseErrorKind::ExpectedToken {
                    expected: TokenKind::Fun,
                    found: x,
                },
                file: self.file.clone(),
                start: self.current_token()?.location.start,
                end: self.current_token()?.location.end,
            }),
        }
    }

    pub fn parse_toplevel_item(&mut self) -> Result<AstTopLevelItem, ParseError> {
        self.collect_annotations()?;
        let annotations = self.annotations();
        match &self.current_token()?.kind {
            TokenKind::Module => {
                let module = self.parse_module()?;
                let span = module.span.clone();
                Ok(AstTopLevelItem::new(
                    AstTopLevelItemDesc::Module(module),
                    annotations,
                    span,
                ))
            }
            TokenKind::Fun => {
                let fdef = self.parse_fundef()?;
                let span = fdef.span.clone();
                Ok(AstTopLevelItem::new(
                    AstTopLevelItemDesc::Fundef(fdef),
                    annotations,
                    span,
                ))
            }
            TokenKind::Impl => {
                self.collect_annotations()?;
                let annotations = self.annotations();
                let start = self.get_start();
                let impl_block = self.parse_impl_block()?;
                let end = self.get_end();
                Ok(AstTopLevelItem::new(
                    AstTopLevelItemDesc::Impl(impl_block),
                    annotations,
                    start.span(&end),
                ))
            }
            TokenKind::Struct => {
                let start = self.get_start();
                let struct_def = self.parse_struct_def()?;
                let end = self.get_end();
                let span = start.span(&end);
                Ok(AstTopLevelItem::new(
                    AstTopLevelItemDesc::StructDef(struct_def),
                    annotations,
                    span,
                ))
            }
            TokenKind::Interface => {
                let start = self.get_start();
                self.consume();
                let name = self.parse_symbol()?.data;
                let template_args = self.parse_optional_template_args()?;
                let supers = if let Some(t) = self.peek_n(0)
                    && t.kind == TokenKind::Colon
                {
                    let mut supers = vec![self.parse_type_expr()?];
                    while let Some(t) = self.peek_n(0)
                        && t.kind == TokenKind::Plus
                    {
                        self.consume();
                        supers.push(self.parse_type_expr()?);
                    }
                    self.expect(TokenKind::CloseBra)?;
                    self.consume();
                    supers
                } else {
                    vec![]
                };
                self.expect(TokenKind::OpenBra)?;
                self.consume();
                let mut items = vec![];
                while let Some(t) = self.peek_n(0)
                    && !matches!(t.kind, TokenKind::CloseBra)
                {
                    let item = self.parse_interface_item()?;
                    items.push(item);
                }
                self.expect(TokenKind::CloseBra)?;
                self.consume();
                Ok(AstTopLevelItem::new(
                    AstTopLevelItemDesc::Interface(AstInterface {
                        name,
                        supers,
                        template_args,
                        items,
                    }),
                    vec![],
                    start.span(&self.get_end()),
                ))
            }
            x => todo!("{:?}: {}", self.get_start(), x.display(self.db)),
        }
    }
}
