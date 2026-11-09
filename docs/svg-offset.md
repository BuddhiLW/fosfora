# SVG Offset

Native layer effect in **Catalog → Pattern → SVG Offset**. The default is an original eight-leaf ornament; no external artwork is required.

## Try from the isolated worktree

```sh
cd /home/klein/PP/fosfora/.claude/worktrees/fosfora-svg-offset
timeout 1500 cargo build
# Launch only when you want a separate instance, not over an existing performance:
./target/debug/fosfora
```

Select SVG Offset in the Catalog. In its contextual **SVG Offset** section, click **Load SVG…** to choose a file. **Reload** rereads the current file; **Default ornament** restores the bundled artwork. Source path and geometry settings round-trip in presets. Missing files retain the fallback and report the load error.

Bindable Parameters:
- Amount, Weight and Shrink set contour density, thickness and silhouette confinement. Shrink below 1 permits outer offsets.
- Speed and Phase control crawling. Mode: `0` Continuous, `1` Bounce.
- Color mode: `0` Greyscale, `1` Rainbow, `2` White, `3` Original.
- Order: `0` Radial, `1` Vertical, `2` Group, `3` Random.
- Falloff, Boost, Hue speed, Hue phase and Scale complete the look.

Speed and Hue speed are CPU-integrated rates, so changing a speed does not multiply the change by the session's uptime. Quality (Full/Half/Quarter) changes field resolution, not animation speed. Transform, background (Alpha/Black/White) and keyer are in the contextual section.

## Geometry and cache

usvg 0.45 normalizes primitives, curves, transforms and uses. tiny-skia 0.11.4 rasterizes opaque per-shape IDs in painter order, including fill rules and stroke geometry. The GPU caches inside/outside JFA+1 fields and a per-shape inradius reduction. Changing performance parameters does not rebuild the field; loading/reloading artwork, changing quality or resizing does.

This is a geometry effect, not a full SVG compositor: filters, clipping/masks, raster images and text without vector outlines are not reproduced. Convert text to paths first. Original color is one representative color per shape (first gradient stop for gradients). Overlapping shapes use the visible painter-ordered mask; obscured portions are not independently offset. JFA+1 is an approximate distance transform, particularly for adversarial thin or fragmented shapes.

## Verification

```sh
cargo test -p fosfora-app
cargo test -p fosfora-app --lib gpu::svg_offset -- --include-ignored --nocapture
cargo clippy -p fosfora-app --all-targets --features analyze
```

The ignored SVG test requires a GPU. It reads the actual Rgba32Float field back and compares signed distances, IDs and inradius to the CPU reference. Set `SVG_OFFSET_CAPTURE=/tmp/svg-capture` for optional native-render PNG frames used by the catalog previews. GPU probes run in their own process; they do not attach to a running fosfora instance.
