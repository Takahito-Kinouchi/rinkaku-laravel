//! PHP `LanguageSupport` implementation.
//!
//! Uses `tree-sitter-php`'s `LANGUAGE_PHP` grammar (the variant that
//! understands `<?php ... ?>` tags with interleaved HTML text) rather than
//! `LANGUAGE_PHP_ONLY`: real-world `.php` files — templates especially —
//! routinely mix markup with code, and the full grammar parses both while
//! degrading to the same tree for a pure-code file.

use super::LanguageSupport;

/// Tree-sitter query capturing the definition node kinds whose signatures
/// rinkaku extracts: free functions, class/interface/trait/enum methods,
/// classes, interfaces, traits, and enums.
///
/// `method_declaration` is the same node kind name Go's grammar uses for
/// its receiver methods; `extract.rs`'s flat node-kind matching already
/// maps it to `SymbolKind::Function` for both grammars, and
/// `find_container` distinguishes the two by shape (a Go method has a
/// `receiver` field, a PHP method has a class/interface/trait/enum
/// ancestor).
const DEFINITION_QUERY: &str = "\
[
  (function_definition) @definition.function
  (method_declaration) @definition.function
  (class_declaration) @definition.class
  (interface_declaration) @definition.interface
  (trait_declaration) @definition.trait
  (enum_declaration) @definition.enum
] @definition";

/// Tree-sitter query capturing identifiers referenced from inside a
/// definition.
///
/// - `function_call_expression function: (name)` captures free function
///   calls (`helper($x)`). Method calls through a receiver
///   (`$x->bar()`) are captured separately below; dynamic calls
///   (`$f()`, callee is a `variable_name`) are not a bare `name` and
///   stay uncaptured, same as every other grammar's non-identifier
///   callees (ADR 0003).
/// - `object_creation_expression (name)` captures class instantiation
///   (`new Foo()`), mirroring TypeScript's `new_expression` capture —
///   the class name is a plain child here, not a named field.
/// - `named_type (name)` captures parameter/return/property type
///   references. PHP's built-in types (`int`, `string`, `bool`, ...)
///   parse as the distinct `primitive_type` node kind, so they are
///   excluded by construction rather than via an exclusion list, same
///   as TypeScript's `predefined_type`.
/// - `scoped_call_expression scope: (name)` captures the class half of a
///   static call (`Foo::bar()`) as a type reference. The method half is
///   deliberately not captured: `X::create()`-style names are the same
///   associated-call shape whose unfiltered capture measurably polluted
///   Rust's graph (ADR 0064) — the class name alone already links the
///   call site to the type's definition.
/// - `member_call_expression name: (name)` captures receiver method
///   calls (`$x->bar()`) under `@reference.method`, the capture kind
///   `graph::collect_edges` matches without a same-container
///   restriction (ADR 0068) — a PHP method always lives inside a
///   class-like container, so a bare-reference capture could never
///   link to it.
/// - `use_declaration (name)` captures a class body's trait uses
///   (`use SomeTrait;`), linking the class to the trait definition the
///   same way a type reference would.
const REFERENCE_QUERY: &str = "\
[
  (function_call_expression function: (name) @reference.call)
  (object_creation_expression (name) @reference.call)
  (named_type (name) @reference.type)
  (scoped_call_expression scope: (name) @reference.type)
  (member_call_expression name: (name) @reference.method)
  (use_declaration (name) @reference.type)
]";

pub struct PhpSupport;

impl LanguageSupport for PhpSupport {
    fn name(&self) -> &'static str {
        "php"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_php::LANGUAGE_PHP.into()
    }

    fn definition_query(&self) -> &str {
        DEFINITION_QUERY
    }

    fn reference_query(&self) -> &str {
        REFERENCE_QUERY
    }

    /// Blade templates parse under this grammar (their path still ends
    /// in `.php`) but are excluded from the dependency index — see the
    /// trait method's doc comment.
    fn contributes_to_dependency_index(&self, path: &str) -> bool {
        !path.ends_with(".blade.php")
    }

    /// PHPUnit's conventions: suites live under a `tests/` (commonly
    /// capitalized `Tests/` in Symfony-style projects) directory, and a
    /// test class file ends in `Test.php` wherever it lives.
    fn is_test_path(&self, path: &str) -> bool {
        let file_name = path.rsplit('/').next().unwrap_or(path);
        file_name.ends_with("Test.php")
            || path
                .split('/')
                .any(|segment| segment == "tests" || segment == "Tests")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn should_produce_a_grammar_that_parses_without_errors() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&PhpSupport.grammar())
            .expect("grammar must load");
        let source = "<?php\nfunction helper(int $x): int {\n    return $x;\n}\n";

        let tree = parser.parse(source, None).expect("parse must succeed");

        assert!(!tree.root_node().has_error());
    }

    #[rstest]
    #[case::should_exclude_blade_template_from_index("resources/views/page.blade.php", false)]
    #[case::should_include_plain_php_in_index("app/Service.php", true)]
    fn contributes_to_dependency_index_cases(#[case] path: &str, #[case] expected: bool) {
        let support = PhpSupport;

        let actual = support.contributes_to_dependency_index(path);

        assert_eq!(expected, actual);
    }

    #[rstest]
    #[case::should_treat_tests_dir_as_test_path("tests/AppTest.php", true)]
    #[case::should_treat_capitalized_tests_dir_as_test_path("Tests/Unit/FooTest.php", true)]
    #[case::should_treat_nested_tests_dir_as_test_path("app/tests/helper.php", true)]
    #[case::should_treat_test_suffix_file_as_test_path("src/FooTest.php", true)]
    #[case::should_not_treat_production_file_as_test_path("src/Foo.php", false)]
    #[case::should_not_treat_tests_substring_segment_as_test_path("contests/Entry.php", false)]
    fn is_test_path_cases(#[case] path: &str, #[case] expected: bool) {
        let support = PhpSupport;

        let actual = support.is_test_path(path);

        assert_eq!(expected, actual);
    }
}
