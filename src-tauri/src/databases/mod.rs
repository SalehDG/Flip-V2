
mod migrations;

use rusqlite::Connection;
use std::{
    path::Path,
    sync::Mutex,
    time::Duration,
};

pub struct Database {
    pub connection: Mutex<Connection>,
}

impl Database {
    pub fn new(path: &Path) -> rusqlite::Result<Self> {
        let mut connection = Connection::open(path)?;

        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.busy_timeout(Duration::from_secs(5))?;

        migrations::run(&mut connection)?;

        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
}
