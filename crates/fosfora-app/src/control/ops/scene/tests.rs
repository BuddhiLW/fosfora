//! Unit + property tests for the scene ops, against an in-memory library
//! and the real (pure) [`Timeline`]. The integration leg is
//! `tests/repl_ops.rs`.

use serde_json::{Value, json};

use super::*;
use crate::control::testkit::cases;
use crate::control::{ControlAddon, OpHost, Registry, run};
use crate::scene::store::first_free_name;
use crate::scene::timeline::{Timeline, TimelineInfoState};
use crate::scene::types::{AdvanceMode, SceneCue, SceneSet};

struct Stub {
    scenes: Vec<(String, SceneSet)>,
    current: Option<usize>,
    tl: Timeline,
    presets: Vec<String>,
    loaded: Option<usize>,
    autosaves: usize,
    registry: Registry<Stub>,
}

impl Stub {
    fn new(presets: &[&str]) -> Self {
        let mut s = Self {
            scenes: Vec::new(),
            current: None,
            tl: Timeline::new(Vec::new(), false, AdvanceMode::Manual),
            presets: presets.iter().map(|p| (*p).to_owned()).collect(),
            loaded: None,
            autosaves: 0,
            registry: Registry::default(),
        };
        s.registry.register(&Addon as &dyn ControlAddon<Stub>);
        s
    }

    /// A stub with scenes `names`, each holding `cues` cues of preset "A".
    fn with_scenes(names: &[&str], cues: usize) -> Self {
        let mut s = Self::new(&["A", "Bloom", "Night Drive"]);
        for n in names {
            let mut set = SceneSet::new(n);
            set.cues = (0..cues).map(|_| SceneCue::new("A")).collect();
            s.scenes.push(((*n).to_owned(), set));
        }
        s.scenes.sort_by(|a, b| a.0.cmp(&b.0));
        s
    }

    fn call(&mut self, op: &str, args: Value) -> Result<Value, String> {
        run(self, op, &args)
    }

    fn names(&self) -> Vec<String> {
        self.scenes.iter().map(|(n, _)| n.clone()).collect()
    }

    fn put(&mut self, set: SceneSet) -> usize {
        let name = set.name.clone();
        self.scenes.retain(|(n, _)| *n != name);
        self.scenes.push((name.clone(), set));
        self.scenes.sort_by(|a, b| a.0.cmp(&b.0));
        self.scenes.iter().position(|(n, _)| *n == name).unwrap()
    }

    fn autosave(&mut self) {
        self.autosaves += 1;
        if let Some((name, set)) = self.current.and_then(|i| self.scenes.get_mut(i)) {
            set.name = name.clone();
            set.cues = self.tl.cues.clone();
            set.loop_mode = self.tl.loop_mode;
            set.advance_mode = self.tl.advance_mode;
        }
    }

    fn current_name(&self) -> Option<String> {
        self.current.and_then(|i| self.scenes.get(i)).map(|(n, _)| n.clone())
    }

    fn reselect(&mut self, name: Option<String>) {
        self.current = name.and_then(|n| self.scenes.iter().position(|(s, _)| *s == n));
    }
}

impl OpHost for Stub {
    fn registry(&self) -> &Registry<Self> {
        &self.registry
    }
}

impl ScenePort for Stub {
    fn scene_list(&self) -> Vec<(String, usize)> {
        self.scenes.iter().map(|(n, s)| (n.clone(), s.cues.len())).collect()
    }
    fn current_scene(&self) -> Option<usize> {
        self.current
    }
    fn save_scene(&mut self, name: &str) -> Result<usize, String> {
        if name.trim().is_empty() {
            return Err("Scene name cannot be empty".into());
        }
        if !self.scenes.iter().any(|(n, _)| n == name) {
            self.tl.cues.clear();
            self.tl.stop();
            self.tl.loop_mode = false;
            self.tl.advance_mode = AdvanceMode::Manual;
        }
        let mut set = SceneSet::new(name);
        set.cues = self.tl.cues.clone();
        set.loop_mode = self.tl.loop_mode;
        set.advance_mode = self.tl.advance_mode;
        let i = self.put(set);
        self.current = Some(i);
        Ok(i)
    }
    fn open_scene(&mut self, index: usize) -> Result<(), String> {
        let (_, set) = self.scenes.get(index).cloned().ok_or("Invalid scene index")?;
        self.current = Some(index);
        self.tl = Timeline::new(set.cues, set.loop_mode, set.advance_mode);
        Ok(())
    }
    fn play_scene(&mut self, index: usize) -> Result<(), String> {
        self.open_scene(index)?;
        self.tl.start(0);
        Ok(())
    }
    fn delete_scene(&mut self, index: usize) -> Result<(), String> {
        if index >= self.scenes.len() {
            return Err("Invalid scene index".into());
        }
        let keep = self.current_name();
        self.scenes.remove(index);
        self.reselect(keep);
        Ok(())
    }
    fn rename_scene(&mut self, index: usize, name: &str) -> Result<usize, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Scene name cannot be empty".into());
        }
        let (old, mut set) = self.scenes.get(index).cloned().ok_or("Invalid scene index")?;
        if old == name {
            return Ok(index);
        }
        if self.scenes.iter().any(|(n, _)| n == name) {
            return Err(format!("A scene named '{name}' already exists"));
        }
        let was_current = self.current == Some(index);
        let keep = self.current_name();
        self.scenes.remove(index);
        set.name = name.to_owned();
        let i = self.put(set);
        self.reselect(if was_current { Some(name.to_owned()) } else { keep });
        Ok(i)
    }
    fn duplicate_scene(&mut self, index: usize) -> Result<usize, String> {
        let (name, mut set) = self.scenes.get(index).cloned().ok_or("Invalid scene index")?;
        let keep = self.current_name();
        set.name = first_free_name(&self.names(), &format!("{name} copy"));
        let i = self.put(set);
        self.reselect(keep);
        Ok(i)
    }
}

impl CuePort for Stub {
    fn timeline(&self) -> TimelineView {
        let info = self.tl.info();
        TimelineView {
            cues: self.tl.cues.clone(),
            active: info.active,
            current_cue: info.current_cue,
            loop_mode: info.loop_mode,
            advance: info.advance_mode,
            transition: match info.state {
                TimelineInfoState::Transitioning { from, to, progress, .. } => Some((from, to, progress)),
                _ => None,
            },
        }
    }
    fn preset_names(&self) -> Vec<String> {
        self.presets.clone()
    }
    fn current_preset_name(&self) -> Option<String> {
        self.loaded.map(|i| self.presets[i].clone())
    }
    fn insert_cue(&mut self, at: Option<usize>, preset: &str) -> usize {
        let mut cue = SceneCue::new(preset);
        if self.tl.advance_mode == AdvanceMode::Timer {
            cue.hold_secs = Some(DEFAULT_HOLD_SECS);
        }
        let at = at.unwrap_or(self.tl.cues.len()).min(self.tl.cues.len());
        self.tl.insert_cue(at, cue);
        self.autosave();
        at
    }
    fn remove_cue(&mut self, index: usize) -> bool {
        if index >= self.tl.cues.len() {
            return false;
        }
        self.tl.cues.remove(index);
        self.tl.notify_cue_removed(index);
        self.autosave();
        true
    }
    fn move_cue(&mut self, from: usize, to: usize) {
        self.tl.move_cue(from, to);
        self.autosave();
    }
    fn edit_cue(&mut self, index: usize, edit: &CueEdit) -> bool {
        let Some(cue) = self.tl.cues.get_mut(index) else {
            return false;
        };
        apply_cue_edit(cue, edit);
        self.autosave();
        true
    }
    fn configure_timeline(&mut self, loop_mode: Option<bool>, advance: Option<AdvanceMode>) {
        if let Some(l) = loop_mode {
            self.tl.loop_mode = l;
        }
        if let Some(m) = advance {
            if m == AdvanceMode::Timer {
                fill_timer_holds(&mut self.tl.cues);
            }
            self.tl.advance_mode = m;
        }
        self.autosave();
    }
    fn transport(&mut self, t: Transport) {
        match t {
            Transport::Next => {
                self.tl.go_next();
            }
            Transport::Prev => {
                self.tl.go_prev();
            }
            Transport::Jump(i) => {
                self.tl.go_to_cue(i);
            }
            Transport::Toggle => {
                if self.tl.active {
                    self.tl.stop();
                } else {
                    self.tl.start(0);
                }
            }
            Transport::Play(i) => {
                self.tl.start(i);
            }
            Transport::Stop => self.tl.stop(),
        }
    }
}

// ── Unit ───────────────────────────────────────────────────────────────────

#[test]
fn every_op_is_registered_under_the_scene_namespace_and_every_signal_maps_to_one() {
    let s = Stub::new(&[]);
    let catalog = s.registry.catalog();
    let names: Vec<&str> = catalog
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert!(names.iter().all(|n| n.starts_with("scene/")), "{names:?}");
    for &(signal, op) in ControlAddon::<Stub>::ui_signals(&Addon) {
        assert!(names.contains(&op), "{signal} -> {op} not registered");
    }
}

#[test]
fn list_and_current_describe_the_library_and_the_timeline() {
    let mut s = Stub::with_scenes(&["Opener", "Close"], 2);
    let r = s.call("scene/list", json!({})).unwrap();
    assert_eq!(r[0], json!({"index": 0, "name": "Close", "cues": 2, "current": false}));
    let r = s.call("scene/current", json!({})).unwrap();
    assert_eq!(r["scene"], Value::Null);
    assert_eq!(r["playing"], false);
    assert_eq!(r["cues"], json!([]));
    s.call("scene/open", json!({"name": "open"})).unwrap();
    let r = s.call("scene/current", json!({})).unwrap();
    assert_eq!(r["scene"]["name"], "Opener");
    assert_eq!(r["cues"].as_array().unwrap().len(), 2);
    assert_eq!(r["advance"], json!({"mode": "manual", "beats-per-cue": null}));
}

#[test]
fn saving_a_new_name_starts_a_blank_cue_list_and_an_existing_one_keeps_it() {
    let mut s = Stub::with_scenes(&["Opener"], 3);
    s.call("scene/open", json!({"index": 0})).unwrap();
    let r = s.call("scene/save", json!({"name": "Opener"})).unwrap();
    assert_eq!(r, json!({"index": 0, "name": "Opener", "cues": 3}));
    let r = s.call("scene/save", json!({"name": "Fresh"})).unwrap();
    assert_eq!(r, json!({"index": 0, "name": "Fresh", "cues": 0}));
    assert!(s.tl.cues.is_empty());
    assert!(s.call("scene/save", json!({"name": " "})).is_err());
    assert!(s.call("scene/save", json!({})).is_err());
}

#[test]
fn open_edits_without_playing_and_load_plays_from_the_first_cue() {
    let mut s = Stub::with_scenes(&["Opener"], 2);
    s.call("scene/open", json!({"name": "Opener"})).unwrap();
    assert!(!s.tl.active);
    let r = s.call("scene/load", json!({"index": 0})).unwrap();
    assert_eq!(r["playback"]["playing"], true);
    assert_eq!(r["playback"]["cue"], 0);
    assert!(s.call("scene/load", json!({"index": 5})).is_err());
    assert!(s.call("scene/open", json!({"name": "nope"})).is_err());
    assert!(s.call("scene/open", json!({})).is_err());
}

#[test]
fn delete_rename_and_duplicate_report_the_scene_they_touched() {
    let mut s = Stub::with_scenes(&["Opener", "Zed"], 1);
    let r = s.call("scene/duplicate", json!({"name": "Opener"})).unwrap();
    assert_eq!(r["name"], "Opener copy");
    let r = s.call("scene/rename", json!({"name": "Opener copy", "to": "Encore"})).unwrap();
    assert_eq!(r["name"], "Encore");
    assert!(s.call("scene/rename", json!({"name": "Encore", "to": "Zed"})).is_err(), "taken");
    assert!(s.call("scene/rename", json!({"name": "Encore"})).is_err(), "no :to");
    let r = s.call("scene/delete", json!({"name": "Encore"})).unwrap();
    assert_eq!(r["name"], "Encore");
    assert_eq!(s.names(), ["Opener", "Zed"]);
    assert!(s.call("scene/delete", json!({"index": 2})).is_err());
}

#[test]
fn add_cue_uses_the_loaded_preset_or_a_named_one_and_inserts_at() {
    let mut s = Stub::with_scenes(&["Opener"], 0);
    s.call("scene/open", json!({"index": 0})).unwrap();
    assert!(s.call("scene/add-cue", json!({})).is_err(), "nothing loaded");
    s.loaded = Some(1);
    let r = s.call("scene/add-cue", json!({})).unwrap();
    assert_eq!(r["preset"], "Bloom");
    let r = s.call("scene/add-cue", json!({"preset": "night", "at": 0})).unwrap();
    assert_eq!((r["index"].as_u64(), r["preset"].as_str()), (Some(0), Some("Night Drive")));
    let r = s.call("scene/add-cue", json!({"preset": 0})).unwrap();
    assert_eq!((r["index"].as_u64(), r["preset"].as_str()), (Some(2), Some("A")));
    assert!(s.call("scene/add-cue", json!({"preset": "zzz"})).is_err());
    assert!(s.call("scene/add-cue", json!({"preset": 9})).is_err());
    assert!(s.call("scene/add-cue", json!({"preset": "A", "at": -1})).is_err());
    assert_eq!(s.scenes[0].1.cues.len(), 3, "edits autosave into the open scene");
}

#[test]
fn a_cue_added_in_timer_mode_gets_a_hold() {
    let mut s = Stub::with_scenes(&["Opener"], 0);
    s.call("scene/timeline", json!({"advance": "timer"})).unwrap();
    let r = s.call("scene/add-cue", json!({"preset": "A"})).unwrap();
    assert_eq!(r["hold"], json!(DEFAULT_HOLD_SECS));
}

#[test]
fn remove_and_move_refuse_out_of_range_cues() {
    let mut s = Stub::with_scenes(&["Opener"], 0);
    for p in ["A", "Bloom", "Night Drive"] {
        s.call("scene/add-cue", json!({"preset": p})).unwrap();
    }
    let r = s.call("scene/move-cue", json!({"from": 2, "to": 0})).unwrap();
    assert_eq!(r["preset"], "Night Drive");
    let r = s.call("scene/remove-cue", json!({"cue": 1})).unwrap();
    assert_eq!(r["preset"], "A");
    assert!(s.call("scene/remove-cue", json!({"cue": 2})).is_err());
    assert!(s.call("scene/move-cue", json!({"from": 0, "to": 5})).is_err());
    assert!(s.call("scene/remove-cue", json!({})).is_err());
    assert_eq!(s.tl.cues.len(), 2);
}

#[test]
fn set_cue_reads_without_changes_and_refuses_out_of_range_values() {
    let mut s = Stub::with_scenes(&["Opener"], 0);
    s.call("scene/add-cue", json!({"preset": "A"})).unwrap();
    let before = s.call("scene/set-cue", json!({"cue": 0})).unwrap();
    let saves = s.autosaves;
    assert_eq!(before["transition"], "cut");
    assert_eq!(s.autosaves, saves, "a read does not save");
    let r = s
        .call("scene/set-cue", json!({"cue": 0, "transition": "dissolve", "secs": 2.5, "hold": 8, "label": " Drop "}))
        .unwrap();
    assert_eq!(r, json!({"index": 0, "preset": "A", "label": "Drop", "transition": "dissolve", "secs": 2.5, "hold": 8.0}));
    let r = s.call("scene/set-cue", json!({"cue": 0, "hold": false, "label": ""})).unwrap();
    assert_eq!((r["hold"].clone(), r["label"].clone()), (Value::Null, Value::Null));
    for bad in [json!({"secs": 0}), json!({"secs": 31}), json!({"hold": 0.1}), json!({"transition": "wipe"}), json!({"label": 3})] {
        let mut a = bad.clone();
        a["cue"] = json!(0);
        assert!(s.call("scene/set-cue", a).is_err(), "{bad}");
    }
    assert_eq!(s.tl.cues[0].transition, crate::scene::types::TransitionType::Dissolve, "refusals change nothing");
    assert!(s.call("scene/set-cue", json!({"cue": 4})).is_err());
}

#[test]
fn transport_plays_steps_jumps_and_stops() {
    let mut s = Stub::with_scenes(&["Opener"], 0);
    assert!(s.call("scene/toggle", json!({})).is_err(), "no cues");
    assert!(s.call("scene/play", json!({})).is_err(), "no cues");
    for p in ["A", "Bloom", "Night Drive"] {
        s.call("scene/add-cue", json!({"preset": p})).unwrap();
    }
    assert!(s.call("scene/next", json!({})).is_err(), "stopped");
    let r = s.call("scene/toggle", json!({})).unwrap();
    assert_eq!((r["playing"].clone(), r["cue"].clone()), (json!(true), json!(0)));
    assert_eq!(s.call("scene/next", json!({})).unwrap()["cue"], 1);
    assert_eq!(s.call("scene/prev", json!({})).unwrap()["cue"], 0);
    assert_eq!(s.call("scene/jump", json!({"cue": 2})).unwrap()["cue"], 2);
    assert!(s.call("scene/jump", json!({"cue": 3})).is_err());
    let r = s.call("scene/stop", json!({})).unwrap();
    assert_eq!((r["playing"].clone(), r["cue"].clone()), (json!(false), Value::Null));
    assert_eq!(s.call("scene/play", json!({"cue": 1})).unwrap()["cue"], 1);
    assert!(s.call("scene/play", json!({"cue": 7})).is_err());
    assert_eq!(s.call("scene/toggle", json!({})).unwrap()["playing"], false);
}

#[test]
fn timeline_reads_and_changes_loop_and_advance() {
    let mut s = Stub::with_scenes(&["Opener"], 0);
    s.call("scene/add-cue", json!({"preset": "A"})).unwrap();
    let r = s.call("scene/timeline", json!({})).unwrap();
    assert_eq!(r, json!({"loop": false, "advance": {"mode": "manual", "beats-per-cue": null}}));
    assert!(s.call("scene/timeline", json!({"beats": 8})).is_err(), "not beat-synced");
    let r = s.call("scene/timeline", json!({"loop": true, "advance": "timer"})).unwrap();
    assert_eq!(r["loop"], true);
    assert_eq!(s.tl.cues[0].hold_secs, Some(DEFAULT_HOLD_SECS), "Timer fills holds");
    let r = s.call("scene/timeline", json!({"advance": "beat-sync"})).unwrap();
    assert_eq!(r["advance"], json!({"mode": "beats", "beats-per-cue": DEFAULT_BEATS}));
    let r = s.call("scene/timeline", json!({"beats": 16})).unwrap();
    assert_eq!(r["advance"]["beats-per-cue"], 16);
    for bad in [json!({"beats": 0}), json!({"beats": 65}), json!({"advance": "wild"}), json!({"loop": "yes"}), json!({"advance": 2})] {
        assert!(s.call("scene/timeline", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(s.tl.advance_mode, AdvanceMode::BeatSync { beats_per_cue: 16 });
}

#[test]
fn the_cue_strip_mode_ids_map_like_the_selector() {
    assert_eq!(advance_from_ui_id(0), AdvanceMode::Manual);
    assert_eq!(advance_from_ui_id(1), AdvanceMode::Timer);
    assert_eq!(advance_from_ui_id(2), AdvanceMode::BeatSync { beats_per_cue: DEFAULT_BEATS });
    assert_eq!(advance_from_ui_id(99), AdvanceMode::Manual);
}

// ── Property ───────────────────────────────────────────────────────────────

fn any_mode(rng: &mut crate::control::testkit::Rng) -> AdvanceMode {
    match rng.below(3) {
        0 => AdvanceMode::Manual,
        1 => AdvanceMode::Timer,
        _ => AdvanceMode::BeatSync { beats_per_cue: 1 + u32::try_from(rng.below(64)).unwrap() },
    }
}

#[test]
fn plan_advance_is_total_and_its_plans_are_in_range() {
    cases(500, 0x5CE_0001, |rng, _| {
        let current = any_mode(rng);
        let kind = rng.coin().then(|| *rng.pick(AdvanceKind::ALL).unwrap());
        let beats = rng.coin().then(|| u32::try_from(rng.below(80)).unwrap());
        match plan_advance(current, kind, beats) {
            Ok(None) => assert!(kind.is_none() && beats.is_none()),
            Ok(Some(mode)) => {
                if let Some(k) = kind {
                    assert_eq!(AdvanceKind::of(mode), k);
                }
                if let AdvanceMode::BeatSync { beats_per_cue } = mode {
                    assert!((BEATS_RANGE.0..=BEATS_RANGE.1).contains(&beats_per_cue));
                    if let Some(b) = beats {
                        assert_eq!(beats_per_cue, b);
                    }
                }
                // Applying the same request again changes nothing further.
                assert_eq!(plan_advance(mode, kind, beats), Ok(Some(mode)));
            }
            Err(_) => assert!(beats.is_some(), "only a bad :beats is refused"),
        }
    });
}

#[test]
fn advance_kind_names_round_trip() {
    for &k in AdvanceKind::ALL {
        assert_eq!(AdvanceKind::parse(k.name()), Ok(k));
        assert_eq!(AdvanceKind::parse(&k.name().to_uppercase()), Ok(k));
    }
    assert!(AdvanceKind::parse("sometimes").is_err());
}

#[test]
fn cue_edits_are_idempotent_and_touch_only_what_they_name() {
    use crate::scene::types::TransitionType as T;
    cases(400, 0x5CE_0002, |rng, _| {
        let mut cue = SceneCue::new("A");
        cue.hold_secs = rng.coin().then_some(3.0);
        let edit = CueEdit {
            transition: rng.coin().then(|| *rng.pick(T::ALL).unwrap()),
            secs: rng.coin().then_some(2.0),
            hold: rng.coin().then(|| rng.coin().then_some(7.0)),
            label: rng.coin().then(|| rng.name()),
        };
        let before = cue.clone();
        apply_cue_edit(&mut cue, &edit);
        let once = cue.clone();
        apply_cue_edit(&mut cue, &edit);
        assert_eq!(cue_json(0, &cue), cue_json(0, &once), "idempotent");
        assert_eq!(cue.preset_name, before.preset_name);
        if edit.transition.is_none() {
            assert_eq!(cue.transition, before.transition);
        }
        if edit.hold.is_none() {
            assert_eq!(cue.hold_secs, before.hold_secs);
        }
        if let Some(l) = &edit.label {
            assert_eq!(cue.label.as_deref(), Some(l.trim()).filter(|t| !t.is_empty()));
        }
    });
}

#[test]
fn fill_timer_holds_keeps_existing_holds_and_is_idempotent() {
    cases(200, 0x5CE_0003, |rng, _| {
        let mut cues: Vec<SceneCue> = (0..rng.below(6))
            .map(|_| {
                let mut c = SceneCue::new("A");
                c.hold_secs = rng.coin().then_some(9.0);
                c
            })
            .collect();
        let before = cues.clone();
        fill_timer_holds(&mut cues);
        for (a, b) in before.iter().zip(&cues) {
            assert_eq!(b.hold_secs, Some(a.hold_secs.unwrap_or(DEFAULT_HOLD_SECS)));
        }
        let once: Vec<_> = cues.iter().map(|c| c.hold_secs).collect();
        fill_timer_holds(&mut cues);
        assert_eq!(cues.iter().map(|c| c.hold_secs).collect::<Vec<_>>(), once);
    });
}

#[test]
fn random_op_sequences_keep_library_and_timeline_consistent() {
    let ops = [
        "scene/list", "scene/current", "scene/save", "scene/open", "scene/load", "scene/delete",
        "scene/rename", "scene/duplicate", "scene/add-cue", "scene/remove-cue", "scene/move-cue",
        "scene/set-cue", "scene/play", "scene/stop", "scene/toggle", "scene/next", "scene/prev",
        "scene/jump", "scene/timeline",
    ];
    cases(150, 0x5CE_0004, |rng, _| {
        let mut s = Stub::with_scenes(&["Opener", "Close"], 2);
        s.loaded = rng.coin().then_some(0);
        for _ in 0..40 {
            let op = *rng.pick(&ops).unwrap();
            let n = s.tl.cues.len() + s.scenes.len() + 2;
            let args = match rng.below(4) {
                0 => json!({"index": rng.below(n), "cue": rng.below(n), "from": rng.below(n), "to": rng.name()}),
                1 => json!({"name": rng.name(), "to": rng.name(), "preset": rng.name(), "cue": rng.below(n)}),
                2 => json!({"cue": rng.below(n), "from": rng.below(n), "to": rng.below(n), "loop": rng.coin(),
                            "advance": *rng.pick(&["manual", "timer", "beats"]).unwrap(), "hold": false}),
                _ => json!({}),
            };
            let scenes_before = s.scenes.len();
            let result = s.call(op, args);
            // The open scene always points inside the library.
            assert!(s.current.is_none_or(|c| c < s.scenes.len()), "{op}");
            // Playback always sits on a cue that exists.
            let t = s.timeline();
            if t.active {
                assert!(t.current_cue < t.cues.len(), "{op}");
            }
            // Only save/duplicate/rename grow the library, only delete shrinks it.
            match (op, &result) {
                ("scene/save" | "scene/duplicate", Ok(_)) => assert!(s.scenes.len() <= scenes_before + 1),
                ("scene/delete", Ok(_)) => assert_eq!(s.scenes.len(), scenes_before - 1),
                _ => assert_eq!(s.scenes.len(), scenes_before, "{op} changed the library"),
            }
            // Library results name a scene that exists.
            if let (Ok(v), true) = (&result, matches!(op, "scene/save" | "scene/open" | "scene/rename" | "scene/duplicate")) {
                let i = v["index"].as_u64().unwrap() as usize;
                assert_eq!(s.scenes[i].0, v["name"].as_str().unwrap(), "{op}");
            }
        }
    });
}
