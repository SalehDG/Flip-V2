use rusqlite::{Connection, OptionalExtension, Result};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailAccount {
    pub id: i64,
    pub email_address: String,
    pub history_id: Option<String>,
    pub sync_start_date: Option<String>,
    pub last_sync_at: Option<String>,
    pub account_status: String,
    pub created_at: String,
    pub updated_at: String,
}

pub struct GmailAccountRepository;

impl GmailAccountRepository {
    /// Menambahkan akun Gmail baru.
    /// Email bersifat unik tanpa membedakan huruf besar/kecil.
    pub fn create(
        connection: &Connection,
        email_address: &str,
    ) -> Result<i64> {
        connection.execute(
            "INSERT INTO gmail_accounts (email_address)
             VALUES (?1)",
            [email_address.trim()],
        )?;

        Ok(connection.last_insert_rowid())
    }

    /// Create or update a connected Gmail account while preserving its sync state.
    pub fn upsert_connected_account(
        connection: &Connection,
        email_address: &str,
        token_reference: &str,
    ) -> Result<i64> {
        connection.execute(
            "INSERT INTO gmail_accounts (email_address, encrypted_token_reference, account_status)
             VALUES (?1, ?2, 'connected')
             ON CONFLICT(email_address) DO UPDATE SET
                encrypted_token_reference = excluded.encrypted_token_reference,
                account_status = 'connected',
                updated_at = CURRENT_TIMESTAMP",
            (email_address.trim(), token_reference),
        )?;
        connection.query_row(
            "SELECT id FROM gmail_accounts WHERE email_address = ?1 COLLATE NOCASE",
            [email_address.trim()],
            |row| row.get(0),
        )
    }

    /// Retrieve the opaque Windows Credential Manager reference for an account.
    pub fn find_token_reference(connection: &Connection, id: i64) -> Result<Option<String>> {
        connection.query_row(
            "SELECT encrypted_token_reference FROM gmail_accounts WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
    }

    /// Mengambil seluruh akun Gmail.
    pub fn find_all(connection: &Connection) -> Result<Vec<GmailAccount>> {
        let mut statement = connection.prepare(
            "SELECT
                id,
                email_address,
                history_id,
                sync_start_date,
                last_sync_at,
                account_status,
                created_at,
                updated_at
             FROM gmail_accounts
             ORDER BY id ASC",
        )?;

        let rows = statement.query_map([], Self::map_row)?;

        rows.collect()
    }

    /// Mengambil satu akun berdasarkan ID.
    pub fn find_by_id(
        connection: &Connection,
        id: i64,
    ) -> Result<Option<GmailAccount>> {
        connection
            .query_row(
                "SELECT
                    id,
                    email_address,
                    history_id,
                    sync_start_date,
                    last_sync_at,
                    account_status,
                    created_at,
                    updated_at
                 FROM gmail_accounts
                 WHERE id = ?1",
                [id],
                Self::map_row,
            )
            .optional()
    }

    /// Menyimpan tanggal awal sinkronisasi akun.
    pub fn update_sync_start_date(
        connection: &Connection,
        id: i64,
        start_date: &str,
    ) -> Result<bool> {
        let affected = connection.execute(
            "UPDATE gmail_accounts
             SET sync_start_date = ?1,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?2",
            (start_date, id),
        )?;

        Ok(affected > 0)
    }

    /// Memperbarui informasi sinkronisasi akun.
    pub fn update_sync_state(
        connection: &Connection,
        id: i64,
        history_id: Option<&str>,
        last_sync_at: Option<&str>,
    ) -> Result<bool> {
        let affected = connection.execute(
            "UPDATE gmail_accounts
             SET history_id = ?1,
                 last_sync_at = ?2,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?3",
            (history_id, last_sync_at, id),
        )?;

        Ok(affected > 0)
    }

    /// Memperbarui status koneksi akun.
    pub fn update_status(
        connection: &Connection,
        id: i64,
        status: &str,
    ) -> Result<bool> {
        let affected = connection.execute(
            "UPDATE gmail_accounts
             SET account_status = ?1,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?2",
            (status, id),
        )?;

        Ok(affected > 0)
    }

    /// Menghapus referensi token setelah token dihapus
    /// dari secure storage oleh modul autentikasi.
    pub fn clear_token_reference(
        connection: &Connection,
        id: i64,
    ) -> Result<bool> {
        let affected = connection.execute(
            "UPDATE gmail_accounts
             SET encrypted_token_reference = NULL,
                 account_status = 'disconnected',
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1",
            [id],
        )?;

        Ok(affected > 0)
    }

    fn map_row(row: &rusqlite::Row<'_>) -> Result<GmailAccount> {
        Ok(GmailAccount {
            id: row.get("id")?,
            email_address: row.get("email_address")?,
            history_id: row.get("history_id")?,
            sync_start_date: row.get("sync_start_date")?,
            last_sync_at: row.get("last_sync_at")?,
            account_status: row.get("account_status")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let mut connection = Connection::open_in_memory().unwrap();

        connection
            .pragma_update(None, "foreign_keys", "ON")
            .unwrap();

        crate::databases::migrations::run(&mut connection).unwrap();

        connection
    }

    #[test]
    fn creates_gmail_account() {
        let connection = setup();

        let id = GmailAccountRepository::create(
            &connection,
            "test@gmail.com",
        )
        .unwrap();

        let account = GmailAccountRepository::find_by_id(
            &connection,
            id,
        )
        .unwrap()
        .unwrap();

        assert_eq!(account.email_address, "test@gmail.com");
        assert_eq!(account.account_status, "disconnected");
    }

    #[test]
    fn rejects_duplicate_email_address() {
        let connection = setup();

        GmailAccountRepository::create(
            &connection,
            "test@gmail.com",
        )
        .unwrap();

        let result = GmailAccountRepository::create(
            &connection,
            "TEST@gmail.com",
        );

        assert!(result.is_err());
    }

    #[test]
    fn upserts_connected_account_and_preserves_existing_sync_settings() {
        let connection = setup();
        let id = GmailAccountRepository::create(&connection, "test@gmail.com").unwrap();
        GmailAccountRepository::update_sync_start_date(&connection, id, "2026-11-01").unwrap();

        let updated_id = GmailAccountRepository::upsert_connected_account(
            &connection,
            "TEST@gmail.com",
            "gmail-refresh-token:test-reference",
        ).unwrap();

        assert_eq!(id, updated_id);
        let account = GmailAccountRepository::find_by_id(&connection, id).unwrap().unwrap();
        assert_eq!(account.account_status, "connected");
        assert_eq!(account.sync_start_date.as_deref(), Some("2026-11-01"));
        assert_eq!(
            GmailAccountRepository::find_token_reference(&connection, id).unwrap().as_deref(),
            Some("gmail-refresh-token:test-reference"),
        );
    }

    #[test]
    fn finds_all_gmail_accounts() {
        let connection = setup();

        GmailAccountRepository::create(
            &connection,
            "first@gmail.com",
        )
        .unwrap();

        GmailAccountRepository::create(
            &connection,
            "second@gmail.com",
        )
        .unwrap();

        let accounts =
            GmailAccountRepository::find_all(&connection).unwrap();

        assert_eq!(accounts.len(), 2);
    }

    #[test]
    fn updates_sync_start_date() {
        let connection = setup();

        let id = GmailAccountRepository::create(
            &connection,
            "test@gmail.com",
        )
        .unwrap();

        let updated =
            GmailAccountRepository::update_sync_start_date(
                &connection,
                id,
                "2026-11-01",
            )
            .unwrap();

        assert!(updated);

        let account = GmailAccountRepository::find_by_id(
            &connection,
            id,
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            account.sync_start_date.as_deref(),
            Some("2026-11-01")
        );
    }

    #[test]
    fn returns_false_when_account_does_not_exist() {
        let connection = setup();

        let updated =
            GmailAccountRepository::update_status(
                &connection,
                999,
                "connected",
            )
            .unwrap();

        assert!(!updated);
    }
}