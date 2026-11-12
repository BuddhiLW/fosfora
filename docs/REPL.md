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
| `trama` | effect chains on each layer and on the master: nodes, wires, params, modulation, bypass, import and export |
| `bind` | the Binding Matrix, LFOs and templates, audio input, MIDI and OSC mappings |
| `sys` | tempo and Link, recording, outputs, output window, settings, theme, overlay, triggers, quit |

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
(def k (trama/add! "kaleido" :chain 0))
(trama/connect! "layer-input" (:id k) :chain 0)
(trama/connect! (:id k) "output" :chain 0)
(bind/add! "audio.kick" "layer.0.opacity" :smoothing 0.3)
(sys/link! :enabled true :quantum 4)
(sys/record!)
```

Some ops do things the namespaces above only touch in passing:

| op | helper | what it does |
|---|---|---|
| `:layer/add {:at n}` | `(layer/add! :at 0)` | add an empty layer at position `n` (0 is the top) and select it, as dropping between layers does; without `:at` it goes at the bottom |
| `:layer/shader-source` | `(layer/shader-source 1)` | the WGSL a layer's effect runs: `:source` (first pass) and `:passes` |
| `:layer/loading` | `(layer/loading)` | media files still decoding into new layers: index, file, frames done and total |
| `:layer/cancel-media {:load i}` | `(layer/cancel-media! "intro")` | stop one of those, by index or file name (none needed when only one loads) |
| `:particle/loading` | `(particle/loading)` | particle source, obstacle and morph files still loading |
| `:particle/cancel-load {:load i}` | `(particle/cancel-load!)` | stop one of those, by index or file name |
| `:layer/postfx-previous`, `:layer/postfx-defaults` | `(layer/postfx-previous!)` | the post-processing panel's other two resets |
| `:scene/add-cue {:at n}`, `:scene/move-cue`, `:scene/set-cue {:label s}` / `{:hold false}` | `(scene/add-cue! "Tunnel" :at 0)` | what the cue strip does: drop a preset between cues, drag a cue, name it, make a Timer cue wait for Go |

```clojure
(layer/add! :at 0)                           ; a new empty layer on top
(println (layer/shader-source))              ; the active layer's shader
(layer/media! "~/loops/long.mov")
(layer/loading)                              ;=> [{:index 0 :file "long.mov" :done 40 :total 900 ...}]
(layer/cancel-media!)
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
