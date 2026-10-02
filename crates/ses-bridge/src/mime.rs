/// The subset of RFC 5322 headers the bridge cares about.
///
/// Parsing is deliberately shallow: this only needs to extract envelope-ish
/// metadata for the queue message. It does not decode MIME parts or transfer
/// encodings, so `subject` may still be RFC 2047 encoded in the queue payload.
#[derive(Debug, Default)]
pub struct ParsedEmail {
    pub from: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: Option<String>,
    pub date: Option<String>,
    pub header_message_id: Option<String>,
    pub delivered_to: Vec<String>,
    pub x_original_to: Vec<String>,
}

impl ParsedEmail {
    /// Recipients most likely to reflect the SMTP envelope, falling back to
    /// the message headers when no delivery headers are present.
    pub fn recipients(&self) -> Vec<String> {
        let mut all = Vec::new();
        for group in [&self.delivered_to, &self.x_original_to, &self.to, &self.cc] {
            for value in group {
                if !all.iter().any(|existing| existing == value) {
                    all.push(value.clone());
                }
            }
        }
        all
    }
}

pub fn parse(raw: &[u8]) -> ParsedEmail {
    let mut parsed = ParsedEmail::default();

    for (name, value) in headers(raw) {
        match name.as_str() {
            "from" => parsed.from = Some(value),
            "to" => parsed.to = split_addresses(&value),
            "cc" => parsed.cc = split_addresses(&value),
            "subject" => parsed.subject = Some(value),
            "date" => parsed.date = Some(value),
            "message-id" => parsed.header_message_id = Some(value),
            "delivered-to" => parsed.delivered_to = split_addresses(&value),
            "x-original-to" => parsed.x_original_to = split_addresses(&value),
            _ => {}
        }
    }

    parsed
}

/// Yields unfolded `(lowercase name, value)` header pairs up to the empty line
/// separating headers from the body.
fn headers(raw: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(raw);
    let header_block = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .map(|(head, _)| head)
        .unwrap_or(&text);

    let mut result = Vec::new();
    let mut current: Option<(String, String)> = None;

    for line in header_block.lines() {
        let line = line.trim_end_matches('\r');

        // A leading whitespace character marks a continuation of the prior header.
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some((_, value)) = current.as_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }

        if let Some(entry) = current.take() {
            result.push(entry);
        }

        if let Some((name, value)) = line.split_once(':') {
            current = Some((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }

    if let Some(entry) = current.take() {
        result.push(entry);
    }

    result
}

/// Splits a comma-separated address list and keeps only the bare address.
fn split_addresses(value: &str) -> Vec<String> {
    value
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let address = match (part.rfind('<'), part.rfind('>')) {
                (Some(open), Some(close)) if open < close => part[open + 1..close].trim(),
                _ => part,
            };
            Some(address.to_lowercase())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_headers() {
        let raw = b"From: Alice <alice@example.com>\r\n\
To: Bob <bob@example.com>, carol@example.com\r\n\
Subject: hello\r\n\
Delivered-To: catch@sub.example.com\r\n\
\r\n\
body";
        let parsed = parse(raw);
        assert_eq!(parsed.from.as_deref(), Some("Alice <alice@example.com>"));
        assert_eq!(parsed.to, vec!["bob@example.com", "carol@example.com"]);
        assert_eq!(parsed.subject.as_deref(), Some("hello"));
        assert_eq!(
            parsed.recipients(),
            vec![
                "catch@sub.example.com",
                "bob@example.com",
                "carol@example.com"
            ]
        );
    }

    #[test]
    fn unfolds_continuation_lines() {
        let raw = b"To: bob@example.com,\r\n\tcarol@example.com\n\n";
        let parsed = parse(raw);
        assert_eq!(parsed.to, vec!["bob@example.com", "carol@example.com"]);
    }
}
