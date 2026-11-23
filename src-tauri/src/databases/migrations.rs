use rusqlite::{Connection, Result};

const CURRENT_VERSION: i64 = 6;

pub fn run(connection: &mut Connection) -> Result<()> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;

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
            ",
        )?;

        transaction.pragma_update(None, "user_version", 1)?;

        transaction.commit()?;
    }

    if version < 2 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "
            CREATE TABLE gmail_sync_state (
                gmail_account_id INTEGER PRIMARY KEY,
                sync_status TEXT NOT NULL DEFAULT 'idle',
                sync_start_date TEXT,
                page_token TEXT,
                initial_history_id TEXT,
                last_error TEXT,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (gmail_account_id) REFERENCES gmail_accounts(id) ON DELETE CASCADE,
                CHECK (sync_status IN ('idle', 'initial_sync', 'completed', 'failed', 'authentication_required'))
            );

            CREATE TABLE gmail_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gmail_account_id INTEGER NOT NULL,
                gmail_message_id TEXT NOT NULL,
                gmail_thread_id TEXT NOT NULL,
                history_id TEXT,
                internal_date TEXT,
                sender TEXT,
                subject TEXT,
                snippet TEXT,
                payload_json TEXT NOT NULL,
                retrieved_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (gmail_account_id) REFERENCES gmail_accounts(id) ON DELETE CASCADE,
                UNIQUE (gmail_account_id, gmail_message_id)
            );

            CREATE INDEX idx_gmail_messages_thread
                ON gmail_messages(gmail_account_id, gmail_thread_id);

            CREATE TABLE gmail_attachments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gmail_account_id INTEGER NOT NULL,
                gmail_message_id TEXT NOT NULL,
                part_key TEXT NOT NULL,
                attachment_id TEXT,
                filename TEXT,
                declared_mime_type TEXT,
                detected_format TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                sha256 TEXT NOT NULL,
                local_path TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (gmail_account_id, gmail_message_id)
                    REFERENCES gmail_messages(gmail_account_id, gmail_message_id) ON DELETE CASCADE,
                UNIQUE (gmail_account_id, gmail_message_id, part_key)
            );

            CREATE INDEX idx_gmail_attachments_message
                ON gmail_attachments(gmail_account_id, gmail_message_id);
            "
        )?;
        transaction.pragma_update(None, "user_version", 2)?;
        transaction.commit()?;
    }

    if version < 3 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "
            ALTER TABLE gmail_sync_state RENAME TO gmail_sync_state_v2;

            CREATE TABLE gmail_sync_state (
                gmail_account_id INTEGER PRIMARY KEY,
                sync_status TEXT NOT NULL DEFAULT 'idle',
                sync_start_date TEXT,
                page_token TEXT,
                initial_history_id TEXT,
                latest_history_id TEXT,
                last_error TEXT,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (gmail_account_id) REFERENCES gmail_accounts(id) ON DELETE CASCADE,
                CHECK (sync_status IN (
                    'idle',
                    'initial_sync',
                    'incremental_sync',
                    'completed',
                    'failed',
                    'authentication_required'
                ))
            );

            INSERT INTO gmail_sync_state (
                gmail_account_id,
                sync_status,
                sync_start_date,
                page_token,
                initial_history_id,
                last_error,
                updated_at
            )
            SELECT
                gmail_account_id,
                sync_status,
                sync_start_date,
                page_token,
                initial_history_id,
                last_error,
                updated_at
            FROM gmail_sync_state_v2;

            DROP TABLE gmail_sync_state_v2;

            CREATE INDEX IF NOT EXISTS idx_gmail_messages_account_history
                ON gmail_messages(gmail_account_id, history_id);
            ",
        )?;
        transaction.pragma_update(None, "user_version", 3)?;
        transaction.commit()?;
    }

    if version < 4 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "ALTER TABLE transactions ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE transactions ADD COLUMN last_attempt_at TEXT;
             ALTER TABLE transactions ADD COLUMN last_error TEXT;
             CREATE INDEX IF NOT EXISTS idx_transactions_retry_status
                ON transactions(status, attempt_count);
             CREATE INDEX IF NOT EXISTS idx_processing_logs_transaction
                ON processing_logs(transaction_id, created_at DESC);"
        )?;
        transaction.pragma_update(None, "user_version", 4)?;
        transaction.commit()?;
    }


    // Version 5 repairs extraction metadata from earlier Phase 2 builds.
    // Some v4 databases used retry_count, while newer extraction code expects attempt_count.
    // This migration is idempotent and preserves the existing retry values.
    if version < 5 {
        let transaction = connection.transaction()?;

        let has_retry_count: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('transactions') WHERE name='retry_count')",
            [],
            |row| row.get(0),
        )?;
        let has_attempt_count: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('transactions') WHERE name='attempt_count')",
            [],
            |row| row.get(0),
        )?;
        let has_last_attempt_at: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('transactions') WHERE name='last_attempt_at')",
            [],
            |row| row.get(0),
        )?;
        let has_last_error: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('transactions') WHERE name='last_error')",
            [],
            |row| row.get(0),
        )?;

        if has_retry_count && !has_attempt_count {
            transaction.execute_batch("ALTER TABLE transactions RENAME COLUMN retry_count TO attempt_count;")?;
        } else if !has_attempt_count {
            transaction.execute_batch("ALTER TABLE transactions ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0;")?;
        } else if has_retry_count {
            // If both columns exist, retain the larger retry value before removing the legacy column.
            // Drop the old retry index first because it may still reference retry_count.
            transaction.execute_batch(
                "DROP INDEX IF EXISTS idx_transactions_retry_status;
                 UPDATE transactions SET attempt_count=MAX(attempt_count, retry_count);
                 ALTER TABLE transactions DROP COLUMN retry_count;"
            )?;
        }

        if !has_last_attempt_at {
            transaction.execute_batch("ALTER TABLE transactions ADD COLUMN last_attempt_at TEXT;")?;
        }
        if !has_last_error {
            transaction.execute_batch("ALTER TABLE transactions ADD COLUMN last_error TEXT;")?;
        }

        transaction.execute_batch(
            "DROP INDEX IF EXISTS idx_transactions_retry_status;
             CREATE INDEX idx_transactions_retry_status ON transactions(status, attempt_count);
             CREATE INDEX IF NOT EXISTS idx_processing_logs_transaction
                ON processing_logs(transaction_id, created_at DESC);"
        )?;
        transaction.pragma_update(None, "user_version", 5)?;
        transaction.commit()?;
    }


    // Version 6 adds reversible soft-merge metadata. Merged source rows remain
    // stored for audit, while queries can exclude them from active transaction lists.
    if version < 6 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "ALTER TABLE transactions ADD COLUMN merged_into_id INTEGER;
             CREATE INDEX idx_transactions_merged_into ON transactions(merged_into_id);
             CREATE TABLE transaction_merge_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                master_transaction_id INTEGER NOT NULL,
                source_transaction_id INTEGER NOT NULL,
                source_snapshot_json TEXT NOT NULL,
                merge_type TEXT NOT NULL,
                merged_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );"
        )?;
        transaction.pragma_update(None, "user_version", 6)?;
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
    fn migration_sets_database_version_to_five() {
        let connection = create_test_database();

        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();

        assert_eq!(version, 6);
    }

    #[test]
    fn migration_adds_extraction_retry_metadata() {
        let connection = create_test_database();
        for column in ["attempt_count", "last_attempt_at", "last_error"] {
            let exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('transactions') WHERE name=?1)",
                [column],
                |row| row.get(0),
            ).unwrap();
            assert!(exists, "Column {column} missing");
        }
    }

    #[test]
    fn migration_repairs_legacy_v4_retry_metadata() {
        let mut connection = create_test_database();
        connection.execute_batch(
            "ALTER TABLE transactions RENAME COLUMN attempt_count TO retry_count;
             ALTER TABLE transactions DROP COLUMN last_error;
             PRAGMA user_version = 4;"
        ).unwrap();

        run(&mut connection).unwrap();

        let columns: Vec<String> = connection.prepare("PRAGMA table_info(transactions)")
            .unwrap().query_map([], |row| row.get(1)).unwrap()
            .collect::<Result<Vec<_>, _>>().unwrap();
        assert!(columns.iter().any(|c| c == "attempt_count"));
        assert!(columns.iter().any(|c| c == "last_error"));
        assert!(!columns.iter().any(|c| c == "retry_count"));

        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
        assert_eq!(version, 6);
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
            "gmail_sync_state",
            "gmail_messages",
            "gmail_attachments",
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

        assert_eq!(version, 6);
    }

    #[test]
    fn incremental_sync_status_is_allowed_after_migration() {
        let connection = create_test_database();
        connection
            .execute(
                "INSERT INTO gmail_accounts (email_address) VALUES ('sync@gmail.com')",
                [],
            )
            .unwrap();

        let result = connection.execute(
            "INSERT INTO gmail_sync_state (gmail_account_id, sync_status)
             VALUES (1, 'incremental_sync')",
            [],
        );

        assert!(result.is_ok());
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
