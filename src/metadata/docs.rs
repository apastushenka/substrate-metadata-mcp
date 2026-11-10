//! Documentation as the tools hand it on. Every doc text in a response goes through here,
//! so pallets, runtime APIs, items and type fields all read alike.

const SUMMARY_CHARS: usize = 200;

/// The first non-blank line, cut after `SUMMARY_CHARS` characters, or `None` if every line is
/// blank. The `summary` fields of the responses promise this in their descriptions, so change
/// them together.
pub fn summary(docs: &[String]) -> Option<String> {
    let line = docs
        .iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())?;

    match line.char_indices().nth(SUMMARY_CHARS) {
        Some((end, _)) => Some(format!("{}...", &line[..end])),
        None => Some(String::from(line)),
    }
}

/// FRAME keeps the space after `///`; rustdoc strips exactly one, and so do we. `trim_start`
/// would also eat code-block indentation, and empty lines stay to separate paragraphs.
pub fn full(docs: &[String]) -> Vec<String> {
    docs.iter()
        .map(|line| match line.strip_prefix(' ') {
            Some(rest) => String::from(rest),
            None => line.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_strips_one_leading_space() {
        let docs = [" Transfer funds.", "No leading space", "  indented", ""].map(String::from);

        assert_eq!(
            full(&docs),
            ["Transfer funds.", "No leading space", " indented", "",]
        );
    }

    #[test]
    fn summary_skips_blank_lines_and_trims() {
        let docs = ["", "  ", "  Transfer funds.  "].map(String::from);

        assert_eq!(summary(&docs).as_deref(), Some("Transfer funds."));
    }

    #[test]
    fn summary_is_absent_without_text() {
        assert_eq!(summary(&[]), None);
        assert_eq!(summary(&["  ".into(), "\t".into()]), None);
    }

    #[test]
    fn summary_keeps_line_at_limit_whole() {
        let line = "a".repeat(SUMMARY_CHARS);

        assert_eq!(
            summary(core::slice::from_ref(&line)).as_deref(),
            Some(line.as_str())
        );
    }

    #[test]
    fn summary_truncates_past_limit() {
        let line = "я".repeat(SUMMARY_CHARS + 1);

        assert_eq!(
            summary(core::slice::from_ref(&line)),
            Some(format!("{}...", "я".repeat(SUMMARY_CHARS)))
        );
    }
}
