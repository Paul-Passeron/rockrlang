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

use crate::compiler;

#[salsa::db]
#[derive(Clone)]
pub struct RockrDb {
    pub storage: salsa::Storage<Self>,
    pub config: compiler::Config,
}

#[salsa::db]
pub trait Db: salsa::Database {
    fn config(&self) -> &compiler::Config;
}

#[salsa::db]
impl Db for RockrDb {
    fn config(&self) -> &compiler::Config {
        &self.config
    }
}

impl RockrDb {
    pub fn new(config: compiler::Config) -> Self {
        Self {
            storage: salsa::Storage::default(),
            config,
        }
    }
}

#[salsa::db]
impl salsa::Database for RockrDb {}
