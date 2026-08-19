//! Recall and precision tests for `LanguageSupport::index_prefilter_patterns`
//! overrides (ADR 0080).
//!
//! - **Recall**: for every overriding language, a file defining the
//!   referenced name — as a free function/type AND, where the language has
//!   that shape, as a class/impl/receiver member — must still be indexed,
//!   including a variant with irregular whitespace between the
//!   introducing keyword and the name (the whole reason
//!   [`normalize_whitespace`] exists). These go through the real
//!   `TagsResolver::new` end to end.
//! - **Precision**: content that only *calls* the referenced name, never
//!   defining it, must fail the prefilter — checked directly at
//!   [`should_parse_file`], since `resolve()` cannot distinguish "the file
//!   was never parsed" from "it was parsed but defines nothing by that
//!   name" (both look like an empty result).
//!
//! `use super::super::*` (not `super::*`) reaches `deps.rs` directly, the
//! same pattern `generated_lockfile_path.rs` uses, so the private
//! `should_parse_file`/`normalize_whitespace` helpers are in scope
//! alongside the public `TagsResolver`/`ResolvedSymbol`.

use super::super::*;
use crate::language::LanguageSupport;
use crate::language::go::GoSupport;
use crate::language::php::PhpSupport;
use crate::language::python::PythonSupport;
use crate::language::rust::RustSupport;
use pretty_assertions::assert_eq;
use std::collections::HashSet;

/// Builds a `TagsResolver` over a single `(path, content)` file using the
/// real language registry (`crate::language::language_for_path`, not the
/// `.rs`/`.go`-only `lang_for_path` test helper the other `deps_tests`
/// modules use) so PHP/Python content routes to the right grammar, then
/// resolves `name` against it — the end-to-end harness every recall case
/// below shares.
fn resolve_single_file(path: &str, content: &str, name: &str) -> Vec<ResolvedSymbol> {
    let files = [(path.to_string(), content.to_string())];
    let reference_names: HashSet<String> = [name.to_string()].into_iter().collect();

    let resolver = TagsResolver::new(
        files,
        crate::language::language_for_path,
        &reference_names,
        true,
        &HashSet::new(),
        true,
        None,
    );

    resolver.resolve(name)
}

/// Whether `content` passes the prefilter for `name` under `support`'s own
/// `index_prefilter_patterns` — the pure, should_parse_file-level check
/// the precision tests need, since an end-to-end `resolve()` cannot tell
/// "never parsed" apart from "parsed but empty".
fn should_parse_with_patterns(support: &dyn LanguageSupport, name: &str, content: &str) -> bool {
    let patterns = support.index_prefilter_patterns(name);
    let matcher = aho_corasick::AhoCorasick::new(&patterns)
        .expect("index_prefilter_patterns output must build a valid matcher");
    should_parse_file(&matcher, &normalize_whitespace(content))
}

// ---- Recall: PHP ----

#[test]
fn should_index_php_free_function_when_referenced() {
    let content = "<?php\nfunction helper(int $x): int {\n    return $x;\n}\n";

    let actual = resolve_single_file("src/helpers.php", content, "helper");

    assert_eq!(1, actual.len());
}

#[test]
fn should_index_php_class_method_when_referenced() {
    let content =
        "<?php\nclass Foo {\n    public function helper(): int {\n        return 1;\n    }\n}\n";

    let actual = resolve_single_file("src/Foo.php", content, "helper");

    assert_eq!(1, actual.len());
}

#[test]
fn should_index_php_free_function_when_keyword_and_name_have_irregular_whitespace() {
    let content = "<?php\nfunction\n   helper(int $x): int {\n    return $x;\n}\n";

    let actual = resolve_single_file("src/helpers.php", content, "helper");

    assert_eq!(1, actual.len());
}

// ---- Recall: Python ----

#[test]
fn should_index_python_free_function_when_referenced() {
    let content = "def helper():\n    pass\n";

    let actual = resolve_single_file("src/helpers.py", content, "helper");

    assert_eq!(1, actual.len());
}

#[test]
fn should_index_python_class_method_when_referenced() {
    let content = "class Foo:\n    def helper(self):\n        pass\n";

    let actual = resolve_single_file("src/foo.py", content, "helper");

    assert_eq!(1, actual.len());
}

#[test]
fn should_index_python_free_function_when_keyword_and_name_have_irregular_whitespace() {
    let content = "def  helper():\n    pass\n";

    let actual = resolve_single_file("src/helpers.py", content, "helper");

    assert_eq!(1, actual.len());
}

// ---- Recall: Rust ----

#[test]
fn should_index_rust_free_function_when_referenced() {
    let content = "fn helper(x: i32) -> i32 {\n    x\n}\n";

    let actual = resolve_single_file("src/lib.rs", content, "helper");

    assert_eq!(1, actual.len());
}

#[test]
fn should_index_rust_impl_method_when_referenced() {
    let content = "struct Foo;\n\nimpl Foo {\n    fn helper(&self) -> i32 {\n        1\n    }\n}\n";

    let actual = resolve_single_file("src/lib.rs", content, "helper");

    assert_eq!(1, actual.len());
}

#[test]
fn should_index_rust_free_function_when_keyword_and_name_have_irregular_whitespace() {
    let content = "fn\n   helper(x: i32) -> i32 {\n    x\n}\n";

    let actual = resolve_single_file("src/lib.rs", content, "helper");

    assert_eq!(1, actual.len());
}

// ---- Recall: Go ----

#[test]
fn should_index_go_free_function_when_referenced() {
    let content = "package main\n\nfunc Helper(x int) int {\n    return x\n}\n";

    let actual = resolve_single_file("pkg/helper.go", content, "Helper");

    assert_eq!(1, actual.len());
}

#[test]
fn should_index_go_receiver_method_when_referenced() {
    let content = "package main\n\ntype Repo struct{}\n\nfunc (r *Repo) Helper(x int) int {\n    return x\n}\n";

    let actual = resolve_single_file("pkg/repo.go", content, "Helper");

    assert_eq!(1, actual.len());
}

#[test]
fn should_index_go_free_function_when_keyword_and_name_have_irregular_whitespace() {
    let content = "package main\n\nfunc\n   Helper(x int) int {\n    return x\n}\n";

    let actual = resolve_single_file("pkg/helper.go", content, "Helper");

    assert_eq!(1, actual.len());
}

#[test]
fn should_index_go_receiver_method_when_name_and_params_are_split_by_whitespace() {
    // Legal (if never gofmt-emitted) spacing between a method's name and
    // its parameter list — the `") {name} ("` anchor's reason to exist.
    let content = "package main

type Repo struct{}

func (r *Repo) Helper (x int) int {
    return x
}
";

    let actual = resolve_single_file("pkg/repo.go", content, "Helper");

    assert_eq!(1, actual.len());
}

// ---- Precision: should_parse_file-level ----
//
// The motivating scenario (see `deps.rs`'s module doc, ADR 0080): a
// PHP/Laravel helper called from nearly every file but defined in exactly
// one must no longer make every calling file pass the prefilter.

#[test]
fn should_reject_php_file_that_only_calls_format_price() {
    let actual = should_parse_with_patterns(
        &PhpSupport,
        "format_price",
        "<?php\necho format_price($total);\n",
    );

    assert!(!actual);
}

#[test]
fn should_accept_php_file_that_defines_format_price() {
    let actual = should_parse_with_patterns(
        &PhpSupport,
        "format_price",
        "<?php\nfunction format_price(int $cents): string {\n    return (string) $cents;\n}\n",
    );

    assert!(actual);
}

#[test]
fn should_reject_python_file_that_only_calls_helper() {
    let actual = should_parse_with_patterns(
        &PythonSupport,
        "helper",
        "result = helper(x)\nprint(result)\n",
    );

    assert!(!actual);
}

#[test]
fn should_accept_python_file_that_defines_helper() {
    let actual =
        should_parse_with_patterns(&PythonSupport, "helper", "def helper(x):\n    return x\n");

    assert!(actual);
}

#[test]
fn should_reject_rust_file_that_only_calls_helper() {
    let actual =
        should_parse_with_patterns(&RustSupport, "helper", "fn main() {\n    helper(1);\n}\n");

    assert!(!actual);
}

#[test]
fn should_accept_rust_file_that_defines_helper() {
    let actual = should_parse_with_patterns(
        &RustSupport,
        "helper",
        "fn helper(x: i32) -> i32 {\n    x\n}\n",
    );

    assert!(actual);
}

#[test]
fn should_reject_go_file_that_only_calls_helper() {
    let actual = should_parse_with_patterns(
        &GoSupport,
        "Helper",
        "package main\n\nfunc main() {\n    Helper(1)\n}\n",
    );

    assert!(!actual);
}

#[test]
fn should_accept_go_file_that_defines_helper() {
    let actual = should_parse_with_patterns(
        &GoSupport,
        "Helper",
        "package main\n\nfunc Helper(x int) int {\n    return x\n}\n",
    );

    assert!(actual);
}

// ---- normalize_whitespace ----

#[test]
fn should_collapse_multiple_spaces_to_one() {
    let actual = normalize_whitespace("function   helper");

    assert_eq!("function helper", actual);
}

#[test]
fn should_collapse_mixed_whitespace_run_to_one_space() {
    let actual = normalize_whitespace("function\n\t  helper");

    assert_eq!("function helper", actual);
}

#[test]
fn should_leave_single_spaces_unchanged() {
    let actual = normalize_whitespace("function helper(x)");

    assert_eq!("function helper(x)", actual);
}

#[test]
fn should_leave_content_with_no_whitespace_unchanged() {
    let actual = normalize_whitespace("function(x){}");

    assert_eq!("function(x){}", actual);
}

#[test]
fn should_return_empty_string_when_input_is_empty() {
    let actual = normalize_whitespace("");

    assert_eq!("", actual);
}

#[test]
fn should_collapse_leading_and_trailing_whitespace_runs() {
    let actual = normalize_whitespace("  \n function helper \t\n");

    assert_eq!(" function helper ", actual);
}
