/// Result status for the OCR-only proof of concept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrExtractionStatus {
    Completed,
    NeedReview,
}


/// Parsed transfer note and OCR evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferNoteResult {
    pub transfer_note: Option<String>,
    pub raw_text: String,
    pub status: OcrExtractionStatus,
    pub review_reason: Option<String>,
}


const REMARK_LABEL: &str = "catatan transfer";
const NEXT_FIELD_LABELS: [&str; 3] = ["pengirim", "nama pengirim", "sender"];


/// Extracts only the value following the invoice's `Catatan Transfer` label.
/// No guessed/fallback value is returned when the label is missing or blank.
///
/// Matching is case-insensitive, accepts common OCR whitespace/separator variation,
/// and stops at a known following field if it appears on the same line.
pub fn parse_transfer_note(raw_text: String) -> TransferNoteResult {
    let note = raw_text.lines().find_map(|line| {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if !lower.starts_with(REMARK_LABEL) {
            return None;
        }

        // Ensure the label isn't merely the prefix of a different word.
        let after_label = trimmed.get(REMARK_LABEL.len()..)?;
        if after_label.chars().next().is_some_and(|ch| ch.is_alphanumeric()) {
            return None;
        }

        let value = after_label.trim_start_matches([' ', ':', '-', '\t']);
        let value_lower = value.to_ascii_lowercase();
        let cut_at = NEXT_FIELD_LABELS
            .iter()
            .filter_map(|label| value_lower.find(label))
            .min()
            .unwrap_or(value.len());
        let cleaned = value[..cut_at].trim();
        (!cleaned.is_empty()).then(|| cleaned.to_owned())
    });

    match note {
        Some(value) => TransferNoteResult {
            transfer_note: Some(value),
            raw_text,
            status: OcrExtractionStatus::Completed,
            review_reason: None,
        },
        None => TransferNoteResult {
            transfer_note: None,
            raw_text,
            status: OcrExtractionStatus::NeedReview,
            review_reason: Some("Catatan Transfer tidak ditemukan atau kosong pada hasil OCR.".into()),
        },
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_note_with_spaces() {
        let result = parse_transfer_note("Catatan Transfer BG17 tahu jabon\nPengirim test".into());
        assert_eq!(result.transfer_note.as_deref(), Some("BG17 tahu jabon"));
        assert_eq!(result.status, OcrExtractionStatus::Completed);
    }

    #[test]
    fn parses_note_with_colon_and_mixed_case() {
        let result = parse_transfer_note("CATATAN TRANSFER: DP12 ayam cimanggis".into());
        assert_eq!(result.transfer_note.as_deref(), Some("DP12 ayam cimanggis"));
    }

    #[test]
    fn parses_note_with_dash_separator() {
        let result = parse_transfer_note("Catatan Transfer - TR27 telur lemo".into());
        assert_eq!(result.transfer_note.as_deref(), Some("TR27 telur lemo"));
    }

    #[test]
    fn stops_before_sender_field_on_same_line() {
        let result = parse_transfer_note("Catatan Transfer: DP19 tahu sawangan Pengirim: Abdul".into());
        assert_eq!(result.transfer_note.as_deref(), Some("DP19 tahu sawangan"));
    }

    #[test]
    fn rejects_label_prefix_of_other_word() {
        let result = parse_transfer_note("Catatan Transferan: bukan remark\nPengirim test".into());
        assert_eq!(result.transfer_note, None);
        assert_eq!(result.status, OcrExtractionStatus::NeedReview);
    }

    #[test]
    fn missing_note_requires_review() {
        let result = parse_transfer_note("Transaksi berhasil diproses\nPengirim test".into());
        assert_eq!(result.transfer_note, None);
        assert_eq!(result.status, OcrExtractionStatus::NeedReview);
        assert!(result.review_reason.is_some());
    }

    #[test]
    fn blank_note_requires_review() {
        let result = parse_transfer_note("Catatan Transfer   \nPengirim test".into());
        assert_eq!(result.transfer_note, None);
        assert_eq!(result.status, OcrExtractionStatus::NeedReview);
    }
}
