use nonempty::nonempty;

use crate::{
    common::symbols::Symbol,
    lexer::TokenKind,
    parse_tree::top_level::{
        AnyTopLevelItem, AnyTopLevelItemDesc, Fundef, FundefArg, FundefDesc, Funsig, FunsigDesc,
        IncludePath, Module, ModuleDesc, TemplateArg, TopLevelItem, TopLevelItemDesc,
    },
    parser::{ParseError, Parser},
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
                    start.span(&end),
                ))
            }
            _ => self
                .parse_toplevel_item()
                .map(|item| AnyTopLevelItem::new(AnyTopLevelItemDesc::Item(item.data), item.span)),
        }
    }

    pub fn parse_module(&mut self) -> Result<Module, ParseError> {
        let start = self.get_start();
        self.expect(TokenKind::Module)?;
        self.consume();
        let name = self.parse_symbol()?.data;
        self.expect(TokenKind::BigArrow)?;
        self.consume();
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
        Ok(Module::new(ModuleDesc { name, items }, start.span(&end)))
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
            && !matches!(t.kind, TokenKind::Lt)
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
            start.span(&end),
        ))
    }

    fn parse_fundef(&mut self) -> Result<Fundef, ParseError> {
        self.expect(TokenKind::Let)?;
        self.consume();
        let Funsig {
            data:
                FunsigDesc {
                    name,
                    args,
                    template_args,
                    return_type,
                },
            span,
        } = self.parse_funsig()?;
        self.expect(TokenKind::BigArrow)?;
        self.consume();

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
            span,
        ))
    }

    pub fn parse_toplevel_item(&mut self) -> Result<TopLevelItem, ParseError> {
        match &self.current_token()?.kind {
            TokenKind::Module => {
                let module = self.parse_module()?;
                let span = module.span.clone();
                Ok(TopLevelItem::new(TopLevelItemDesc::Module(module), span))
            }
            TokenKind::Let => {
                let fdef = self.parse_fundef()?;
                let span = fdef.span.clone();
                Ok(TopLevelItem::new(TopLevelItemDesc::Fundef(fdef), span))
            }
            x => todo!("{}", x.display(self.db)),
        }
    }
}
