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
