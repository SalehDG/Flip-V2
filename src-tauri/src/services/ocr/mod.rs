//! Local OCR service for Flip V2.
//!
//! This module is intentionally isolated from Gmail Sync. It invokes the local
//! Tesseract executable and only extracts the transfer-note field (Remark).

mod remark_parser;
mod tesseract;

pub use remark_parser::{parse_transfer_note, OcrExtractionStatus, TransferNoteResult};
pub use tesseract::{run_tesseract, TesseractOptions};
