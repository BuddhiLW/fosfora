//! Tests for [`super`]: the manifest format, path resolution, the clip choice
//! and the family order.

use super::*;

const FIXTURE: &str = include_str!("../../tests/fixtures/media-pack/media.json");

fn fixture() -> Vec<MediaItem> {
    parse(FIXTURE, Path::new("/packs/tessera")).expect("the fixture parses")
}

fn item(hd: Option<&str>, uw: Option<&str>) -> MediaItem {
    MediaItem {
        id: "x".into(),
        title: "X".into(),
        description: String::new(),
        family: "Orbits".into(),
        tags: vec![],
        seconds: 10.0,
        fps: 30.0,
        looping: true,
        hd: hd.map(PathBuf::from),
        uw: uw.map(PathBuf::from),
        thumb: None,
        preview: None,
    }
}

// A trimmed copy of tessera's real out/media/media.json (2026-11-08): both
// clips, uw only, hd only, and two unrendered items, one with no thumb.
#[test]
fn the_tessera_manifest_parses() {
    let items = fixture();
    assert_eq!(items.len(), 5);
    let g = &items[0];
    assert_eq!(g.id, "graphene-fermi");
    assert_eq!(g.title, "Graphene Fermi Contours");
    assert_eq!(
        g.description,
        "Graphene: Fermi contours rising from the Dirac points"
    );
    assert_eq!(g.family, "Lattices");
    assert_eq!(g.tags, ["graphene", "fermi-surface", "dirac", "hexagonal"]);
    assert_eq!((g.seconds, g.fps, g.looping), (16.0, 30.0, true));
    assert!(!g.unrendered());

    let c = items.iter().find(|i| i.id == "choreographies").unwrap();
    assert!(c.hd.is_none() && c.uw.is_some(), "uw only");

    let unrendered: Vec<&str> = items
        .iter()
        .filter(|i| i.unrendered())
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(
        unrendered,
        ["lissajous-3d", "girih-gold"],
        "items with empty clips are kept"
    );
    let girih = items.iter().find(|i| i.id == "girih-gold").unwrap();
    assert!(
        girih.thumb.is_none() && girih.preview.is_none(),
        "no thumb at all"
    );
}

#[test]
fn unknown_fields_and_missing_optional_ones_are_fine() {
    let json = r#"{"version": 7, "producer": {"name": "future"},
        "items": [{"id": "a", "family": "Knots",
                   "clips": {"hd": "a/hd.mp4", "8k": "a/8k.mp4"}, "shiny": true},
                  {"id": "b"},
                  {"title": "no id, skipped"}]}"#;
    let items = parse(json, Path::new("/p")).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].hd.as_deref(), Some(Path::new("/p/a/hd.mp4")));
    assert_eq!(items[1].title, "b", "an untitled item is called by its id");
    assert!(items[1].unrendered());
    assert!(items[1].thumb.is_none());
    assert!(
        parse("\"pack\"", Path::new("/p")).is_err(),
        "not a manifest"
    );
    assert!(parse("{\"items\": 3}", Path::new("/p")).is_err());
}

#[test]
fn paths_resolve_against_the_manifest_directory() {
    let items = fixture();
    let g = &items[0];
    let at = |s: &str| Some(PathBuf::from(s));
    assert_eq!(g.hd, at("/packs/tessera/graphene-fermi/clip-hd.mp4"));
    assert_eq!(g.uw, at("/packs/tessera/graphene-fermi/clip-uw.mp4"));
    assert_eq!(g.thumb, at("/packs/tessera/graphene-fermi/thumb.webp"));
    assert_eq!(g.preview, at("/packs/tessera/graphene-fermi/preview.webp"));

    let json = r#"{"items": [{"id": "a", "thumb": "/abs/t.webp", "preview": "",
                              "clips": {"hd": "../up/a.mp4"}}]}"#;
    let a = &parse(json, Path::new("/p/q")).unwrap()[0];
    assert_eq!(a.thumb, at("/abs/t.webp"), "an absolute path is kept");
    assert_eq!(a.preview, None, "an empty path is no path");
    assert_eq!(a.hd, at("/p/q/../up/a.mp4"));
}

#[test]
fn the_clip_follows_the_output_aspect() {
    let both = item(Some("hd.mp4"), Some("uw.mp4"));
    let hd = Some(Path::new("hd.mp4"));
    let uw = Some(Path::new("uw.mp4"));
    assert_eq!(both.clip_for(16.0 / 9.0), hd, "16:9 plays hd");
    assert_eq!(both.clip_for(4.0 / 3.0), hd);
    assert_eq!(both.clip_for(2.0), hd);
    assert_eq!(both.clip_for(2.2), uw, "2.2:1 counts as ultrawide");
    assert_eq!(both.clip_for(21.0 / 9.0), uw, "21:9 plays uw");
    assert_eq!(both.clip_for(2560.0 / 1080.0), uw);
    assert_eq!(both.clip_for(32.0 / 9.0), uw);

    let hd_only = item(Some("hd.mp4"), None);
    assert_eq!(hd_only.clip_for(21.0 / 9.0), hd, "no uw: hd on ultrawide");
    let uw_only = item(None, Some("uw.mp4"));
    assert_eq!(uw_only.clip_for(16.0 / 9.0), uw, "no hd: uw on 16:9");
    assert_eq!(item(None, None).clip_for(16.0 / 9.0), None);
}

#[test]
fn families_come_in_catalog_order() {
    let mk = |f: &str| MediaItem {
        family: f.into(),
        ..item(None, None)
    };
    let items: Vec<MediaItem> = [
        "Scenes", "Zebras", "Orbits", "", "Tilings", "Curves", "Apples", "Orbits", "Quantum",
        "Knots", "Fractals", "Lattices", "Waves", "Flows",
    ]
    .iter()
    .map(|f| mk(f))
    .collect();
    assert_eq!(
        families(&items),
        [
            "Orbits", "Curves", "Knots", "Fractals", "Lattices", "Waves", "Quantum", "Tilings",
            "Flows", "Scenes", "Apples", "Zebras", "Other"
        ]
    );
    assert_eq!(family_rank("Orbits"), 0);
    assert_eq!(family_rank("scenes"), 9, "case does not matter");
    assert_eq!(family_rank("Nope"), FAMILY_ORDER.len());

    // The fixture's families, as tessera writes them.
    assert_eq!(
        families(&fixture()),
        ["Orbits", "Curves", "Lattices", "Tilings"]
    );
}

#[test]
fn keys_tell_items_of_different_packs_apart() {
    let a = parse(FIXTURE, Path::new("/one")).unwrap();
    let b = parse(FIXTURE, Path::new("/two")).unwrap();
    assert_eq!(a[0].id, b[0].id);
    assert_ne!(a[0].key(), b[0].key());
    assert!(a[0].key().starts_with("media:"));
}

#[test]
fn manifests_are_found_recursively() {
    let root = std::env::temp_dir().join(format!("fosfora-media-packs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("a/b")).unwrap();
    std::fs::create_dir_all(root.join("c")).unwrap();
    std::fs::write(
        root.join("media.json"),
        r#"{"items":[{"id":"top","clips":{"hd":"t.mp4"}}]}"#,
    )
    .unwrap();
    std::fs::write(root.join("a/b/media.json"), r#"{"items":[{"id":"deep"}]}"#).unwrap();
    std::fs::write(root.join("c/media.edn"), "{}").unwrap();
    std::fs::write(root.join("c/media.json"), "not json").unwrap();

    let found: Vec<String> = manifests(&root)
        .iter()
        .map(|p| p.strip_prefix(&root).unwrap().display().to_string())
        .collect();
    assert_eq!(found, ["a/b/media.json", "c/media.json", "media.json"]);

    let items = scan(std::slice::from_ref(&root));
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["deep", "top"], "a broken manifest is skipped");
    assert_eq!(items[1].hd.as_deref(), Some(root.join("t.mp4").as_path()));
    let _ = std::fs::remove_dir_all(&root);
}
