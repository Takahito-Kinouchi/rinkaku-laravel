//! Decoding git's C-style path quoting.
//!
//! git does not write every path into a diff header verbatim. When a path
//! contains a quote, a backslash, a control character, or — under the
//! default `core.quotePath=true` — any non-ASCII byte, git wraps it in
//! double quotes and escapes those bytes C-style:
//!
//! ```text
//! diff --git "a/src/\346\227\245\346\234\254\350\252\236.rs" "b/src/\346\227\245\346\234\254\350\252\236.rs"
//! ```
//!
//! A parser that reads the quoted form as a literal path gets a string
//! that names no file at all, so the entry silently drops out of the
//! analysis. That is what this module exists to prevent.
//!
//! **Decoding happens before containment.** The escapes are octal *byte*
//! values, so `"a/\056\056/etc/passwd"` decodes to `a/../etc/passwd` — a
//! path that leaves the repository while spelling nothing suspicious in
//! its quoted form. [`crate::repo_path::is_repo_relative`] must therefore
//! run on the decoded path, never on the raw header text, and
//! [`crate::diff::parse_unified_diff`] is what guarantees that ordering by
//! decoding here, at the point the path enters `ChangedFile`.

use std::borrow::Cow;

/// Decodes a path as it appears in a git diff header.
///
/// An unquoted path is returned as-is ([`Cow::Borrowed`]) — that is the
/// common case, and the overwhelming majority of paths. A quoted path is
/// decoded into its real bytes and returned as [`Cow::Owned`].
///
/// `None` when the quoted form is malformed (no closing quote, a
/// truncated or out-of-range escape, an unknown escape letter) or when the
/// decoded bytes are not valid UTF-8. A caller that gets `None` has a path
/// it cannot act on, and must treat the entry as unusable rather than
/// guessing at what was meant.
pub fn unquote_git_path(value: &str) -> Option<Cow<'_, str>> {
    if !value.starts_with('"') {
        return Some(Cow::Borrowed(value));
    }
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut out: Vec<u8> = Vec::with_capacity(inner.len());
    let mut bytes = inner.bytes();

    while let Some(byte) = bytes.next() {
        if byte != b'\\' {
            // A bare `"` inside the quoted region means the closing quote
            // was not the last character — the token was mis-split.
            if byte == b'"' {
                return None;
            }
            out.push(byte);
            continue;
        }
        let escape = bytes.next()?;
        match escape {
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            b'"' => out.push(b'"'),
            b'\\' => out.push(b'\\'),
            b'0'..=b'7' => {
                // Exactly three octal digits, as `quote_c_style` writes
                // them. Accepting a shorter run would mis-read the digits
                // of an ordinary filename that follows the escape.
                let second = bytes.next()?;
                let third = bytes.next()?;
                let digits = [escape, second, third];
                if !digits.iter().all(|d| (b'0'..=b'7').contains(d)) {
                    return None;
                }
                let value = digits
                    .iter()
                    .fold(0u32, |acc, digit| acc * 8 + u32::from(digit - b'0'));
                out.push(u8::try_from(value).ok()?);
            }
            _ => return None,
        }
    }

    String::from_utf8(out).ok().map(Cow::Owned)
}

/// Splits a leading double-quoted token off `value`, returning it (quotes
/// included, still encoded) and the remainder.
///
/// Needed because a `diff --git` header carries two paths separated by a
/// space, and a quoted path may itself contain an escaped space or quote —
/// so the split cannot be made by looking for a space, only by tracking
/// where the token's own closing quote falls.
///
/// `None` when `value` does not start with a quote, or has no unescaped
/// closing quote.
pub fn split_quoted_token(value: &str) -> Option<(&str, &str)> {
    if !value.starts_with('"') {
        return None;
    }
    let bytes = value.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some((&value[..=i], &value[i + 1..])),
            _ => i += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    mod unquote_git_path_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        #[rstest]
        #[case::should_pass_an_unquoted_path_through("src/lib.rs", "src/lib.rs")]
        #[case::should_pass_an_unquoted_path_with_a_space_through("src/a b.rs", "src/a b.rs")]
        // What git actually writes for a Japanese filename under the
        // default core.quotePath=true — the case that motivated this
        // module.
        #[case::should_decode_octal_escaped_utf8(
            "\"src/\\346\\227\\245\\346\\234\\254\\350\\252\\236.rs\"",
            "src/日本語.rs"
        )]
        #[case::should_decode_an_escaped_quote("\"src/a\\\"b.rs\"", "src/a\"b.rs")]
        #[case::should_decode_an_escaped_backslash("\"src/a\\\\b.rs\"", "src/a\\b.rs")]
        #[case::should_decode_a_named_control_escape("\"src/a\\tb.rs\"", "src/a\tb.rs")]
        #[case::should_decode_a_newline_escape("\"src/a\\nb.rs\"", "src/a\nb.rs")]
        #[case::should_decode_an_octal_control_escape("\"src/a\\033b.rs\"", "src/a\u{1b}b.rs")]
        #[case::should_decode_an_empty_quoted_path("\"\"", "")]
        fn should_decode_path_when_input_is_well_formed(
            #[case] input: &str,
            #[case] expected: &str,
        ) {
            let actual = unquote_git_path(input);

            assert_eq!(Some(expected.to_string()), actual.map(|p| p.into_owned()));
        }

        #[rstest]
        #[case::unterminated_quote("\"src/lib.rs")]
        #[case::escape_at_end_of_input("\"src/lib.rs\\\"")]
        #[case::truncated_octal_escape("\"src/\\34\"")]
        #[case::non_octal_digit_in_escape("\"src/\\389\"")]
        #[case::unknown_escape_letter("\"src/\\q.rs\"")]
        // \377 is a valid byte but not valid UTF-8 on its own.
        #[case::octal_escape_that_is_not_utf8("\"src/\\377.rs\"")]
        #[case::bare_quote_inside_the_token("\"src/a\"b.rs\"")]
        fn should_reject_path_when_quoted_form_is_malformed(#[case] input: &str) {
            let actual = unquote_git_path(input);

            assert_eq!(None, actual.map(|p| p.into_owned()));
        }

        // ADR 0090: the escapes are octal *byte* values, so a quoted path
        // can spell an escape that only appears once decoded. Decoding has
        // to happen before containment is judged, which is exactly why
        // this function exists at the parse boundary rather than further
        // in.
        #[rstest]
        #[case::parent_escape("\"a/\\056\\056/etc/passwd\"", "a/../etc/passwd")]
        #[case::absolute_path("\"\\057etc\\057shadow\"", "/etc/shadow")]
        fn should_decode_an_escaping_path_rather_than_hiding_it(
            #[case] input: &str,
            #[case] expected: &str,
        ) {
            let actual = unquote_git_path(input);

            assert_eq!(Some(expected.to_string()), actual.map(|p| p.into_owned()));
            assert_eq!(false, crate::repo_path::is_repo_relative(expected));
        }

        #[test]
        fn should_borrow_without_allocating_when_the_path_is_unquoted() {
            let actual = unquote_git_path("src/lib.rs");

            assert_eq!(true, matches!(actual, Some(Cow::Borrowed(_))));
        }
    }

    #[rstest]
    #[case::plain_token("\"a/x.rs\" \"b/x.rs\"", "\"a/x.rs\"", " \"b/x.rs\"")]
    #[case::token_containing_an_escaped_quote("\"a/x\\\"y.rs\" rest", "\"a/x\\\"y.rs\"", " rest")]
    #[case::token_containing_an_escaped_backslash("\"a/x\\\\.rs\" rest", "\"a/x\\\\.rs\"", " rest")]
    #[case::token_containing_a_space("\"a/x y.rs\" rest", "\"a/x y.rs\"", " rest")]
    #[case::nothing_after_the_token("\"a/x.rs\"", "\"a/x.rs\"", "")]
    fn should_split_quoted_token_when_it_is_terminated(
        #[case] input: &str,
        #[case] expected_token: &str,
        #[case] expected_rest: &str,
    ) {
        let actual = split_quoted_token(input);

        assert_eq!(Some((expected_token, expected_rest)), actual);
    }

    #[rstest]
    #[case::unquoted("a/x.rs b/x.rs")]
    #[case::unterminated("\"a/x.rs")]
    #[case::closing_quote_escaped_away("\"a/x.rs\\\"")]
    fn should_not_split_quoted_token_when_input_is_not_a_terminated_token(#[case] input: &str) {
        let actual = split_quoted_token(input);

        assert_eq!(None, actual);
    }
}
