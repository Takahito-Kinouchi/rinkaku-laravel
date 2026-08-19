//! Tests pinning [`super::extract_changed_symbols`] and
//! [`super::extract_all_symbols`] behavior on PHP sources: free
//! functions, class/interface/trait/enum containers, class-like
//! signature slicing with method bodies stripped, and the reference
//! captures (free calls, `new` expressions, named types, static-call
//! scopes, receiver method calls, trait `use`).

use super::*;
use crate::language::php::PhpSupport;
use pretty_assertions::assert_eq;

#[test]
fn should_extract_function_signature_when_body_line_changed() {
    let source = "\
<?php
function format_name(string $name): string {
    return trim($name);
}
";
    let lang = PhpSupport;
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "format_name".to_string(),
        kind: SymbolKind::Function,
        signature: "function format_name(string $name): string".to_string(),
        range: LineRange { start: 2, end: 4 },
        container: None,
        referenced_names: vec!["trim".to_string()],
        referenced_method_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_report_enclosing_class_as_container_when_method_body_changed() {
    let source = "\
<?php
class UserService
{
    public function findUser(int $id): ?User
    {
        return $this->repository->find($id);
    }
}
";
    let lang = PhpSupport;
    let changed_ranges = vec![LineRange { start: 6, end: 6 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "findUser".to_string(),
        kind: SymbolKind::Function,
        signature: "public function findUser(int $id): ?User".to_string(),
        range: LineRange { start: 4, end: 7 },
        container: Some("class UserService".to_string()),
        referenced_names: vec!["User".to_string()],
        // `find` is on the ADR 0064 ubiquitous-name stoplist, so the
        // receiver call `->find($id)` contributes nothing here.
        referenced_method_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_strip_method_bodies_from_class_signature() {
    let source = "\
<?php
class Greeter
{
    use LoggingTrait;

    public function greet(string $name): string
    {
        return \"hi $name\";
    }
}
";
    let lang = PhpSupport;

    let symbols = extract_all_symbols(source, &lang);

    let class_symbol = symbols
        .iter()
        .find(|s| s.name == "Greeter")
        .expect("class symbol extracted");
    assert_eq!(SymbolKind::Class, class_symbol.kind);
    assert_eq!(
        "class Greeter\n{\n    use LoggingTrait;\n\n    public function greet(string $name): string\n\n}",
        class_symbol.signature
    );
    // The trait `use` links the class to the trait definition.
    assert_eq!(
        vec!["LoggingTrait".to_string()],
        class_symbol.referenced_names
    );
}

#[test]
fn should_report_trait_and_interface_and_enum_containers() {
    let source = "\
<?php
interface Greeter
{
    public function greet(string $name): string;
}

trait LoggingTrait
{
    public function log(mixed $value): void
    {
        error_log(print_r($value, true));
    }
}

enum Status: string
{
    case Active = 'active';

    public function label(): string
    {
        return ucfirst($this->value);
    }
}
";
    let lang = PhpSupport;

    let symbols = extract_all_symbols(source, &lang);

    let containers: Vec<(String, Option<String>, SymbolKind)> = symbols
        .iter()
        .map(|s| (s.name.clone(), s.container.clone(), s.kind))
        .collect();
    let expected = vec![
        ("Greeter".to_string(), None, SymbolKind::Interface),
        (
            "greet".to_string(),
            Some("interface Greeter".to_string()),
            SymbolKind::Function,
        ),
        ("LoggingTrait".to_string(), None, SymbolKind::Trait),
        (
            "log".to_string(),
            Some("trait LoggingTrait".to_string()),
            SymbolKind::Function,
        ),
        ("Status".to_string(), None, SymbolKind::Enum),
        (
            "label".to_string(),
            Some("enum Status".to_string()),
            SymbolKind::Function,
        ),
    ];
    assert_eq!(expected, containers);
}

#[test]
fn should_strip_method_bodies_from_trait_and_enum_signatures() {
    let source = "\
<?php
trait LoggingTrait
{
    public function log(mixed $value): void
    {
        error_log(print_r($value, true));
    }
}

enum Status: string
{
    case Active = 'active';

    public function label(): string
    {
        return ucfirst($this->value);
    }
}
";
    let lang = PhpSupport;

    let symbols = extract_all_symbols(source, &lang);

    let trait_symbol = symbols
        .iter()
        .find(|s| s.name == "LoggingTrait")
        .expect("trait symbol extracted");
    assert_eq!(
        "trait LoggingTrait\n{\n    public function log(mixed $value): void\n\n}",
        trait_symbol.signature
    );
    let enum_symbol = symbols
        .iter()
        .find(|s| s.name == "Status")
        .expect("enum symbol extracted");
    assert_eq!(
        "enum Status: string\n{\n    case Active = 'active';\n\n    public function label(): string\n\n}",
        enum_symbol.signature
    );
}

#[test]
fn should_capture_new_expression_and_static_call_scope_as_references() {
    let source = "\
<?php
class Factory
{
    public static function create(): UserService
    {
        Clock::reset();
        return new UserService(new UserRepository());
    }
}
";
    let lang = PhpSupport;

    let symbols = extract_all_symbols(source, &lang);

    let method = symbols
        .iter()
        .find(|s| s.name == "create")
        .expect("method extracted");
    assert_eq!(
        vec![
            "Clock".to_string(),
            "UserRepository".to_string(),
            "UserService".to_string(),
        ],
        method.referenced_names
    );
}

#[test]
fn should_extract_via_end_to_end_diff_when_php_file_changes() {
    let diff = "\
diff --git a/src/Service.php b/src/Service.php
--- a/src/Service.php
+++ b/src/Service.php
@@ -2,3 +2,3 @@
 function helper(int $x): int {
-    return $x;
+    return $x + 1;
 }
";
    let source = "\
<?php
function helper(int $x): int {
    return $x + 1;
}
";
    let changed = crate::diff::parse_unified_diff(diff).expect("diff parses");
    let file = &changed[0];
    let lang = crate::language::language_for_path(&file.path).expect("php routes");

    let actual = extract_changed_symbols(source, lang, &file.changed_ranges);

    assert_eq!(1, actual.len());
    assert_eq!("helper", actual[0].name);
    assert_eq!("function helper(int $x): int", actual[0].signature);
}
