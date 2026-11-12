# Live coding fosfora from a REPL

A build with the `repl` feature can run an nREPL server inside fosfora. Any
nREPL client can then drive the running show: CIDER, Calva, Conjure, or
`clj -M:nrepl`. Everything the interface can do is available from the REPL:
presets, scenes and cues, layers, effects, chains, bindings, outputs and
recording.

```sh
cargo build --release --features repl,video
FOSFORA_NREPL=7888 ./target/release/fosfora   # 0 picks a free port
```

fosfora writes `.nrepl-port` in its working directory, so editors connect
on their own. The server listens on `127.0.0.1` only.

## The two layers

**Ops.** Every interface action is a named op. Each op runs on the render
thread between frames and returns its result as data. If the action fails,
the op throws with the same message the interface shows.

```clojure
(f/op :preset/save {:name "Night Drive"})   ;=> {:index 7 :name "Night Drive"}
(f/ops)                                     ; every op: name, addon, args, doc
(f/help "scene")                            ; the scene ops, one per line
(f/addons)                                  ; ["core" "preset" "scene" ...]
```

**Namespaces.** Each area has a friendlier namespace over its ops. All of
them are already required in `user`:

| alias | what it drives |
|---|---|
| `preset` | save, load (with the default transition, or `:cut true`), next/prev, copy, delete, the default transition |
| `scene` | the scene library and its cue timeline: save, open, play, cues, advance, loop |
| `layer` | the stack, effects, params (real units or normalized), opacity, blend, media transport, cameras, post-processing |
| `particle` | particle simulation, sources, obstacles, morph targets, lattice and helix, volumetric look, quality |
| `svg` | the SVG Offset layer: its SVG file (or the built-in ornament), reload, quality, background, offset, rotation, keyer |
| `trama` | effect chains on each layer and on the master: nodes, wires, params, modulation, bypass, import and export |
| `bind` | the Binding Matrix, LFOs and templates, audio input, MIDI and OSC mappings, the OSC listener and sender |
| `sys` | tempo and Link, recording, outputs, output window, settings, themes and theme files, interface scale, web remote, tours, overlay, triggers, quit |

Setup and Appearance ops:

| op | REPL | what it does |
|---|---|---|
| `bindings/osc-config` | `(bind/osc-config! :port 9000 :lan true)` | the OSC listener: port, other devices, on/off; restarts it |
| `bindings/osc-send` | `(bind/osc-send! :enabled true :host "10.0.0.5" :port 9001 :rate 30 :prefix :fosfora)` | sending the analysis to another app |
| `system/web` | `(sys/web! :port 9090 :lan false)` | the web remote; a new port restarts it |
| `system/ui-scale` | `(sys/ui-scale! 1.25)` | interface scale, 0.8 to 2.0 |
| `system/theme-reload` | `(sys/reload-themes!)` | read the themes folder again |
| `system/theme-save-copy` | `(sys/save-theme-copy!)` | save the theme in use as a file to edit, and use it |
| `system/theme-folder` | `(sys/theme-folder)`, `(sys/open-theme-folder!)` | where theme files live; open it |
| `system/tour` | `(sys/tour! "layers")`, `(sys/tour! :end)`, `(sys/tour)` | start, end or read the guided tour |
| `system/quit` | `(sys/quit!)`, `(sys/quit! :confirm)`, `(sys/quit! :cancel)` | the quit dialog |

Ports take 1024 to 65535. Without arguments, each of these ops reports the
current value and changes nothing.

Anything that picks one item takes an index or a name. A name can be any
unique part of it, in any case.

```clojure
(preset/load! "night")                       ; default transition
(preset/load! 3 :cut true)
(scene/save! "Set B")                        ; a new name starts a blank cue list
(scene/add-cue! "Tunnel" :at 0)
(scene/set-cue! 1 :transition :dissolve :secs 2.0 :hold 8)
(scene/play!)
(layer/load! "Sumi" :layer 2)
(layer/set-param! :speed 2.5)                ; refused outside the param's range
(layer/blend! 0 :screen)
(svg/load! "~/art/logo.svg")                 ; on an SVG Offset layer; no path = built-in ornament
(svg/settings! :quality :half :background :black :rotation 45)
(svg/keyer! :width 0.5 :repeats 4)
(def k (trama/add! "kaleido" :chain 0))
(trama/connect! "layer-input" (:id k) :chain 0)
(trama/connect! (:id k) "output" :chain 0)
(bind/add! "audio.kick" "layer.0.opacity" :smoothing 0.3)
(sys/link! :enabled true :quantum 4)
(sys/record!)
```

The older helpers in `f/` still work: `f/state`, `f/audio`, `f/set-param!`,
`f/trigger!`, `f/data!`, `f/media!`, `f/put-trama!`. They send the same
messages as the web remote.

## Your own scripts

At startup the REPL evaluates every `*.cljrs` file in
`~/.config/fosfora/repl/`, in name order, into `user`. Set
`FOSFORA_REPL_INIT` to use another folder. A broken script is logged and
skipped, so the REPL still starts.

```clojure
;; ~/.config/fosfora/repl/10-show.cljrs
(defn drop! []
  (preset/load! "Drop" :cut true)
  (layer/opacity! 0 1.0)
  (sys/trigger! :toggle-postprocess))
```

## Adding ops from Rust: `ControlAddon`

An embedder or a fork adds its own ops by implementing `ControlAddon`. An
addon is a set of ops plus an optional REPL namespace.

```rust
use fosfora_app::control::{ControlAddon, OpSpec, OpResult, CljrsNs};
use fosfora_app::app::App;
use serde_json::{json, Value};

struct Strobe;

impl ControlAddon for Strobe {
    fn id(&self) -> &'static str { "strobe" }
    fn ops(&self) -> Vec<OpSpec> {
        vec![OpSpec::new("strobe/flash", "{:layer int}", "Flash a layer once.", flash)]
    }
}

fn flash(app: &mut App, args: &Value) -> OpResult {
    let layer = args["layer"].as_u64().ok_or("missing :layer")? as usize;
    // ... act on `app` ...
    Ok(json!({"layer": layer}))
}

// once the app exists:
app.register_addon(&Strobe);
```

Built-in addons live in `crates/fosfora-app/src/control/ops/`, one folder per
area. A later addon can replace an op by registering the same name. Ops
registered at runtime can be called with `(f/op ...)` right away. A
namespace an addon ships is preloaded only for the built-in addons.

Every action the interface raises must be reachable as an op. A test
(`control::coverage`) reads `main.rs` and fails if an action has no op and
no written reason for having none.

## Testing against a live instance

```sh
FOSFORA_NREPL=7888 cargo run --features repl,video &
FOSFORA_IT_NREPL=7888 cargo test --features repl,video --test repl_ops
```

These tests drive the instance over nREPL. Anything they create they also
remove, such as a uniquely named preset or binding, and anything they
change they put back, such as a layer's opacity.
