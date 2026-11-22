/// Word breaks, not substrings: `all` does not hit `allow` in
/// `transfer_allow_death`, and a fragment of a pallet name does not select
/// the pallet's children.
pub fn tokenize(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;

    core::iter::from_fn(move || {
        rest = rest.trim_start_matches(|c: char| !c.is_alphanumeric());

        if rest.is_empty() {
            return None;
        }

        let (token, tail) = rest.split_at(token_end(rest));
        rest = tail;

        Some(token)
    })
}

/// Length in bytes of the word starting `text`, which must begin with an alphanumeric.
fn token_end(text: &str) -> usize {
    let mut chars = text.char_indices();
    let mut prev = chars.next().expect("text starts with an alphanumeric").1;

    for (i, c) in chars {
        if !c.is_alphanumeric() {
            return i;
        }

        if c.is_uppercase() && !prev.is_uppercase() {
            return i;
        }

        // `XCMPQueue` -> `XCMP`, `Queue`: a run of capitals belongs to the word before,
        // except for the last one when a lowercase letter follows it.
        if prev.is_uppercase()
            && c.is_uppercase()
            && text[i + c.len_utf8()..]
                .chars()
                .next()
                .is_some_and(char::is_lowercase)
        {
            return i;
        }

        prev = c;
    }

    text.len()
}

#[cfg(test)]
mod tests {
    use super::tokenize;

    fn split(text: &str) -> Vec<&str> {
        tokenize(text).collect()
    }

    #[test]
    fn splits_on_underscores() {
        assert_eq!(
            split("transfer_allow_death"),
            ["transfer", "allow", "death"]
        );
    }

    #[test]
    fn splits_on_case_boundaries() {
        assert_eq!(split("ExistentialDeposit"), ["Existential", "Deposit"]);
        assert_eq!(split("transferAll"), ["transfer", "All"]);
    }

    #[test]
    fn splits_run_of_capitals_before_lowercase_word() {
        assert_eq!(split("XCMPQueue"), ["XCMP", "Queue"]);
        assert_eq!(split("XCMP"), ["XCMP"]);
    }

    #[test]
    fn keeps_digits_with_their_word() {
        assert_eq!(split("AccountId32"), ["Account", "Id32"]);
        assert_eq!(split("transfer_v2"), ["transfer", "v2"]);
    }

    #[test]
    fn drops_punctuation() {
        assert_eq!(
            split("Transfer succeeded. See `transfer_all`."),
            ["Transfer", "succeeded", "See", "transfer", "all"]
        );
    }

    #[test]
    fn yields_nothing_without_alphanumerics() {
        assert!(split("  _ -- ").is_empty());
    }
}
