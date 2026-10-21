use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::types::Value;

/// Converts epoch seconds or milliseconds to RFC 3339 so timestamps from different
/// client databases sort consistently as strings.
pub fn epoch_to_rfc3339(raw: f64) -> Option<String> {
    if !raw.is_finite() || raw <= 0.0 {
        return None;
    }
    let millis = if raw > 1e12 { raw } else { raw * 1000.0 };
    Utc.timestamp_millis_opt(millis as i64)
        .single()
        .map(|dt| dt.to_rfc3339())
}

/// Normalizes the timestamp formats used by Codex, Claude and Antigravity to RFC 3339.
/// Unrecognized text is returned unchanged.
pub fn normalize_timestamp(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return dt.with_timezone(&Utc).to_rfc3339();
    }
    if let Ok(number) = trimmed.parse::<f64>() {
        if let Some(text) = epoch_to_rfc3339(number) {
            return text;
        }
    }
    for format in ["%Y-%m-%d %H:%M:%S%.f%:z", "%Y-%m-%d %H:%M:%S%.f%z"] {
        if let Ok(dt) = DateTime::parse_from_str(trimmed, format) {
            return dt.with_timezone(&Utc).to_rfc3339();
        }
    }
    for format in ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, format) {
            return Utc.from_utc_datetime(&naive).to_rfc3339();
        }
    }
    trimmed.to_string()
}

/// Reads a SQLite value of any storage class as an RFC 3339 timestamp.
pub fn timestamp_from_value(value: &Value) -> Option<String> {
    match value {
        Value::Integer(i) => epoch_to_rfc3339(*i as f64),
        Value::Real(f) => epoch_to_rfc3339(*f),
        Value::Text(s) => Some(normalize_timestamp(s)).filter(|s| !s.is_empty()),
        _ => None,
    }
}

/// Reads a SQLite value as text, accepting numbers stored in text columns.
pub fn text_from_value(value: &Value) -> Option<String> {
    match value {
        Value::Text(s) => Some(s.clone()),
        Value::Integer(i) => Some(i.to_string()),
        Value::Real(f) => Some(f.to_string()),
        _ => None,
    }
}

pub fn first_non_empty(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty())
}

/// Decodes `%XX` escapes as UTF-8; invalid escapes are kept literally.
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(high * 16 + low);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Encodes a path segment the way VS Code-based clients write `file://` URIs.
pub fn percent_encode_path(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

/// Strips the `\\?\` prefix and converts separators so Explorer and VS Code accept the path.
pub fn normalize_windows_path(path: &str) -> String {
    let trimmed = path.trim().trim_matches('"');
    let without_prefix = trimmed
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{}", rest))
        .or_else(|| trimmed.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or_else(|| trimmed.to_string());
    if cfg!(windows) {
        without_prefix.replace('/', "\\")
    } else {
        without_prefix
    }
}

/// Produces a file name that Windows accepts, keeping the original text where possible.
pub fn sanitize_file_name(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut result: String = collapsed.chars().take(120).collect();
    result = result.trim_end_matches(['.', ' ']).trim().to_string();
    if result.is_empty() {
        result = fallback.to_string();
    }
    let stem = result.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        result = format!("_{}", result);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_normalize_to_rfc3339() {
        assert_eq!(
            normalize_timestamp("2026-02-02 08:54:59.6892244+00:00"),
            "2026-02-02T08:54:59.689224400+00:00"
        );
        assert_eq!(
            timestamp_from_value(&Value::Integer(1_781_576_266)).unwrap(),
            "2026-06-16T02:17:46+00:00"
        );
        assert_eq!(
            timestamp_from_value(&Value::Integer(1_781_576_266_000)).unwrap(),
            "2026-06-16T02:17:46+00:00"
        );
        assert_eq!(normalize_timestamp("not a date"), "not a date");
    }

    #[test]
    fn percent_coding_round_trips_unicode_paths() {
        let path = "g:/工作/update ui_0326";
        let encoded = percent_encode_path(path);
        assert_eq!(encoded, "g%3A/%E5%B7%A5%E4%BD%9C/update%20ui_0326");
        assert_eq!(percent_decode(&encoded), path);
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn windows_paths_are_normalized() {
        assert_eq!(normalize_windows_path(r"\\?\C:\work\demo"), r"C:\work\demo");
        if cfg!(windows) {
            assert_eq!(normalize_windows_path("G:/web/demo"), r"G:\web\demo");
        }
    }

    #[test]
    fn file_names_are_safe_on_windows() {
        assert_eq!(sanitize_file_name("a/b:c?", "x"), "a_b_c_");
        assert_eq!(sanitize_file_name("line\nbreak.", "x"), "line break");
        assert_eq!(sanitize_file_name("CON", "x"), "_CON");
        assert_eq!(sanitize_file_name("   ", "conversation"), "conversation");
        assert_eq!(sanitize_file_name(&"长".repeat(300), "x").chars().count(), 120);
    }
}
