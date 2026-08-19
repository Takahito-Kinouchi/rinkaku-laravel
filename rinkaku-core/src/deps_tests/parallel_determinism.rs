//! Regression for `TagsResolver::new`'s rayon-parallel indexing: only the
//! per-file parse is parallel — the collected results are inserted into
//! the index sequentially, in the same order as the input `files`, so a
//! name's candidate list keeps the exact insertion order the old
//! sequential loop produced. `resolve_dependencies`'s stable-sort
//! tie-break depends on that order (see its doc comment), so a switch to
//! an unordered combinator (`par_bridge`, unordered `fold`+`reduce`, or a
//! concurrent map) must fail loudly here.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_keep_same_name_candidates_in_input_file_order_across_repeated_calls() {
    // Several files defining the same name, plus enough distinct files
    // that rayon actually fans out — a shuffled insertion order would
    // reorder `resolve("helper")`'s candidate list.
    let files: Vec<(String, String)> = (0..24)
        .map(|i| {
            (
                format!("src/m{i:02}.rs"),
                format!("fn helper(x: i32) -> i32 {{\n    x + {i}\n}}\nfn only_{i}() {{}}\n"),
            )
        })
        .collect();
    let reference_names = names(&["helper"]);

    let first = TagsResolver::new(
        files.clone(),
        lang_for_path,
        &reference_names,
        false,
        &HashSet::new(),
        true,
        None,
    );

    let expected_paths: Vec<String> = files.iter().map(|(path, _)| path.clone()).collect();
    let first_paths: Vec<String> = first
        .resolve("helper")
        .into_iter()
        .map(|resolved| resolved.path)
        .collect();
    assert_eq!(expected_paths, first_paths);

    for _ in 0..4 {
        let again = TagsResolver::new(
            files.clone(),
            lang_for_path,
            &reference_names,
            false,
            &HashSet::new(),
            true,
            None,
        );
        assert_eq!(first.resolve("helper"), again.resolve("helper"));
    }
}
