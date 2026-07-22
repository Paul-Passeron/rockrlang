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

use std::sync::Arc;

use nonempty::nonempty;
use salsa::Accumulator;

use crate::{
    common::symbols::Symbol,
    lexer::TokenKind,
    parse_tree::{
        Spanned,
        annotation::{AstAnnotation, AstAnnotationArg, AstAnnotationItem},
        top_level::{
            AstAnyTopLevelItem, AstAnyTopLevelItemDesc, AstEnumDef, AstEnumVariant,
            AstEnumVariantKind, AstFundef, AstFundefArg, AstFundefDesc, AstFunsig,
            AstFunsigDesc, AstImplBlock, AstImplItem, AstIncludePath, AstIncludePathDesc,
            AstInterface, AstInterfaceItem, AstMethodDef, AstMethodDefDesc, AstMethodsig,
            AstMethodsigDesc, AstModule, AstModuleDesc, AstReceiver, AstStructDef,
            AstStructDefField, AstTemplateArg, AstTopLevelItem, AstTopLevelItemDesc,
        },
    },
    parser::{
        ParseError,
        ParseErrorKind::{self, UnexpectedEOF},
        Parser,
    },
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

    pub(super) fn parse_any_toplevel_item(
        &mut self,
    ) -> Result<AstAnyTopLevelItem, ParseError> {
        let start = self.get_start();
        match &self.current_token()?.kind {
            TokenKind::Directive(dir) if *dir == Symbol::new(self.db, "include") => {
                self.consume();
                let include_path = self.parse_include_path()?;
                let end = self.get_end();
                Ok(AstAnyTopLevelItem::new(
                    AstAnyTopLevelItemDesc::Include(include_path),
                    vec![],
                    start.span(end),
                ))
            }
            _ => self.parse_toplevel_item().map(|item| {
                AstAnyTopLevelItem::new(
                    AstAnyTopLevelItemDesc::Item(Box::new(item.data)),
                    item.annotations,
                    item.span,
                )
            }),
        }
    }

    pub fn is_top_level_sync_point(k: &TokenKind) -> bool {
        matches!(
            k,
            TokenKind::Fun
                | TokenKind::Module
                | TokenKind::Directive(_)
                | TokenKind::Type
                | TokenKind::Interface
        )
    }

    pub fn parse_module(&mut self) -> Result<AstModule, ParseError> {
        let annotations = self.annotations();
        let start = self.get_start();
        self.expect(TokenKind::Module)?;
        self.consume();
        let name = self.parse_symbol()?;
        self.expect(TokenKind::OpenBra)?;
        self.consume();
        let mut items = vec![];
        let mut includes = vec![];
        while self.current_token()?.kind != TokenKind::CloseBra {
            if let TokenKind::Directive(s) = self.current_token()?.kind
                && s.interned().contents(self.db) == "include"
            {
                self.consume();
                let start = self.get_start();
                match self.parse_include_path() {
                    Ok(include) => includes.push(include),
                    Err(err) => {
                        self.synchronize(Self::is_top_level_sync_point);
                        err.clone().accumulate(self.db);
                        includes.push(AstIncludePath::new(
                            AstIncludePathDesc::Error,
                            vec![],
                            start.span(self.get_end()),
                        ));
                    }
                }
            } else {
                let item = self.parse_toplevel_item();
                match item {
                    Ok(item) => {
                        items.push(item);
                    }
                    Err(err) => {
                        self.synchronize(Self::is_top_level_sync_point);
                        err.clone().accumulate(self.db);
                        items.push(AstTopLevelItem::new(
                            AstTopLevelItemDesc::Error(err),
                            vec![],
                            start.span(self.get_end()),
                        ));
                    }
                }
            }
        }
        self.expect(TokenKind::CloseBra)?;
        self.consume();
        let end = self.get_end();
        Ok(AstModule::new(
            AstModuleDesc { name, items, includes },
            annotations,
            start.span(end),
        ))
    }

    fn parse_fundef_args(&mut self) -> Result<Vec<AstFundefArg>, ParseError> {
        self.parse_list(Self::parse_fundef_arg, TokenKind::Comma, |p| {
            p.peek_n(0).is_none_or(|t| {
                t.kind == TokenKind::ClosePar || t.kind == TokenKind::Plus // For variadics
            })
        })
    }

    fn parse_fundef_arg(&mut self) -> Result<AstFundefArg, ParseError> {
        let start = self.get_start();
        let name = self.parse_symbol()?;
        self.expect(TokenKind::Colon)?;
        self.consume();
        let ty = self.parse_type_expr()?;
        let end = self.get_end();
        Ok(AstFundefArg {
            name: name.data,
            name_span: name.span,
            ty,
            span: start.span(end),
        })
    }

    fn parse_template_arg(&mut self) -> Result<AstTemplateArg, ParseError> {
        let start = self.get_start();

        let name = self.parse_symbol()?;
        let constraints = if let Some(t) = self.peek_n(0)
            && matches!(t.kind, TokenKind::Colon)
        {
            self.consume();
            let args = self.parse_list(Self::parse_type_expr, TokenKind::Plus, |p| {
                p.peek_n(0).is_none_or(|t| t.kind == TokenKind::Comma)
            })?;
            if args.is_empty() {
                self.parse_type_expr()?; // Should bail early if we have no supertraits
            }
            args
        } else {
            vec![]
        };
        Ok(AstTemplateArg {
            name: name.data,
            name_span: name.span,
            constraints,
            span: start.span(self.get_end()),
        })
    }

    fn parse_template_args(&mut self) -> Result<Vec<AstTemplateArg>, ParseError> {
        self.parse_list(Self::parse_template_arg, TokenKind::Comma, |p| {
            p.peek_n(0).is_none_or(|t| t.kind == TokenKind::Gt)
        })
    }

    fn parse_receiver(&mut self) -> AstReceiver {
        self.speculate(|p| p.parse_receiver_aux()).unwrap_or(AstReceiver::None)
    }

    fn expect_self(&mut self) -> Result<(), ParseError> {
        let zelf = Symbol::new(self.db, "self");
        let t = self
            .peek_n(0)
            .map(|t| t.kind)
            .ok_or_else(|| self.parse_error(UnexpectedEOF))?;
        if !matches!(t, TokenKind::Identifier(symbol) if symbol == zelf) {
            return Err(self.parse_error(ParseErrorKind::ExpectedToken {
                expected: TokenKind::Identifier(zelf),
                found: t,
            }));
        }
        self.consume();
        Ok(())
    }

    fn parse_receiver_aux(&mut self) -> Result<AstReceiver, ParseError> {
        let start = self.get_start();
        let zelf = Symbol::new(self.db, "self");
        let Some(t) = self.peek_n(0) else {
            return Err(self.parse_error(ParseErrorKind::UnexpectedEOF));
        };
        match t.kind {
            TokenKind::Mut => {
                self.consume();
                self.expect_self()?;
                Ok(AstReceiver::MutZelf(start.span(self.get_end())))
            }
            TokenKind::Identifier(symbol) if symbol == zelf => {
                self.consume();
                Ok(AstReceiver::Zelf(start.span(self.get_end())))
            }
            TokenKind::BitAnd => {
                self.consume();
                if let Some(t) = self.peek_n(0)
                    && matches!(t.kind, TokenKind::Mut)
                {
                    self.consume();
                    self.expect_self()?;
                    Ok(AstReceiver::MutRefZelf(start.span(self.get_end())))
                } else {
                    self.expect_self()?;
                    Ok(AstReceiver::RefZelf(start.span(self.get_end())))
                }
            }
            TokenKind::Mult => {
                self.consume();
                if let Some(t) = self.peek_n(0)
                    && matches!(t.kind, TokenKind::Mut)
                {
                    self.consume();
                    self.expect_self()?;
                    Ok(AstReceiver::MutPtrZelf(start.span(self.get_end())))
                } else {
                    self.expect_self()?;
                    Ok(AstReceiver::PtrZelf(start.span(self.get_end())))
                }
            }
            _ => Err(self.parse_error(ParseErrorKind::ExpectedToken {
                expected: TokenKind::Identifier(zelf),
                found: t.kind,
            })),
        }
    }

    fn parse_funsig(
        &mut self,
        can_be_variadic: bool,
    ) -> Result<(AstFunsig, bool), ParseError> {
        let (sig, _, var) = self.parse_any_funsig(false, can_be_variadic)?;
        Ok((sig, var))
    }

    fn parse_methodsig(&mut self) -> Result<AstMethodsig, ParseError> {
        let (
            AstFunsig {
                data: AstFunsigDesc { name, args, template_args, return_type },
                annotations,
                span,
            },
            receiver,
            _,
        ) = self.parse_any_funsig(true, false)?;
        let receiver = receiver.unwrap();
        Ok(AstMethodsig::new(
            AstMethodsigDesc { name, receiver, args, template_args, return_type },
            annotations,
            span,
        ))
    }

    fn parse_any_funsig(
        &mut self,
        accept_receiver: bool,
        can_be_variadic: bool,
    ) -> Result<(AstFunsig, Option<AstReceiver>, bool), ParseError> {
        assert!(!(accept_receiver && can_be_variadic));
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
            } else if r != AstReceiver::None {
                has_args = false;
            }
            Some(r)
        } else {
            None
        };

        let args = if has_args { self.parse_fundef_args()? } else { vec![] };

        let variadic = if let Some(t) = self.peek_n(0)
            && matches!(t.kind, TokenKind::Plus)
            && can_be_variadic
        {
            self.consume();
            true
        } else {
            false
        };

        self.expect(TokenKind::ClosePar)?;
        self.consume();

        self.expect(TokenKind::Colon)?;
        self.consume();

        let return_type = self.parse_type_expr()?;

        let end = self.get_end();

        Ok((
            AstFunsig::new(
                AstFunsigDesc { name, args, template_args, return_type },
                annotations,
                start.span(end),
            ),
            receiver,
            variadic,
        ))
    }

    fn parse_methoddef(&mut self) -> Result<AstMethodDef, ParseError> {
        let start = self.get_start();
        self.expect(TokenKind::Fun)?;
        self.consume();
        let AstMethodsig {
            data: AstMethodsigDesc { name, receiver, args, template_args, return_type },
            annotations,
            ..
        } = self.parse_methodsig()?;

        let body_start = self.get_start();
        let body = self.parse_block()?;
        let end = self.get_end();
        let body_span = body_start.span(end);
        let span = start.span(end);
        Ok(AstMethodDef::new(
            AstMethodDefDesc {
                name,
                receiver,
                args,
                template_args,
                return_type,
                body_span,
                body,
            },
            annotations,
            span,
        ))
    }

    fn parse_fundef(&mut self) -> Result<AstFundef, ParseError> {
        let start = self.get_start();
        self.expect(TokenKind::Fun)?;
        self.consume();
        let (
            AstFunsig {
                data: AstFunsigDesc { name, args, template_args, return_type },
                annotations,
                ..
            },
            _,
        ) = self.parse_funsig(false)?;

        let body_start = self.get_start();
        let body = self.parse_block()?;
        let end = self.get_end();
        let body_span = body_start.span(end);
        let span = start.span(end);
        Ok(AstFundef::new(
            AstFundefDesc { name, args, template_args, return_type, body_span, body },
            annotations,
            span,
        ))
    }

    pub fn parse_annotation_item(&mut self) -> Result<AstAnnotationItem, ParseError> {
        let name = self.parse_symbol()?.data;
        // Check if this is a Call form: `name(arg1, arg2, ...)`
        if self.peek_n(0).is_none_or(|t| t.kind != TokenKind::OpenPar) {
            return Ok(AstAnnotationItem::Flag(name));
        };
        self.consume(); // consume `(`
        let args = self.parse_list(
            |p| p.parse_type_expr().map(AstAnnotationArg::Type),
            TokenKind::Comma,
            |p| p.peek_n(0).is_none_or(|t| t.kind == TokenKind::ClosePar),
        )?;
        self.expect(TokenKind::ClosePar)?;
        self.consume();
        Ok(AstAnnotationItem::Call { name, args })
    }

    pub fn parse_annotation(&mut self) -> Result<AstAnnotation, ParseError> {
        self.expect(TokenKind::AddressOf)?;
        self.consume();
        self.expect(TokenKind::OpenSqr)?;
        self.consume();
        let items =
            self.parse_list(Self::parse_annotation_item, TokenKind::Comma, |p| {
                p.peek_n(0).is_none_or(|t| t.kind == TokenKind::CloseSqr)
            })?;
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
                let Spanned { data: name, span: name_span, .. } = self.parse_symbol()?;
                self.expect(TokenKind::Eq)?;
                self.consume();
                let ty = self.parse_type_expr()?;
                self.expect(TokenKind::Semicolon)?;
                self.consume();
                Ok(AstImplItem::Type { name, name_span, ty })
            }
            TokenKind::Fun => {
                let mut fdef = self.parse_methoddef()?;
                fdef.annotations = annotations;
                Ok(AstImplItem::Fundef(Box::new(fdef)))
            }
            found => Err(self.parse_error(ParseErrorKind::ExpectedToken {
                expected: TokenKind::Fun,
                found,
            })),
        }
    }

    fn parse_optional_template_args(
        &mut self,
    ) -> Result<Vec<AstTemplateArg>, ParseError> {
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

        let span = start.span(self.get_end());

        Ok(AstImplBlock { template_args, interface, implemented, items, span })
    }

    fn parse_struct_def_field(&mut self) -> Result<AstStructDefField, ParseError> {
        let start = self.get_start();
        let name = self.parse_symbol()?.data;
        self.expect(TokenKind::Colon)?;
        self.consume();
        let ty = self.parse_type_expr()?;
        let end = self.get_end();
        Ok(AstStructDefField { name, ty, span: start.span(end) })
    }

    fn parse_struct_def_fields(&mut self) -> Result<Vec<AstStructDefField>, ParseError> {
        self.parse_list(Self::parse_struct_def_field, TokenKind::Semicolon, |p| {
            p.peek_n(0).is_none_or(|t| t.kind == TokenKind::CloseBra)
        })
    }

    fn parse_enum_variant(&mut self) -> Result<AstEnumVariant, ParseError> {
        let start = self.get_start();
        let name = self.parse_symbol()?.data;

        let kind = match self.peek_n(0).map(|t| t.kind) {
            Some(TokenKind::OpenBra) => {
                self.consume();
                let fields = self.parse_struct_def_fields()?;
                self.expect(TokenKind::CloseBra)?;
                self.consume();
                AstEnumVariantKind::StructLike(fields)
            }
            Some(TokenKind::OpenPar) => {
                self.consume();
                let fields =
                    self.parse_list(Self::parse_type_expr, TokenKind::Comma, |p| {
                        p.peek_n(0).is_none_or(|t| t.kind == TokenKind::ClosePar)
                    })?;

                self.expect(TokenKind::ClosePar)?;
                self.consume();
                AstEnumVariantKind::TupleLike(fields)
            }
            _ => AstEnumVariantKind::Unit,
        };
        let end = self.get_end();
        Ok(AstEnumVariant { name, kind, span: start.span(end) })
    }

    fn parse_enum_variants(&mut self) -> Result<Vec<AstEnumVariant>, ParseError> {
        self.parse_list(Self::parse_enum_variant, TokenKind::Comma, |p| {
            p.peek_n(0).is_none_or(|t| t.kind == TokenKind::CloseBra)
        })
    }

    fn parse_enum_def(&mut self) -> Result<AstEnumDef, ParseError> {
        let start = self.get_start();
        self.expect(TokenKind::Enum)?;
        self.consume();
        let name = self.parse_symbol()?;
        let template_args = self.parse_optional_template_args()?;
        self.expect(TokenKind::OpenBra)?;
        self.consume();
        let variants = self.parse_enum_variants()?;
        self.expect(TokenKind::CloseBra)?;
        self.consume();
        Ok(AstEnumDef { name, template_args, variants, span: start.span(self.get_end()) })
    }

    fn parse_struct_def(&mut self) -> Result<AstStructDef, ParseError> {
        let start = self.get_start();
        self.expect(TokenKind::Struct)?;
        self.consume();
        let name = self.parse_symbol()?;
        let template_args = self.parse_optional_template_args()?;
        self.expect(TokenKind::OpenBra)?;
        self.consume();
        let fields = self.parse_struct_def_fields()?;
        self.expect(TokenKind::CloseBra)?;
        self.consume();

        Ok(AstStructDef { name, template_args, fields, span: start.span(self.get_end()) })
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
                Ok(AstInterfaceItem::Sig(Arc::new(sig)))
            }
            x => Err(ParseError {
                kind: ParseErrorKind::ExpectedToken {
                    expected: TokenKind::Fun,
                    found: x,
                },
                file: self.file,
                start: self.current_token()?.location.start_offset,
                end: self.current_token()?.location.end_offset,
            }),
        }
    }

    pub fn parse_toplevel_item(&mut self) -> Result<AstTopLevelItem, ParseError> {
        self.collect_annotations()?;
        let annotations = self.annotations();
        let start = self.get_start();
        match &self.current_token()?.kind {
            TokenKind::Module => match self.parse_module() {
                Ok(module) => {
                    let span = module.span;
                    Ok(AstTopLevelItem::new(
                        AstTopLevelItemDesc::Module(module),
                        annotations,
                        span,
                    ))
                }
                Err(err) => {
                    err.clone().accumulate(self.db);
                    Ok(AstTopLevelItem::new(
                        AstTopLevelItemDesc::Error(err),
                        annotations,
                        start.span(self.get_end()),
                    ))
                }
            },
            TokenKind::Fun => {
                let fdef = self.parse_fundef()?;
                let span = fdef.span;
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
                    start.span(end),
                ))
            }
            TokenKind::Struct => {
                let start = self.get_start();
                let struct_def = self.parse_struct_def()?;
                let end = self.get_end();
                let span = start.span(end);
                Ok(AstTopLevelItem::new(
                    AstTopLevelItemDesc::StructDef(struct_def),
                    annotations,
                    span,
                ))
            }
            TokenKind::Enum => {
                let start = self.get_start();
                let enum_def = self.parse_enum_def()?;
                let end = self.get_end();
                let span = start.span(end);
                Ok(AstTopLevelItem::new(
                    AstTopLevelItemDesc::EnumDef(enum_def),
                    annotations,
                    span,
                ))
            }
            TokenKind::Interface => {
                let start = self.get_start();
                self.consume();
                let name = self.parse_symbol()?;
                let template_args = self.parse_optional_template_args()?;
                let supers = if let Some(t) = self.peek_n(0)
                    && t.kind == TokenKind::Colon
                {
                    self.consume(); // ':'
                    let mut supers = vec![self.parse_type_expr()?];
                    while let Some(t) = self.peek_n(0)
                        && t.kind == TokenKind::Plus
                    {
                        self.consume();
                        supers.push(self.parse_type_expr()?);
                    }
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
                        span: start.span(self.get_end()),
                    }),
                    vec![],
                    start.span(self.get_end()),
                ))
            }
            TokenKind::Directive(s) if *s == Symbol::new(self.db, "extern") => {
                let start = self.get_start();
                self.consume();
                self.expect(TokenKind::OpenBra)?;
                self.consume();
                self.collect_annotations()?;
                let fun_start = self.get_start();
                self.expect(TokenKind::Fun)?;
                self.consume();
                let (mut funsig, variadic) = self.parse_funsig(true)?;
                funsig.span.start_offset = fun_start.offset;
                self.expect(TokenKind::Semicolon)?;
                self.consume();
                self.expect(TokenKind::CloseBra)?;
                self.consume();
                let end = self.get_end();
                Ok(AstTopLevelItem::new(
                    AstTopLevelItemDesc::ExternDef(funsig, variadic),
                    vec![],
                    start.span(end),
                ))
            }
            _ => Err(self.parse_error(ParseErrorKind::NotATopLevelItem)),
        }
    }
}
