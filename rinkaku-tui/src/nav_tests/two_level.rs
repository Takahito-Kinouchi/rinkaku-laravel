use super::*;
use pretty_assertions::assert_eq;

// ADR 0099 decision 2: a top-level directory or section is a divider,
// everything under it is a file row at the divider's depth with its
// symbols one level in, and a directory below a divider gets no row.

fn laravel_tree() -> Tree {
    Tree {
        roots: vec![
            dir_node(
                "app",
                vec![
                    dir_node(
                        "app/Http/Controllers",
                        vec![file_node(
                            "app/Http/Controllers/UserController.php",
                            vec![symbol_node(
                                "app/Http/Controllers/UserController.php",
                                "index",
                            )],
                        )],
                    ),
                    dir_node(
                        "app/Models",
                        vec![file_node(
                            "app/Models/User.php",
                            vec![symbol_node("app/Models/User.php", "posts")],
                        )],
                    ),
                ],
            ),
            section_node(vec![dir_node(
                "tests/Feature",
                vec![file_node("tests/Feature/UserTest.php", vec![])],
            )]),
        ],
    }
}

#[test]
fn should_list_files_flush_with_their_divider_and_skip_the_directories_between() {
    let tree = laravel_tree();

    let actual: Vec<(&str, usize)> = Nav::new()
        .rows(&tree)
        .iter()
        .map(|row| (row.node.path.as_str(), row.depth))
        .collect();

    assert_eq!(
        vec![
            ("app", 0),
            ("app/Http/Controllers/UserController.php", 0),
            ("app/Http/Controllers/UserController.php", 1),
            ("app/Models/User.php", 0),
            ("app/Models/User.php", 1),
            (crate::tree::TESTS_SECTION_PATH, 0),
            ("tests/Feature/UserTest.php", 0),
        ],
        actual
    );
}

#[test]
fn should_keep_the_files_of_a_directory_below_a_divider_visible_after_collapse_all() {
    // `CollapseAll` puts every directory in the collapse set, including
    // the ones without a row; re-expanding the divider must still show
    // their files, since nothing could ever expand those directories again.
    let tree = laravel_tree();

    let nav = Nav::new()
        .handle(Action::CollapseAll, &tree)
        .handle(Action::ToggleExpand, &tree);

    assert_eq!(
        vec![
            "app",
            "app/Http/Controllers/UserController.php",
            "app/Models/User.php",
            crate::tree::TESTS_SECTION_PATH,
        ],
        row_paths(&nav.rows(&tree))
    );
}

#[test]
fn should_land_on_a_symbol_inside_a_directory_that_has_no_row_when_jumping_to_it() {
    let tree = laravel_tree();
    let mut nav = Nav::new().handle(Action::CollapseAll, &tree);

    let actual = nav.move_cursor_to_symbol(&tree, "app/Models/User.php::posts");

    assert!(actual);
    let rows = nav.rows(&tree);
    assert_eq!("posts", symbol_name(&rows[nav.cursor()]));
}
