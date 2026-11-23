//! Duplicate transaction merging. Complementary duplicate records merge automatically;
//! identical or conflicting records remain reviewable and require user confirmation.
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use serde::Serialize;
use crate::databases::Database;

#[derive(Debug, Serialize)]
struct MergeSnapshot {
    id: i64,
    flip_transaction_id: Option<String>,
    transaction_date: Option<String>,
    recipient_name: Option<String>,
    amount: Option<i64>,
    transfer_note: Option<String>,
    status: String,
    gmail_account_id: i64,
    gmail_message_id: String,
    created_at: String,
}
#[derive(Clone)]
struct RowData { id: i64, date: Option<String>, recipient: Option<String>, amount: Option<i64>, note: Option<String> }
fn clean(v: Option<String>) -> Option<String> { v.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()) }
fn merge_field<T: PartialEq + Clone>(master: &Option<T>, other: &Option<T>) -> Result<Option<T>, ()> {
    match (master, other) { (Some(a), Some(b)) if a != b => Err(()), (Some(a), _) => Ok(Some(a.clone())), (_, Some(b)) => Ok(Some(b.clone())), _ => Ok(None) }
}
fn read_rows(tx: &Transaction<'_>, flip_id: &str) -> Result<Vec<RowData>, String> {
    let mut stmt = tx.prepare("SELECT id, transaction_date, recipient_name, amount, transfer_note, status FROM transactions WHERE flip_transaction_id=?1 AND merged_into_id IS NULL ORDER BY id ASC").map_err(|e| e.to_string())?;
    let mapped = stmt.query_map([flip_id], |r| Ok(RowData { id:r.get(0)?, date:clean(r.get(1)?), recipient:clean(r.get(2)?), amount:r.get(3)?, note:clean(r.get(4)?) }))
        .map_err(|e|e.to_string())?;
    mapped.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
}
fn store_child_snapshot(tx: &Transaction<'_>, child_id: i64, master_id: i64, merge_type: &str) -> Result<(), String> {
    let snapshot: MergeSnapshot = tx.query_row("SELECT id, flip_transaction_id, transaction_date, recipient_name, amount, transfer_note, status, gmail_account_id, gmail_message_id, created_at FROM transactions WHERE id=?1", [child_id], |r| Ok(MergeSnapshot { id:r.get(0)?, flip_transaction_id:r.get(1)?, transaction_date:r.get(2)?, recipient_name:r.get(3)?, amount:r.get(4)?, transfer_note:r.get(5)?, status:r.get(6)?, gmail_account_id:r.get(7)?, gmail_message_id:r.get(8)?, created_at:r.get(9)? })).map_err(|e|e.to_string())?;
    let json=serde_json::to_string(&snapshot).map_err(|e|e.to_string())?;
    tx.execute("INSERT INTO transaction_merge_history(master_transaction_id,source_transaction_id,source_snapshot_json,merge_type) VALUES(?1,?2,?3,?4)", params![master_id,child_id,json,merge_type]).map_err(|e|e.to_string())?;
    tx.execute("UPDATE transactions SET merged_into_id=?1, updated_at=CURRENT_TIMESTAMP WHERE id=?2",params![master_id,child_id]).map_err(|e|e.to_string())?;
    Ok(())
}
fn merge_group(tx: &Transaction<'_>, rows: &[RowData], merge_type: &str, auto_only: bool) -> Result<bool, String> {
    if rows.len()<2 { return Ok(false); }
    let master=&rows[0];
    let mut date=master.date.clone(); let mut recipient=master.recipient.clone(); let mut amount=master.amount; let mut note=master.note.clone();
    let mut has_missing=false;
    for child in rows.iter().skip(1) {
        has_missing |= date.is_none() && child.date.is_some() || recipient.is_none() && child.recipient.is_some() || amount.is_none() && child.amount.is_some() || note.is_none() && child.note.is_some();
        date=merge_field(&date,&child.date).or_else(|_| if auto_only { Err(()) } else { Ok(date.clone()) }).map_err(|_|"Konflik tanggal transaksi".to_string())?;
        recipient=merge_field(&recipient,&child.recipient).or_else(|_| if auto_only { Err(()) } else { Ok(recipient.clone()) }).map_err(|_|"Konflik nama penerima".to_string())?;
        amount=merge_field(&amount,&child.amount).or_else(|_| if auto_only { Err(()) } else { Ok(amount) }).map_err(|_|"Konflik nominal".to_string())?;
        note=merge_field(&note,&child.note).or_else(|_| if auto_only { Err(()) } else { Ok(note.clone()) }).map_err(|_|"Konflik catatan transfer".to_string())?;
    }
    if auto_only && !has_missing { return Ok(false); }
    // Save child source rows before soft-hiding them; source email metadata is retained.
    for child in rows.iter().skip(1) { store_child_snapshot(tx,child.id,master.id,merge_type)?; }
    let required_complete=date.is_some() && recipient.is_some() && amount.is_some();
    let status=if required_complete {"Completed"} else {"Needs Review"};
    let reason=if required_complete {None} else {Some("Merge dilakukan; field transaksi wajib masih belum lengkap.")};
    tx.execute("UPDATE transactions SET transaction_date=?1,recipient_name=?2,amount=?3,transfer_note=?4,status=?5,review_reason=?6,last_error=NULL,updated_at=CURRENT_TIMESTAMP WHERE id=?7",params![date,recipient,amount,note,status,reason,master.id]).map_err(|e|e.to_string())?;
    tx.execute("INSERT INTO processing_logs(transaction_id,process_type,status,error_message) VALUES(?1,?2,?3,?4)",params![master.id,merge_type,status,reason]).map_err(|e|e.to_string())?;
    Ok(true)
}
/// Called during duplicate detection; automatically merges only complementary, non-conflicting data.
pub fn auto_merge_complementary(tx: &Transaction<'_>, flip_id: &str) -> Result<bool, String> {
    let rows=read_rows(tx,flip_id)?;
    match merge_group(tx,&rows,"auto-merge",true) { Ok(result)=>Ok(result), Err(_conflict)=>Ok(false) }
}
/// User-confirmed merge: uses the oldest record as master, fills blanks, and keeps master values on conflicts.
pub fn merge_duplicate_group(database: &Database, transaction_id: i64) -> Result<i64, String> {
    let mut connection=database.connection.lock().map_err(|e|e.to_string())?;
    let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e|e.to_string())?;
    let flip_id: Option<String>=tx.query_row("SELECT flip_transaction_id FROM transactions WHERE id=?1 AND merged_into_id IS NULL",[transaction_id],|r|r.get(0)).optional().map_err(|e|e.to_string())?.flatten();
    let flip_id=flip_id.ok_or("Transaksi tidak ditemukan atau tidak memiliki Flip Transaction ID.")?;
    let rows=read_rows(&tx,&flip_id)?;
    if rows.len()<2 { return Err("Tidak ditemukan grup duplikat yang dapat digabung.".into()); }
    let master_id=rows[0].id;
    merge_group(&tx,&rows,"manual-merge",false)?;
    tx.commit().map_err(|e|e.to_string())?;
    Ok(master_id)
}
