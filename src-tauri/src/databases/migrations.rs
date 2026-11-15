
use rusqlite::{Connection, Result};

const CURRENT_VERSION: i64 = 1;

pub fn run(connection: &mut Connection) -> Result<()> {
    let version: i64 = connection.pragma_query_value(
        None,
        "user_version",
        |row| row.get(0),
    )?;

    if version < 1 {
        let transaction = connection.transaction()?;

        transaction.execute_batch(
            "
            CREATE TABLE gmail_accounts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                email_address TEXT NOT NULL COLLATE NOCASE UNIQUE,
                encrypted_token_reference TEXT,
                history_id TEXT,
                sync_start_date TEXT,
                last_sync_at TEXT,
                account_status TEXT NOT NULL DEFAULT 'disconnected',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gmail_account_id INTEGER NOT NULL,
                gmail_message_id TEXT NOT NULL,
                flip_transaction_id TEXT,
                transaction_date TEXT,
                recipient_name TEXT,
                amount INTEGER,
                transfer_note TEXT,
                ocr_raw_result TEXT,
                status TEXT NOT NULL DEFAULT 'Detected',
                review_reason TEXT,
                png_path TEXT,
                first_copied_at TEXT,
                last_copied_at TEXT,
                retention_until TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,

                FOREIGN KEY (gmail_account_id)
                    REFERENCES gmail_accounts(id)
                    ON DELETE RESTRICT,

                UNIQUE (gmail_account_id, gmail_message_id),

                CHECK (
                    status IN (
                        'Detected',
                        'Parsed',
                        'OCR Processing',
                        'Completed',
                        'Needs Review',
                        'Duplicate Review',
                        'Failed'
                    )
                )
            );

            CREATE INDEX idx_transactions_date
                ON transactions(transaction_date);

            CREATE INDEX idx_transactions_status
                ON transactions(status);

            CREATE INDEX idx_transactions_flip_id
                ON transactions(flip_transaction_id);

            CREATE TABLE correction_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                transaction_id INTEGER NOT NULL,
                field_name TEXT NOT NULL,
                old_value TEXT,
                new_value TEXT,
                corrected_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,

                FOREIGN KEY (transaction_id)
                    REFERENCES transactions(id)
                    ON DELETE CASCADE
            );

            CREATE TABLE processing_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gmail_account_id INTEGER,
                transaction_id INTEGER,
                process_type TEXT NOT NULL,
                status TEXT NOT NULL,
                error_message TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,

                FOREIGN KEY (gmail_account_id)
                    REFERENCES gmail_accounts(id)
                    ON DELETE SET NULL,

                FOREIGN KEY (transaction_id)
                    REFERENCES transactions(id)
                    ON DELETE SET NULL
            );

            CREATE TABLE app_settings (
                setting_key TEXT PRIMARY KEY,
                setting_value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            "
        )?;

        transaction.pragma_update(None, "user_version", 1)?;

        transaction.commit()?;
    }

    if version > CURRENT_VERSION {
        return Err(rusqlite::Error::InvalidQuery);
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn create_test_database() -> Connection {
        let mut connection = Connection::open_in_memory().unwrap();

        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();

        run(&mut connection).unwrap();

        connection
    }

    #[test]
    fn migration_sets_database_version_to_one() {
        let connection = create_test_database();

        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();

        assert_eq!(version, 1);
    }

    #[test]
    fn migration_creates_all_required_tables() {
        let connection = create_test_database();

        let tables = [
            "gmail_accounts",
            "transactions",
            "correction_history",
            "processing_logs",
            "app_settings",
        ];

        for table in tables {
            let exists: bool = connection
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = ?1
                    )",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();

            assert!(exists, "Table {table} tidak ditemukan");
        }
    }

    #[test]
    fn migration_can_run_twice_without_recreating_tables() {
        let mut connection = create_test_database();

        let result = run(&mut connection);

        assert!(result.is_ok());

        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();

        assert_eq!(version, 1);
    }

    #[test]
    fn transaction_requires_existing_gmail_account() {
        let connection = create_test_database();

        let result = connection.execute(
            "INSERT INTO transactions (
                gmail_account_id,
                gmail_message_id
            ) VALUES (999, 'message-test')",
            [],
        );

        assert!(result.is_err());
    }

    #[test]
    fn duplicate_message_id_is_rejected_within_same_account() {
        let connection = create_test_database();

        connection
            .execute(
                "INSERT INTO gmail_accounts (email_address)
                 VALUES ('test@gmail.com')",
                [],
            )
            .unwrap();

        let first = connection.execute(
            "INSERT INTO transactions (
                gmail_account_id,
                gmail_message_id
            ) VALUES (1, 'message-001')",
            [],
        );

        assert!(first.is_ok());

        let duplicate = connection.execute(
            "INSERT INTO transactions (
                gmail_account_id,
                gmail_message_id
            ) VALUES (1, 'message-001')",
            [],
        );

        assert!(duplicate.is_err());
    }
}


