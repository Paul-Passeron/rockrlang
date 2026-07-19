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

use crate::{Lsp, log};
use lsp_server::{Message, Request, Response};
use lsp_types::request::{HoverRequest, Request as IRequest};
use std::fmt::Display;

pub mod hover;

impl<'a> Lsp<'a> {
    fn dispatch_request<R: IRequest, Err: Display>(
        &mut self,
        r: Request,
        mut handler: impl FnMut(&mut Self, R::Params) -> Result<R::Result, Err>,
    ) {
        if r.method != R::METHOD {
            panic!(
                "Tried to handle request method `{}` using `{}` handler",
                r.method,
                R::METHOD
            )
        }
        let parsed = serde_json::from_value::<R::Params>(r.params).unwrap();
        let response = match handler(self, parsed) {
            Ok(result) => Response::new_ok(r.id, result),
            Err(msg) => Response::new_err(r.id, 1, msg.to_string()),
        };
        self.connection.sender.send(Message::Response(response)).unwrap();
    }

    pub fn handle_request(&mut self, req: Request) {
        match req.method.as_str() {
            HoverRequest::METHOD => self
                .dispatch_request::<HoverRequest, String>(req, |this, params| {
                    Ok(this.handle_hover(params))
                }),
            _ => log!("TODO: Unhandled request ! {}", req.method),
        }
    }
}
