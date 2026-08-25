//! Neutralizing terminal control sequences in rendered output (ADR 0091).
//!
//! Everything a report is built from — file paths, symbol names,
//! signatures sliced out of source — comes from the change under review,
//! and a review is exactly the situation where that content was written by
//! someone else. A file whose *name* contains an ESC byte is enough: the
//! path is echoed verbatim into the Markdown report, and a terminal
//! printing that report interprets the sequence rather than showing it.

use std::borrow::Cow;

/// Replaces every control character that a terminal would act on with a
/// printable `\u{..}` escape, leaving `\n` and `\t` — the two that carry
/// layout rather than commands — alone.
///
/// Covers C0 (`U+0000`–`U+001F`), `DEL`, and C1 (`U+0080`–`U+009F`): C1 is
/// included because a terminal in 8-bit mode acts on those as it would on
/// their two-byte C0 equivalents, so escaping only the ESC-prefixed form
/// would leave the same commands reachable by another spelling.
///
/// Returns [`Cow::Borrowed`] unchanged when there is nothing to escape,
/// which is every ordinary report.
pub fn escape_control_chars(text: &str) -> Cow<'_, str> {
    if !text.chars().any(is_terminal_control) {
        return Cow::Borrowed(text);
    }
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if is_terminal_control(character) {
            escaped.push_str(&format!("\\u{{{:x}}}", character as u32));
        } else {
            escaped.push(character);
        }
    }
    Cow::Owned(escaped)
}

/// Whether `character` is a control character a terminal would act on.
/// `\n` and `\t` are excluded: the report's own structure is built out of
/// them.
fn is_terminal_control(character: char) -> bool {
    match character {
        '\n' | '\t' => false,
        '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::should_keep_plain_text("src/lib.rs", "src/lib.rs")]
    #[case::should_keep_newlines_and_tabs(
        "## Definitions\n\tfn foo()\n",
        "## Definitions\n\tfn foo()\n"
    )]
    #[case::should_keep_multibyte_text("fn 輪郭() {}", "fn 輪郭() {}")]
    #[case::should_escape_an_ansi_color_sequence(
        "src/\u{1b}[31mPWNED\u{1b}[0m.rs",
        "src/\\u{1b}[31mPWNED\\u{1b}[0m.rs"
    )]
    #[case::should_escape_an_osc_window_title_sequence(
        "src/\u{1b}]0;hijack\u{7}x.rs",
        "src/\\u{1b}]0;hijack\\u{7}x.rs"
    )]
    #[case::should_escape_a_carriage_return_used_to_overwrite_a_line(
        "harmless line\rPWNED",
        "harmless line\\u{d}PWNED"
    )]
    #[case::should_escape_delete("before\u{7f}after", "before\\u{7f}after")]
    // A terminal in 8-bit mode reads U+009B as CSI, the same command
    // `\u{1b}[` spells in two bytes.
    #[case::should_escape_a_c1_control("before\u{9b}31m", "before\\u{9b}31m")]
    fn should_escape_control_characters_when_present(#[case] input: &str, #[case] expected: &str) {
        let actual = escape_control_chars(input);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_borrow_without_allocating_when_there_is_nothing_to_escape() {
        let actual = escape_control_chars("## Definitions\n\nfn foo()\n");

        assert_eq!(true, matches!(actual, Cow::Borrowed(_)));
    }
}
