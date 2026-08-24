//! Tests for `collect_edges`'s path-proximity narrowing (ADR 0087): when a
//! referenced name matches several changed symbols, only the ones closest
//! to the referencing file in the repository tree become edges. The
//! motivating shape is a monorepo of Laravel applications, where framework
//! naming conventions put the same class name in every application.

use super::*;
use pretty_assertions::assert_eq;

/// Two applications side by side, each with its own `StoreOrderRequest`
/// whose `rules` changed, and each controller type-hinting that class.
/// Both controllers reference the bare name `StoreOrderRequest`, so
/// without proximity narrowing each links to both applications' `rules`.
fn two_app_monorepo_files() -> Vec<FileReport> {
    let mut files = Vec::new();
    for app in ["admin", "shop"] {
        files.push(FileReport {
            path: format!("apps/{app}/app/Http/Controllers/OrderController.php"),
            symbols: vec![symbol("store", vec!["StoreOrderRequest"])],
        });
        files.push(FileReport {
            path: format!("apps/{app}/app/Http/Requests/StoreOrderRequest.php"),
            symbols: vec![ExtractedSymbol {
                container: Some("class StoreOrderRequest".to_string()),
                ..symbol("rules", vec![])
            }],
        });
    }
    files
}

#[test]
fn should_link_only_within_the_nearest_app_when_a_container_name_spans_a_monorepo() {
    let actual = build_graph(&two_app_monorepo_files());

    let expected_edges = vec![
        Edge {
            from: "apps/admin/app/Http/Controllers/OrderController.php::store".to_string(),
            to: "apps/admin/app/Http/Requests/StoreOrderRequest.php::rules".to_string(),
            is_cycle: false,
        },
        Edge {
            from: "apps/shop/app/Http/Controllers/OrderController.php::store".to_string(),
            to: "apps/shop/app/Http/Requests/StoreOrderRequest.php::rules".to_string(),
            is_cycle: false,
        },
    ];
    assert_eq!(expected_edges, actual.edges);
}

#[test]
fn should_leave_each_app_its_own_root_when_a_container_name_spans_a_monorepo() {
    // The roots are the observable consequence of the edges above: with
    // cross-app edges, one app's controller stops being a root and the
    // tree claims a dependency between two unrelated applications.
    let actual = build_graph(&two_app_monorepo_files());

    let expected_roots = vec![
        "apps/admin/app/Http/Controllers/OrderController.php::store".to_string(),
        "apps/shop/app/Http/Controllers/OrderController.php::store".to_string(),
    ];
    assert_eq!(expected_roots, actual.roots);
}

#[test]
fn should_link_only_within_the_nearest_app_when_a_method_name_spans_a_monorepo() {
    // The same collision through `referenced_method_names` — `$service->
    // create(...)` in both applications — which predates ADR 0086's
    // container matching and is narrowed by the same rule.
    let mut files = Vec::new();
    for app in ["admin", "shop"] {
        files.push(FileReport {
            path: format!("apps/{app}/app/Http/Controllers/OrderController.php"),
            symbols: vec![ExtractedSymbol {
                referenced_method_names: vec!["create".to_string()],
                ..symbol("store", vec![])
            }],
        });
        files.push(FileReport {
            path: format!("apps/{app}/app/Services/OrderService.php"),
            symbols: vec![ExtractedSymbol {
                container: Some("class OrderService".to_string()),
                ..symbol("create", vec![])
            }],
        });
    }

    let actual = build_graph(&files);

    let expected_edges = vec![
        Edge {
            from: "apps/admin/app/Http/Controllers/OrderController.php::store".to_string(),
            to: "apps/admin/app/Services/OrderService.php::create".to_string(),
            is_cycle: false,
        },
        Edge {
            from: "apps/shop/app/Http/Controllers/OrderController.php::store".to_string(),
            to: "apps/shop/app/Services/OrderService.php::create".to_string(),
            is_cycle: false,
        },
    ];
    assert_eq!(expected_edges, actual.edges);
}

#[test]
fn should_keep_every_changed_member_of_one_container_since_they_share_a_file() {
    // The Laravel shape ADR 0086 exists for must survive the narrowing:
    // a container's changed members live in one file, so they tie on
    // proximity and all of them stay linked.
    let files = vec![
        FileReport {
            path: "app/Http/Controllers/OrderController.php".to_string(),
            symbols: vec![symbol("store", vec!["StoreOrderRequest"])],
        },
        FileReport {
            path: "app/Http/Requests/StoreOrderRequest.php".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    container: Some("class StoreOrderRequest".to_string()),
                    ..symbol("authorize", vec![])
                },
                ExtractedSymbol {
                    container: Some("class StoreOrderRequest".to_string()),
                    range: LineRange { start: 10, end: 10 },
                    ..symbol("rules", vec![])
                },
            ],
        },
    ];

    let actual = build_graph(&files);

    let expected_edges = vec![
        Edge {
            from: "app/Http/Controllers/OrderController.php::store".to_string(),
            to: "app/Http/Requests/StoreOrderRequest.php::authorize".to_string(),
            is_cycle: false,
        },
        Edge {
            from: "app/Http/Controllers/OrderController.php::store".to_string(),
            to: "app/Http/Requests/StoreOrderRequest.php::rules".to_string(),
            is_cycle: false,
        },
    ];
    assert_eq!(expected_edges, actual.edges);
}

#[test]
fn should_keep_equidistant_candidates_rather_than_picking_one() {
    // Two same-named targets the same distance from the referrer are
    // equally plausible under name-only resolution, so both stay — the
    // narrowing drops farther candidates, it does not disambiguate.
    let files = vec![
        FileReport {
            path: "src/caller.rs".to_string(),
            symbols: vec![symbol("caller", vec!["helper"])],
        },
        FileReport {
            path: "src/a.rs".to_string(),
            symbols: vec![symbol("helper", vec![])],
        },
        FileReport {
            path: "src/b.rs".to_string(),
            symbols: vec![symbol("helper", vec![])],
        },
    ];

    let actual = build_graph(&files);

    let expected_edges = vec![
        Edge {
            from: "src/caller.rs::caller".to_string(),
            to: "src/a.rs::helper".to_string(),
            is_cycle: false,
        },
        Edge {
            from: "src/caller.rs::caller".to_string(),
            to: "src/b.rs::helper".to_string(),
            is_cycle: false,
        },
    ];
    assert_eq!(expected_edges, actual.edges);
}
