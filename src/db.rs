use crate::CompilerConfig;

#[salsa::db]
#[derive(Clone)]
pub struct RockrDb {
    pub storage: salsa::Storage<Self>,
    pub config: CompilerConfig,
}

#[salsa::db]
pub trait Db: salsa::Database {
    fn config(&self) -> &CompilerConfig;
}

#[salsa::db]
impl Db for RockrDb {
    fn config(&self) -> &CompilerConfig {
        &self.config
    }
}

impl RockrDb {
    pub fn new(config: CompilerConfig) -> Self {
        Self {
            storage: salsa::Storage::default(),
            config,
        }
    }
}

#[salsa::db]
impl salsa::Database for RockrDb {}
