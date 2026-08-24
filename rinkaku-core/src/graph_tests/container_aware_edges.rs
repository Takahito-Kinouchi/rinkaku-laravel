//! Tests for `collect_edges`'s two container-aware matching rules.
//!
//! ADR 0068: a bare `referenced_names` entry may only match a changed
//! symbol with no container or the same container as the referencing
//! symbol, while a `referenced_method_names` entry matches any container,
//! same as every reference matched before that ADR.
//!
//! ADR 0086: a bare `referenced_names` entry that matched nothing by name
//! falls back to matching the *container* a changed symbol lives in,
//! linking the referrer to that container's changed members — but not to
//! its own siblings, and not at all when the name already matched.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_not_build_edge_when_bare_reference_matches_a_same_named_symbol_in_a_different_container()
{
    // Reproduces the bug ADR 0068 fixes: a bare `Foo()` call (Python/Go/
    // TypeScript's grammars never capture a member-access call as a bare
    // reference) must not link to a `Foo` nested inside an unrelated
    // container just because the names collide.
    let referrer = symbol("use_foo", vec!["Foo"]);
    let top_level_foo = symbol("Foo", vec![]);
    let contained_foo = ExtractedSymbol {
        container: Some("class Baz".to_string()),
        range: LineRange { start: 10, end: 10 },
        ..symbol("Foo", vec![])
    };
    let files = vec![
        FileReport {
            path: "b.py".to_string(),
            symbols: vec![referrer],
        },
        FileReport {
            path: "a.py".to_string(),
            symbols: vec![top_level_foo, contained_foo],
        },
    ];

    let expected = SymbolGraph {
        nodes: vec![
            Node {
                id: "b.py::use_foo".to_string(),
                path: "b.py".to_string(),
                name: "use_foo".to_string(),
                container: None,
                is_test: false,
            },
            Node {
                id: "a.py::Foo@1".to_string(),
                path: "a.py".to_string(),
                name: "Foo".to_string(),
                container: None,
                is_test: false,
            },
            Node {
                id: "a.py::Foo@10".to_string(),
                path: "a.py".to_string(),
                name: "Foo".to_string(),
                container: Some("class Baz".to_string()),
                is_test: false,
            },
        ],
        edges: vec![Edge {
            from: "b.py::use_foo".to_string(),
            to: "a.py::Foo@1".to_string(),
            is_cycle: false,
        }],
        roots: vec!["b.py::use_foo".to_string(), "a.py::Foo@10".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_build_edge_when_bare_reference_matches_a_top_level_symbol() {
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("caller", vec!["helper"]), symbol("helper", vec![])],
    }];

    let expected = SymbolGraph {
        nodes: vec![
            Node {
                id: "src/lib.rs::caller".to_string(),
                path: "src/lib.rs".to_string(),
                name: "caller".to_string(),
                container: None,
                is_test: false,
            },
            Node {
                id: "src/lib.rs::helper".to_string(),
                path: "src/lib.rs".to_string(),
                name: "helper".to_string(),
                container: None,
                is_test: false,
            },
        ],
        edges: vec![Edge {
            from: "src/lib.rs::caller".to_string(),
            to: "src/lib.rs::helper".to_string(),
            is_cycle: false,
        }],
        roots: vec!["src/lib.rs::caller".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_build_edge_when_bare_reference_matches_a_symbol_in_the_same_container() {
    let referrer = ExtractedSymbol {
        container: Some("class Point".to_string()),
        ..symbol("area", vec!["helper"])
    };
    let target = ExtractedSymbol {
        container: Some("class Point".to_string()),
        ..symbol("helper", vec![])
    };
    let files = vec![FileReport {
        path: "shapes.py".to_string(),
        symbols: vec![referrer, target],
    }];

    let expected = SymbolGraph {
        nodes: vec![
            Node {
                id: "shapes.py::area".to_string(),
                path: "shapes.py".to_string(),
                name: "area".to_string(),
                container: Some("class Point".to_string()),
                is_test: false,
            },
            Node {
                id: "shapes.py::helper".to_string(),
                path: "shapes.py".to_string(),
                name: "helper".to_string(),
                container: Some("class Point".to_string()),
                is_test: false,
            },
        ],
        edges: vec![Edge {
            from: "shapes.py::area".to_string(),
            to: "shapes.py::helper".to_string(),
            is_cycle: false,
        }],
        roots: vec!["shapes.py::area".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_build_edge_when_method_reference_matches_a_symbol_in_a_different_container() {
    // A `referenced_method_names` entry (Rust's `x.foo()`, trait method
    // names, Go/TypeScript interface method specs) may legitimately
    // denote a symbol in any container, so it keeps the unrestricted
    // matching every reference had before ADR 0068.
    let referrer = ExtractedSymbol {
        container: Some("fn main".to_string()),
        referenced_method_names: vec!["save".to_string()],
        ..symbol("run", vec![])
    };
    let target = ExtractedSymbol {
        container: Some("impl Repo".to_string()),
        ..symbol("save", vec![])
    };
    let files = vec![FileReport {
        path: "repo.rs".to_string(),
        symbols: vec![referrer, target],
    }];

    let expected = SymbolGraph {
        nodes: vec![
            Node {
                id: "repo.rs::run".to_string(),
                path: "repo.rs".to_string(),
                name: "run".to_string(),
                container: Some("fn main".to_string()),
                is_test: false,
            },
            Node {
                id: "repo.rs::save".to_string(),
                path: "repo.rs".to_string(),
                name: "save".to_string(),
                container: Some("impl Repo".to_string()),
                is_test: false,
            },
        ],
        edges: vec![Edge {
            from: "repo.rs::run".to_string(),
            to: "repo.rs::save".to_string(),
            is_cycle: false,
        }],
        roots: vec!["repo.rs::run".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_build_edge_when_bare_reference_names_the_container_of_a_changed_symbol() {
    // The Laravel shape ADR 0086 addresses: the controller action names the
    // request class in a type hint, while the changed symbol on the other
    // side is a method *inside* that class. Without container matching the
    // two never meet and `rules` becomes a second root, rendering beside
    // the action that depends on it instead of underneath it.
    let action = symbol("store", vec!["StoreOrderRequest"]);
    let rules = ExtractedSymbol {
        container: Some("class StoreOrderRequest".to_string()),
        ..symbol("rules", vec![])
    };
    let files = vec![
        FileReport {
            path: "app/Http/Controllers/OrderController.php".to_string(),
            symbols: vec![action],
        },
        FileReport {
            path: "app/Http/Requests/StoreOrderRequest.php".to_string(),
            symbols: vec![rules],
        },
    ];

    let expected = SymbolGraph {
        nodes: vec![
            Node {
                id: "app/Http/Controllers/OrderController.php::store".to_string(),
                path: "app/Http/Controllers/OrderController.php".to_string(),
                name: "store".to_string(),
                container: None,
                is_test: false,
            },
            Node {
                id: "app/Http/Requests/StoreOrderRequest.php::rules".to_string(),
                path: "app/Http/Requests/StoreOrderRequest.php".to_string(),
                name: "rules".to_string(),
                container: Some("class StoreOrderRequest".to_string()),
                is_test: false,
            },
        ],
        edges: vec![Edge {
            from: "app/Http/Controllers/OrderController.php::store".to_string(),
            to: "app/Http/Requests/StoreOrderRequest.php::rules".to_string(),
            is_cycle: false,
        }],
        roots: vec!["app/Http/Controllers/OrderController.php::store".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_build_container_edge_when_the_container_label_carries_no_keyword() {
    // A Go method's container label is the bare receiver type name, with
    // none of the `class `/`impl `/... keywords `container_type_name`
    // strips — the whole label is the type name in that case.
    let referrer = symbol("run", vec!["Repo"]);
    let save = ExtractedSymbol {
        container: Some("Repo".to_string()),
        ..symbol("Save", vec![])
    };
    let files = vec![
        FileReport {
            path: "cmd/main.go".to_string(),
            symbols: vec![referrer],
        },
        FileReport {
            path: "store/repo.go".to_string(),
            symbols: vec![save],
        },
    ];

    let expected = SymbolGraph {
        nodes: vec![
            Node {
                id: "cmd/main.go::run".to_string(),
                path: "cmd/main.go".to_string(),
                name: "run".to_string(),
                container: None,
                is_test: false,
            },
            Node {
                id: "store/repo.go::Save".to_string(),
                path: "store/repo.go".to_string(),
                name: "Save".to_string(),
                container: Some("Repo".to_string()),
                is_test: false,
            },
        ],
        edges: vec![Edge {
            from: "cmd/main.go::run".to_string(),
            to: "store/repo.go::Save".to_string(),
            is_cycle: false,
        }],
        roots: vec!["cmd/main.go::run".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_not_build_container_edge_between_members_of_the_same_container() {
    // `self::`/`static::`/`Foo::class` written inside `Foo` names its own
    // container; expanding that to every changed sibling would turn each
    // class into a mesh saying nothing about which member uses which.
    let store = ExtractedSymbol {
        container: Some("class OrderController".to_string()),
        ..symbol("store", vec!["OrderController"])
    };
    let index = ExtractedSymbol {
        container: Some("class OrderController".to_string()),
        range: LineRange { start: 10, end: 10 },
        ..symbol("index", vec![])
    };
    let files = vec![FileReport {
        path: "app/Http/Controllers/OrderController.php".to_string(),
        symbols: vec![store, index],
    }];

    let expected = SymbolGraph {
        nodes: vec![
            Node {
                id: "app/Http/Controllers/OrderController.php::store".to_string(),
                path: "app/Http/Controllers/OrderController.php".to_string(),
                name: "store".to_string(),
                container: Some("class OrderController".to_string()),
                is_test: false,
            },
            Node {
                id: "app/Http/Controllers/OrderController.php::index".to_string(),
                path: "app/Http/Controllers/OrderController.php".to_string(),
                name: "index".to_string(),
                container: Some("class OrderController".to_string()),
                is_test: false,
            },
        ],
        edges: vec![],
        roots: vec![
            "app/Http/Controllers/OrderController.php::store".to_string(),
            "app/Http/Controllers/OrderController.php::index".to_string(),
        ],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_not_build_container_edge_when_the_reference_already_matched_a_symbol_by_name() {
    // Container matching is a fallback, not an addition: `OrderService` is
    // itself a changed symbol here, so it is the precise target the type
    // hint denotes. Reaching past it to `create` as well would add an edge
    // saying nothing the direct one does not.
    let action = symbol("store", vec!["OrderService"]);
    let service_class = ExtractedSymbol {
        kind: SymbolKind::Class,
        ..symbol("OrderService", vec![])
    };
    let create = ExtractedSymbol {
        container: Some("class OrderService".to_string()),
        range: LineRange { start: 10, end: 10 },
        ..symbol("create", vec![])
    };
    let files = vec![
        FileReport {
            path: "app/Http/Controllers/OrderController.php".to_string(),
            symbols: vec![action],
        },
        FileReport {
            path: "app/Services/OrderService.php".to_string(),
            symbols: vec![service_class, create],
        },
    ];

    let expected = SymbolGraph {
        nodes: vec![
            Node {
                id: "app/Http/Controllers/OrderController.php::store".to_string(),
                path: "app/Http/Controllers/OrderController.php".to_string(),
                name: "store".to_string(),
                container: None,
                is_test: false,
            },
            Node {
                id: "app/Services/OrderService.php::OrderService".to_string(),
                path: "app/Services/OrderService.php".to_string(),
                name: "OrderService".to_string(),
                container: None,
                is_test: false,
            },
            Node {
                id: "app/Services/OrderService.php::create".to_string(),
                path: "app/Services/OrderService.php".to_string(),
                name: "create".to_string(),
                container: Some("class OrderService".to_string()),
                is_test: false,
            },
        ],
        edges: vec![Edge {
            from: "app/Http/Controllers/OrderController.php::store".to_string(),
            to: "app/Services/OrderService.php::OrderService".to_string(),
            is_cycle: false,
        }],
        roots: vec![
            "app/Http/Controllers/OrderController.php::store".to_string(),
            "app/Services/OrderService.php::create".to_string(),
        ],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_build_one_edge_when_a_name_appears_in_both_reference_sets() {
    // A method called through a receiver in one place and named as a bare
    // reference in another lands in both sets, and each set is matched
    // separately. Without deduplication the pair renders twice in the
    // change graph, the second time as a `(see above)` line.
    let referrer = ExtractedSymbol {
        referenced_method_names: vec!["save".to_string()],
        ..symbol("run", vec!["save"])
    };
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![referrer, symbol("save", vec![])],
    }];

    let expected = SymbolGraph {
        nodes: vec![
            Node {
                id: "src/lib.rs::run".to_string(),
                path: "src/lib.rs".to_string(),
                name: "run".to_string(),
                container: None,
                is_test: false,
            },
            Node {
                id: "src/lib.rs::save".to_string(),
                path: "src/lib.rs".to_string(),
                name: "save".to_string(),
                container: None,
                is_test: false,
            },
        ],
        edges: vec![Edge {
            from: "src/lib.rs::run".to_string(),
            to: "src/lib.rs::save".to_string(),
            is_cycle: false,
        }],
        roots: vec!["src/lib.rs::run".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}
