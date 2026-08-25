//! Validation for the `<owner>/<repo>` segments taken out of a `--pr` URL
//! or a clone's `origin` remote (ADR 0092).
//!
//! Both segments are attacker-adjacent input — a `--pr` URL can be pasted
//! from anywhere — and both are used to build things that are not strings:
//! a cache directory path (`…/repos/github.com/<owner>/<repo>`), a `gh
//! repo clone <owner>/<repo>` argument, and a `gh api
//! repos/<owner>/<repo>/…` route. `..` walks up the first, a leading `-`
//! turns the second into an option, and either walks the third.

/// GitHub's own limit is 39 characters for an owner and 100 for a
/// repository; one bound covers both, since the point here is to reject
/// the absurd rather than to mirror GitHub's validation.
const MAX_SEGMENT_LEN: usize = 100;

/// Whether `segment` is a plausible GitHub owner or repository name:
/// non-empty, within [`MAX_SEGMENT_LEN`], made only of ASCII
/// alphanumerics and `-`/`_`/`.`, not a `.`/`..` path component, and not
/// starting with `-` (which would make it an option rather than an
/// argument wherever it is passed to a command).
pub(crate) fn is_valid_repo_segment(segment: &str) -> bool {
    if segment.is_empty() || segment.len() > MAX_SEGMENT_LEN {
        return false;
    }
    if segment == "." || segment == ".." {
        return false;
    }
    if segment.starts_with('-') {
        return false;
    }
    segment
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::plain_owner("octocat")]
    #[case::repo_with_hyphen("hello-world")]
    #[case::repo_with_dot("rinkaku.rs")]
    #[case::repo_with_underscore("hello_world")]
    #[case::digits("2024")]
    #[case::mixed_case("Hello-World")]
    #[case::dot_prefixed_name(".github")]
    fn should_accept_segment_when_it_looks_like_a_github_name(#[case] segment: &str) {
        let actual = is_valid_repo_segment(segment);

        assert_eq!(true, actual);
    }

    #[rstest]
    #[case::empty("")]
    #[case::current_directory(".")]
    #[case::parent_directory("..")]
    #[case::leading_hyphen_reads_as_an_option("-filter=blob:none")]
    #[case::path_separator("octocat/hello-world")]
    #[case::backslash("octocat\\hello-world")]
    #[case::whitespace("hello world")]
    #[case::control_character("hello\u{1b}world")]
    #[case::non_ascii("こんにちは")]
    fn should_reject_segment_when_it_could_not_be_a_github_name(#[case] segment: &str) {
        let actual = is_valid_repo_segment(segment);

        assert_eq!(false, actual);
    }

    #[test]
    fn should_reject_segment_when_it_is_longer_than_the_length_bound() {
        let actual = is_valid_repo_segment(&"a".repeat(MAX_SEGMENT_LEN + 1));

        assert_eq!(false, actual);
    }
}
