use nonempty::nonempty;

use crate::{
    common::symbols::Symbol,
    lexer::TokenKind,
    parse_tree::{
        annotation::{Annotation, AnnotationItem},
        top_level::{
            AnyTopLevelItem, AnyTopLevelItemDesc, Fundef, FundefArg, FundefDesc, Funsig,
            FunsigDesc, ImplBlock, ImplItem, IncludePath, Module, ModuleDesc, StructDef,
            StructDefField, TemplateArg, TopLevelItem, TopLevelItemDesc,
        },
    },
    parser::{ParseError, ParseErrorKind, Parser},
};

impl<'db> Parser<'db> {
    fn parse_include_path(&mut self) -> Result<IncludePath, ParseError> {
        let mut symbols = nonempty![self.parse_symbol()?];
        while self.current_token()?.kind == TokenKind::Access {
            self.consume();
            symbols.push(self.parse_symbol()?);
        }
        Ok(IncludePath::from(symbols))
    }

    pub(super) fn parse_any_toplevel_item(&mut self) -> Result<AnyTopLevelItem, ParseError> {
        let start = self.get_start();
        match &self.current_token()?.kind {
            TokenKind::Directive(dir) if *dir == Symbol::new(self.db, "include") => {
                self.consume();
                let include_path = self.parse_include_path()?;
                let end = self.get_end();
                Ok(AnyTopLevelItem::new(
                    AnyTopLevelItemDesc::Include(include_path),
                    vec![],
                    start.span(&end),
                ))
            }
            _ => self.parse_toplevel_item().map(|item| {
                AnyTopLevelItem::new(
                    AnyTopLevelItemDesc::Item(item.data),
                    item.annotations,
                    item.span,
                )
            }),
        }
    }

    pub fn parse_module(&mut self) -> Result<Module, ParseError> {
        let annotations = self.annotations();
        let start = self.get_start();
        self.expect(TokenKind::Module)?;
        self.consume();
        let name = self.parse_symbol()?.data;
        self.expect(TokenKind::OpenBra)?;
        self.consume();
        let mut items = vec![];
        while self.current_token()?.kind != TokenKind::CloseBra {
            let item = self.parse_toplevel_item()?;
            items.push(item);
        }
        self.expect(TokenKind::CloseBra)?;
        self.consume();
        let end = self.get_end();
        Ok(Module::new(
            ModuleDesc { name, items },
            annotations,
            start.span(&end),
        ))
    }

    fn parse_fundef_args(&mut self) -> Result<Vec<FundefArg>, ParseError> {
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

    fn parse_fundef_arg(&mut self) -> Result<FundefArg, ParseError> {
        // TODO: handle patterns
        let name = self.parse_symbol()?;
        self.expect(TokenKind::Colon)?;
        self.consume();
        let ty = self.parse_type_expr()?;
        Ok(FundefArg {
            name: name.data,
            ty,
        })
    }

    fn parse_template_arg(&mut self) -> Result<TemplateArg, ParseError> {
        let name = self.parse_symbol()?.data;
        let constraints = if let Some(t) = self.peek_n(0)
            && matches!(t.kind, TokenKind::Colon)
        {
            self.consume();
            let mut cs = vec![self.parse_type_expr()?];
            while let Some(t) = self.peek_n(0)
                && matches!(t.kind, TokenKind::Plus)
            {
                cs.push(self.parse_type_expr()?);
            }
            cs
        } else {
            vec![]
        };
        Ok(TemplateArg { name, constraints })
    }

    fn parse_template_args(&mut self) -> Result<Vec<TemplateArg>, ParseError> {
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

    fn parse_funsig(&mut self) -> Result<Funsig, ParseError> {
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
        let args = self.parse_fundef_args()?;
        self.expect(TokenKind::ClosePar)?;
        self.consume();

        self.expect(TokenKind::Colon)?;
        self.consume();

        let return_type = self.parse_type_expr()?;

        let end = self.get_end();

        Ok(Funsig::new(
            FunsigDesc {
                name: name.data,
                args,
                template_args,
                return_type,
            },
            annotations,
            start.span(&end),
        ))
    }

    fn parse_fundef(&mut self) -> Result<Fundef, ParseError> {
        self.expect(TokenKind::Fun)?;
        self.consume();
        let Funsig {
            data:
                FunsigDesc {
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

        Ok(Fundef::new(
            FundefDesc {
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

    pub fn parse_annotation(&mut self) -> Result<Annotation, ParseError> {
        self.expect(TokenKind::AddressOf)?;
        self.consume();
        self.expect(TokenKind::OpenSqr)?;
        self.consume();
        let mut items = vec![];
        while let Some(t) = self.peek_n(0)
            && !matches!(t.kind, TokenKind::CloseSqr)
        {
            items.push(AnnotationItem::Named(self.parse_symbol()?.data));
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
        Ok(Annotation { items })
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

    fn parse_impl_item(&mut self) -> Result<ImplItem, ParseError> {
        self.collect_annotations()?;
        let annotations = self.annotations();
        match self.current_token()?.kind {
            TokenKind::Type => {
                self.consume();
                let name = self.parse_symbol()?.data;
                self.expect(TokenKind::Eq)?;
                self.consume();
                let ty = self.parse_type_expr()?;
                Ok(ImplItem::Type { name, ty })
            }
            TokenKind::Fun => {
                let mut fdef = self.parse_fundef()?;
                fdef.annotations = annotations;
                Ok(ImplItem::Fundef(fdef))
            }
            found => Err(self.parse_error(ParseErrorKind::ExpectedToken {
                expected: TokenKind::Fun,
                found,
            })),
        }
    }

    fn parse_optional_template_args(&mut self) -> Result<Vec<TemplateArg>, ParseError> {
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

    fn parse_impl_block(&mut self) -> Result<ImplBlock, ParseError> {
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

        Ok(ImplBlock {
            template_args,
            interface,
            implemented,
            items,
        })
    }

    fn parse_struct_def_field(&mut self) -> Result<StructDefField, ParseError> {
        let name = self.parse_symbol()?.data;
        self.expect(TokenKind::Colon)?;
        self.consume();
        let ty = self.parse_type_expr()?;
        Ok(StructDefField { name, ty })
    }

    fn parse_struct_def_fields(&mut self) -> Result<Vec<StructDefField>, ParseError> {
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

    fn parse_struct_def(&mut self) -> Result<StructDef, ParseError> {
        self.expect(TokenKind::Struct)?;
        self.consume();
        let name = self.parse_symbol()?.data;
        let template_args = self.parse_optional_template_args()?;
        self.expect(TokenKind::OpenBra)?;
        self.consume();
        let fields = self.parse_struct_def_fields()?;
        self.expect(TokenKind::CloseBra)?;
        self.consume();

        Ok(StructDef {
            name,
            template_args,
            fields,
        })
    }

    pub fn parse_toplevel_item(&mut self) -> Result<TopLevelItem, ParseError> {
        self.collect_annotations()?;
        let annotations = self.annotations();
        match &self.current_token()?.kind {
            TokenKind::Module => {
                let module = self.parse_module()?;
                let span = module.span.clone();
                Ok(TopLevelItem::new(
                    TopLevelItemDesc::Module(module),
                    annotations,
                    span,
                ))
            }
            TokenKind::Fun => {
                let fdef = self.parse_fundef()?;
                let span = fdef.span.clone();
                Ok(TopLevelItem::new(
                    TopLevelItemDesc::Fundef(fdef),
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
                Ok(TopLevelItem::new(
                    TopLevelItemDesc::Impl(impl_block),
                    annotations,
                    start.span(&end),
                ))
            }
            TokenKind::Struct => {
                let start = self.get_start();
                let struct_def = self.parse_struct_def()?;
                let end = self.get_end();
                let span = start.span(&end);
                Ok(TopLevelItem::new(
                    TopLevelItemDesc::StructDef(struct_def),
                    annotations,
                    span,
                ))
            }
            x => todo!("{:?}: {}", self.get_start(), x.display(self.db)),
        }
    }
}
