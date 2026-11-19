use super::*;
use crate::nav::Nav;
use crate::tree::Tree;
use pretty_assertions::assert_eq;

// ADR 0099 decision 2: a top-level directory is a divider showing its
// full path, and a file under it shows its path relative to it, since the
// directories in between get no row of their own. Built through
// `Nav::rows`, the only producer of the row list `relative_labels` reads.

fn labels_of(tree: &Tree) -> Vec<String> {
    relative_labels(&Nav::new().rows(tree))
}

fn file_with_symbol(path: &str, name: &str) -> TreeNode {
    TreeNode {
        children: vec![symbol_node(path, plain_symbol(name), Badges::default())],
        ..file_node(path, Badges::default())
    }
}

#[test]
fn should_label_a_nested_file_by_its_path_below_the_divider() {
    let tree = Tree {
        roots: vec![dir_node(
            "rinkaku-core/src",
            Badges::default(),
            vec![
                dir_node(
                    "rinkaku-core/src/render",
                    Badges::default(),
                    vec![file_with_symbol(
                        "rinkaku-core/src/render/markdown.rs",
                        "symbol_kind_prefix",
                    )],
                ),
                file_node("rinkaku-core/src/lib.rs", Badges::default()),
            ],
        )],
    };

    let actual = labels_of(&tree);

    assert_eq!(
        vec![
            "rinkaku-core/src".to_string(),
            "render/markdown.rs".to_string(),
            String::new(),
            "lib.rs".to_string(),
        ],
        actual
    );
}

#[test]
fn should_not_strip_partial_string_overlap_between_sibling_directory_names() {
    // "src" and "src2" share "src" as a string prefix, but "src2/x.rs" is
    // not under "src": it must lose "src2/", not "src".
    let tree = Tree {
        roots: vec![
            dir_node(
                "src",
                Badges::default(),
                vec![file_node("src/a.rs", Badges::default())],
            ),
            dir_node(
                "src2",
                Badges::default(),
                vec![file_node("src2/x.rs", Badges::default())],
            ),
        ],
    };

    let actual = labels_of(&tree);

    assert_eq!(
        vec![
            "src".to_string(),
            "a.rs".to_string(),
            "src2".to_string(),
            "x.rs".to_string(),
        ],
        actual
    );
}

#[test]
fn should_keep_the_full_path_of_a_top_level_file_that_follows_a_divider() {
    let tree = Tree {
        roots: vec![
            dir_node(
                "src",
                Badges::default(),
                vec![file_node("src/a.rs", Badges::default())],
            ),
            file_node("README.md", Badges::default()),
        ],
    };

    let actual = labels_of(&tree);

    assert_eq!(
        vec![
            "src".to_string(),
            "a.rs".to_string(),
            "README.md".to_string(),
        ],
        actual
    );
}

#[test]
fn should_keep_the_full_path_of_a_file_under_the_tests_section() {
    let tree = Tree {
        roots: vec![section_node(
            SectionKind::Tests,
            Badges::default(),
            vec![dir_node(
                "tests/Feature",
                Badges::default(),
                vec![file_node("tests/Feature/UserTest.php", Badges::default())],
            )],
        )],
    };

    let actual = labels_of(&tree);

    assert_eq!(
        vec![String::new(), "tests/Feature/UserTest.php".to_string()],
        actual
    );
}
