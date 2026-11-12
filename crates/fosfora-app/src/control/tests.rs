//! Registry and queue: unit + property, against a context with no app.

// Every op body has the fixed `OpFn` signature, infallible ones included.
#![allow(clippy::unnecessary_wraps)]

use std::time::Duration;

use serde_json::{Value, json};

use super::testkit::cases;
use super::*;

#[derive(Default)]
struct Counter {
    n: i64,
    registry: Registry<Counter>,
}

impl OpHost for Counter {
    fn registry(&self) -> &Registry<Self> {
        &self.registry
    }
}

fn add(c: &mut Counter, a: &Value) -> OpResult {
    c.n += a["by"].as_i64().ok_or("missing :by")?;
    Ok(json!(c.n))
}

fn get(c: &mut Counter, _: &Value) -> OpResult {
    Ok(json!(c.n))
}

fn shadow(_: &mut Counter, _: &Value) -> OpResult {
    Ok(json!("shadowed"))
}

struct Math;
impl ControlAddon<Counter> for Math {
    fn id(&self) -> &'static str {
        "math"
    }
    fn ops(&self) -> Vec<OpSpec<Counter>> {
        vec![
            OpSpec::new("math/add", "{:by int}", "Add.", add),
            OpSpec::new("math/get", "{}", "Read.", get),
        ]
    }
}

struct Override;
impl ControlAddon<Counter> for Override {
    fn id(&self) -> &'static str {
        "override"
    }
    fn ops(&self) -> Vec<OpSpec<Counter>> {
        vec![OpSpec::new("math/get", "{}", "Shadow.", shadow)]
    }
}

fn counter() -> Counter {
    let mut c = Counter::default();
    c.registry.register(&Math);
    c.registry.register(&ops::core::Addon as &dyn ControlAddon<Counter>);
    c
}

#[test]
fn ops_run_by_name_and_unknown_names_are_refused() {
    let mut c = counter();
    assert_eq!(run(&mut c, "math/add", &json!({"by": 2})), Ok(json!(2)));
    assert_eq!(run(&mut c, "math/get", &json!({})), Ok(json!(2)));
    let e = run(&mut c, "math/nope", &json!({})).unwrap_err();
    assert!(e.contains("math/nope") && e.contains("f/ops"), "{e}");
    assert!(run(&mut c, "math/add", &json!({})).is_err());
}

#[test]
fn the_catalog_lists_every_op_with_its_addon_and_core_reads_it() {
    let mut c = counter();
    let catalog = run(&mut c, "ops", &json!({})).unwrap();
    let rows = catalog.as_array().unwrap();
    assert!(rows.iter().any(|r| r["name"] == "math/add" && r["addon"] == "math"));
    assert!(rows.iter().any(|r| r["name"] == "ops" && r["addon"] == "core"));
    assert_eq!(run(&mut c, "addons", &json!({})), Ok(json!(["math", "core"])));
}

#[test]
fn a_later_addon_overrides_an_op_and_is_listed_once() {
    let mut c = counter();
    c.registry.register(&Override);
    c.registry.register(&Override);
    assert_eq!(run(&mut c, "math/get", &json!({})), Ok(json!("shadowed")));
    assert_eq!(c.registry.addon_ids(), ["math", "core", "override"]);
}

#[test]
fn a_queued_call_is_answered_by_dispatch_and_times_out_without_it() {
    let (queue, rx) = OpQueue::new();
    let caller = {
        let q = queue.clone();
        std::thread::spawn(move || q.call("math/add", json!({"by": 5}), Duration::from_secs(5)))
    };
    let mut c = counter();
    let call = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    dispatch(&mut c, call);
    assert_eq!(caller.join().unwrap(), Ok(json!(5)));

    let e = queue
        .call("math/get", json!({}), Duration::from_millis(20))
        .unwrap_err();
    assert!(e.contains("no answer"), "{e}");
}

#[test]
fn the_builtin_addons_register_without_name_clashes() {
    let addons = ops::builtin_addons();
    let mut seen = std::collections::BTreeMap::new();
    for a in &addons {
        for op in a.ops() {
            if let Some(prev) = seen.insert(op.name, a.id()) {
                panic!("{} defined by both {prev} and {}", op.name, a.id());
            }
            let (domain, _) = op.name.split_once('/').unwrap_or(("", op.name));
            assert!(
                domain.is_empty() || domain == a.id(),
                "{} is not under {}/",
                op.name,
                a.id()
            );
            assert!(!op.doc.is_empty(), "{} has no doc", op.name);
        }
    }
    let ns: Vec<_> = ops::builtin_cljrs().iter().map(|n| n.alias).collect();
    let mut dedup = ns.clone();
    dedup.sort_unstable();
    dedup.dedup();
    assert_eq!(ns.len(), dedup.len(), "duplicate REPL aliases {ns:?}");
}

#[test]
fn random_call_sequences_sum_exactly_like_the_reference() {
    cases(200, 0xC0_FFEE, |rng, _| {
        let mut c = counter();
        let mut model = 0i64;
        for _ in 0..rng.below(40) {
            match rng.below(3) {
                0 => {
                    let by = rng.range_i(-9, 9) as i64;
                    model += by;
                    assert_eq!(run(&mut c, "math/add", &json!({"by": by})), Ok(json!(model)));
                }
                1 => assert_eq!(run(&mut c, "math/get", &json!({})), Ok(json!(model))),
                _ => assert!(run(&mut c, &rng.name(), &json!({})).is_err() || c.n == model),
            }
        }
        assert_eq!(c.n, model);
    });
}
