
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
        let connection = Connection::open(path)?;

        // Mengaktifkan foreign key.
        connection.pragma_update(None, "foreign_keys", "ON")?;

        // Menggunakan Write-Ahead Logging.
        connection.pragma_update(None, "journal_mode", "WAL")?;

        // Menunggu maksimal 5 detik jika database sedang dikunci.
        connection.busy_timeout(Duration::from_secs(5))?;

        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
}
