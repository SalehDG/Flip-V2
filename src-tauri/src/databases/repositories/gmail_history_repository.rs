use rusqlite::{Connection, Result};
use serde::Serialize;
use std::{collections::HashMap, path::Path};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailHistoryAttachment {
    pub id: i64,
    pub filename: Option<String>,
    pub declared_mime_type: Option<String>,
    pub detected_format: String,
    pub file_size: i64,
    pub sha256: String,
    pub local_path: String,
    pub file_exists: bool,
    pub created_at: String,
}


#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailHistoryEmail {
    pub account_id: i64,
    pub account_email: String,
    pub message_id: String,
    pub thread_id: String,
    pub internal_date: Option<String>,
    pub sender: Option<String>,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub retrieved_at: String,
    pub attachments: Vec<GmailHistoryAttachment>,
}


pub struct GmailHistoryRepository;


impl GmailHistoryRepository {
    pub fn find_all(connection: &Connection, account_id: Option<i64>) -> Result<Vec<GmailHistoryEmail>> {
        let mut statement = connection.prepare(
            "SELECT m.gmail_account_id, a.email_address, m.gmail_message_id, m.gmail_thread_id,
                    m.internal_date, m.sender, m.subject, m.snippet, m.retrieved_at,
                    att.id, att.filename, att.declared_mime_type, att.detected_format,
                    att.file_size, att.sha256, att.local_path, att.created_at
             FROM gmail_messages m
             JOIN gmail_accounts a ON a.id = m.gmail_account_id
             LEFT JOIN gmail_attachments att
               ON att.gmail_account_id = m.gmail_account_id
              AND att.gmail_message_id = m.gmail_message_id
             WHERE (?1 IS NULL OR m.gmail_account_id = ?1)
             ORDER BY m.internal_date DESC, m.id DESC, att.id ASC"
        )?;
        let rows = statement.query_map([account_id], |row| {
            Ok((
                row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
                row.get::<_, String>(3)?, row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?, row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?, row.get::<_, String>(8)?,
                row.get::<_, Option<i64>>(9)?, row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?, row.get::<_, Option<String>>(12)?,
                row.get::<_, Option<i64>>(13)?, row.get::<_, Option<String>>(14)?,
                row.get::<_, Option<String>>(15)?, row.get::<_, Option<String>>(16)?,
            ))
        })?;

        let mut grouped: HashMap<(i64, String), GmailHistoryEmail> = HashMap::new();
        let mut order: Vec<(i64, String)> = Vec::new();
        for row in rows {
            let row = row?;
            let key = (row.0, row.2.clone());
            if !grouped.contains_key(&key) {
                order.push(key.clone());
                grouped.insert(key.clone(), GmailHistoryEmail {
                    account_id: row.0, account_email: row.1.clone(), message_id: row.2.clone(),
                    thread_id: row.3.clone(), internal_date: row.4.clone(), sender: row.5.clone(),
                    subject: row.6.clone(), snippet: row.7.clone(), retrieved_at: row.8.clone(),
                    attachments: Vec::new(),
                });
            }
            if let Some(attachment_id) = row.9 {
                let path = row.15.unwrap_or_default();
                let exists = Path::new(&path).is_file();
                if let Some(email) = grouped.get_mut(&key) {
                    email.attachments.push(GmailHistoryAttachment {
                        id: attachment_id,
                        filename: row.10,
                        declared_mime_type: row.11,
                        detected_format: row.12.unwrap_or_default(),
                        file_size: row.13.unwrap_or_default(),
                        sha256: row.14.unwrap_or_default(),
                        local_path: path,
                        file_exists: exists,
                        created_at: row.16.unwrap_or_default(),
                    });
                }
            }
        }
        Ok(order.into_iter().filter_map(|key| grouped.remove(&key)).collect())
    }

}
