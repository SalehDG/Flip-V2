//! Extracts transaction fields from Flip's email body.
//! Subject text is intentionally not used as a data fallback.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailTransactionFields {
    pub transaction_id: Option<String>,
    pub transaction_date: Option<String>, // DD/MM/YYYY
    pub recipient_name: Option<String>,
    pub amount: Option<i64>, // integer Rupiah
    pub review_reasons: Vec<String>,
}


pub fn parse_flip_email(_subject: Option<&str>, decoded_body: &str) -> EmailTransactionFields {
    let normalized = normalize_body(decoded_body);
    let transaction_id = extract_transaction_id(&normalized);
    let transaction_date = extract_sent_date(&normalized);
    let recipient_name = extract_recipient(&normalized);
    let amount = extract_amount(&normalized);

    let mut review_reasons = Vec::new();
    if transaction_id.is_none() { review_reasons.push("Transaction ID tidak ditemukan.".to_string()); }
    if transaction_date.is_none() { review_reasons.push("Waktu Terkirim tidak ditemukan atau tanggal tidak valid.".to_string()); }
    if recipient_name.is_none() { review_reasons.push("Nama Tujuan tidak ditemukan.".to_string()); }
    if amount.is_none() { review_reasons.push("Nominal transfer tidak ditemukan atau ambigu.".to_string()); }

    EmailTransactionFields { transaction_id, transaction_date, recipient_name, amount, review_reasons }
}


fn normalize_body(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}


fn extract_transaction_id(text: &str) -> Option<String> {
    let tail = find_after_label(text, &["ID Transaksi", "Transaction ID", "ID Transaction"])?;
    let token = tail.trim_start_matches(['#', ':', '-', ' '])
        .split_whitespace().next()?
        .trim_matches(|c: char| !c.is_ascii_alphanumeric());
    let id = token.trim();
    (id.starts_with("ST") && id.len() >= 8 && id.chars().all(|c| c.is_ascii_alphanumeric()))
        .then(|| id.to_string())
}


fn extract_sent_date(text: &str) -> Option<String> {
    let tail = find_after_label(text, &["Waktu Terkirim"])?;
    let first_line = tail.lines().next()?.trim();
    let tokens: Vec<&str> = first_line.split_whitespace().collect();
    if tokens.is_empty() { return None; }

    let (day, month, year) = if let Some((d, m, y)) = parse_numeric_date(tokens[0]) {
        (d, m, y)
    } else {
        if tokens.len() < 3 { return None; }
        let day = tokens[0].parse::<u32>().ok()?;
        let month = month_number(tokens[1])?;
        let year = tokens[2].parse::<u32>().ok()?;
        (day, month, year)
    };

    if !valid_date(day, month, year) { return None; }
    Some(format!("{day:02}/{month:02}/{year:04}"))
}


fn parse_numeric_date(value: &str) -> Option<(u32, u32, u32)> {
    let separator = if value.contains('/') { '/' } else if value.contains('-') { '-' } else { return None; };
    let mut parts = value.split(separator);
    let day = parts.next()?.parse::<u32>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let year = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() { return None; }
    Some((day, month, year))
}

fn valid_date(day: u32, month: u32, year: u32) -> bool {
    if year < 2000 || month == 0 || month > 12 || day == 0 { return false; }
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => return false,
    };
    day <= max_day
}

fn month_number(month: &str) -> Option<u32> {
    match month.trim_matches(|c: char| !c.is_alphabetic()).to_ascii_lowercase().as_str() {
        "jan" | "januari" => Some(1), "feb" | "februari" => Some(2),
        "mar" | "maret" => Some(3), "apr" | "april" => Some(4),
        "mei" | "may" => Some(5), "jun" | "juni" => Some(6),
        "jul" | "juli" => Some(7), "agu" | "agustus" | "aug" => Some(8),
        "sep" | "september" => Some(9), "okt" | "oktober" | "oct" => Some(10),
        "nov" | "november" => Some(11), "des" | "desember" | "dec" => Some(12),
        _ => None,
    }
}

fn extract_recipient(text: &str) -> Option<String> {
    let value = find_after_label(text, &["Nama Tujuan"])?;
    let line = value.lines().next()?.trim();
    let cleaned = trim_before_next_label(line, &["Bank Tujuan", "Bank Penerima", "Nomor Rekening", "Catatan Transfer", "ID Transaksi", "Waktu Terkirim"]);
    let cleaned = cleaned.trim().trim_matches([':', '-', ' ']);
    (!cleaned.is_empty()).then(|| cleaned.to_string())
}


fn extract_amount(text: &str) -> Option<i64> {
    let lines: Vec<&str> = text.lines().collect();
    // Prefer a currency amount on a line explicitly describing the transfer amount.
    for line in &lines {
        let lower = line.to_ascii_lowercase();
        if ["nominal", "jumlah transfer", "total transfer", "jumlah"].iter().any(|label| lower.contains(label)) {
            let candidates = parse_rp_candidates(line);
            if candidates.len() == 1 { return candidates.first().copied(); }
        }
    }
    // If the message contains a single Rp amount, it is unambiguous. Multiple amounts
    // without a labeled amount field are deliberately not guessed.
    let all: Vec<i64> = lines.iter().flat_map(|line| parse_rp_candidates(line)).collect();
    let unique: Vec<i64> = all.into_iter().fold(Vec::new(), |mut acc, value| {
        if !acc.contains(&value) { acc.push(value); }
        acc
    });
    (unique.len() == 1).then(|| unique[0])
}


fn parse_rp_candidates(line: &str) -> Vec<i64> {
    let lower = line.to_ascii_lowercase();
    let mut results = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = lower[cursor..].find("rp") {
        let start = cursor + relative + 2;
        let tail = line[start..].trim_start();
        let numeric: String = tail.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',' || *c == ' ').collect();
        let digits: String = numeric.chars().filter(char::is_ascii_digit).collect();
        if let Ok(value) = digits.parse::<i64>() {
            if value > 0 { results.push(value); }
        }
        cursor = start;
        if cursor >= lower.len() { break; }
    }
    results
}

fn find_after_label(text: &str, labels: &[&str]) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let (index, label) = labels.iter().filter_map(|label| lower.find(&label.to_ascii_lowercase()).map(|idx| (idx, *label)))
        .min_by_key(|(idx, _)| *idx)?;
    let tail = text.get(index + label.len()..)?.trim_start();
    let tail = tail.trim_start_matches([':', '-', '#']).trim_start();
    if tail.is_empty() { return None; }
    Some(tail.to_owned())
}

fn trim_before_next_label<'a>(value: &'a str, labels: &[&str]) -> &'a str {
    let lower = value.to_ascii_lowercase();
    labels.iter().filter_map(|label| lower.find(&label.to_ascii_lowercase())).min()
        .map(|idx| value[..idx].trim()).unwrap_or(value)
}


#[cfg(test)]
mod tests {
    use super::*;
    const BODY: &str = "BUKTI TRANSFER\nID Transaksi #ST261121144802248WRXVK\nWaktu Terkirim 21 Nov 2026 14:48 WIB\nNama Tujuan EDDY KURNIAWAN\nRp1.210.000";

    #[test]
    fn parses_sample_fields_without_subject_fallback() {
        let parsed = parse_flip_email(Some("untrusted subject"), BODY);
        assert_eq!(parsed.transaction_id.as_deref(), Some("ST261121144802248WRXVK"));
        assert_eq!(parsed.transaction_date.as_deref(), Some("21/11/2026"));
        assert_eq!(parsed.recipient_name.as_deref(), Some("EDDY KURNIAWAN"));
        assert_eq!(parsed.amount, Some(1_210_000));
        assert!(parsed.review_reasons.is_empty());
    }

    #[test]
    fn missing_recipient_is_not_recovered_from_subject() {
        let parsed = parse_flip_email(Some("Transfer ke EDDY KURNIAWAN berhasil"), "ID Transaksi #ST12345678\nWaktu Terkirim 21 Nov 2026 14:48 WIB\nRp100.000");
        assert_eq!(parsed.recipient_name, None);
        assert!(parsed.review_reasons.iter().any(|reason| reason.contains("Nama Tujuan")));
    }

    #[test]
    fn rejects_invalid_calendar_date() {
        assert_eq!(extract_sent_date("Waktu Terkirim 31 Feb 2026 12:00 WIB"), None);
        assert_eq!(extract_sent_date("Waktu Terkirim 29 Feb 2024 12:00 WIB"), Some("29/02/2024".into()));
    }

    #[test]
    fn supports_numeric_date_format() {
        assert_eq!(extract_sent_date("Waktu Terkirim 21/11/2026 14:48 WIB"), Some("21/11/2026".into()));
    }

    #[test]
    fn ambiguous_amount_requires_review() {
        assert_eq!(extract_amount("Rp100.000\nRp200.000"), None);
        assert_eq!(extract_amount("Nominal transfer Rp200.000\nFee Rp2.000"), Some(200000));
    }
}
