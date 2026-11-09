# SVG Offset: implementation plan (work in progress)

Spec: hive memory 20261108205043-257c55ee. Rate rules: 20261108035519-065dd5a5.

## Design (follows the Helix pattern, least invasive)
1. `assets/effects/svg_offset.pfx`: "SVG Offset", category "pattern". It has one trivial pass that writes transparent, plus a new optional top-level
   block `"svg_offset": {}` on `PfxEffect` (format.rs; add it to the diff/test constructors). If the block is present, the layer gets an `SvgOffsetSim`.
2. `gpu/svg_offset.rs`: `SvgOffsetSim`, held as `Option<SvgOffsetSim>` on `PassExecutor` next to `particle_system`.
   It renders after the passes into the final target with its own pipeline and uniform block, so it is not limited to 16 slots.
   Mirror its resize/flip hooks in `PassExecutor::resize`.
3. CPU side: `usvg = { version = "0.45", default-features = false }`, which matches the tiny-skia 0.11.4 already in Cargo.lock.
   Walk `tree.root()` recursively and record a group index per `<g>` (usvg keeps groups that have an id or transform).
   Each `Path` with a fill becomes a shape. A stroke-only path is turned into a fill with `path.stroke(&stroke.to_tiny_skia(), 1.0)`.
   Rasterize a non-antialiased id mask, one u32 per pixel (id+1, 0 = background). Use our own even-odd/nonzero scanline over flattened
   polylines, testable without a GPU, at output resolution x Quality (1, 0.5, 0.25).
   Per-shape metadata: fill rgb, group, centroid, bbox, area.
4. GPU side: compute passes that run only when the SVG, quality or resolution changes.
   - Seed pass: boundary pixels, where the id differs from a neighbour.
   - JFA: log2(N) steps plus one extra step of 1, ping-ponging rg32 seed coordinates.
   - Signed distance: negative inside.
   - Inradius: atomicMax per id of |d| into a storage buffer.
   - Finalize into an rgba16float field holding (d, id, d/inradius, 0).
   Keep a CPU reference JFA alongside, checked against brute force in tests.
5. Shading fragment pass: the rings from the spec, Continuous or Bounce, the Greyscale/Rainbow/White/Original palettes,
   the Radial/Vertical/Group/Random order keys, falloff, boost, the keyer, the transform, and the Alpha/Black/White background.
6. Bindable `.pfx` inputs (13): amount, weight, shrink, speed, phase, falloff, boost, hue_speed, h_phase,
   key_width, k_speed, k_level, scale. `"rates": ["speed","hue_speed","k_speed"]` puts their integrals in slots 13, 14 and 15.
   They are read CPU-side in `frame_prep`, as Helix reads its params.
7. Contextual panel `ui/panels/svg_offset_panel.rs`, wired like helix_panel (shell.rs effect_sections, ShellState,
   shell_harness, main.rs info + `svg_offset_cmd` temp data): Load SVG (rfd), shape count, mode, colour, order, k_phase,
   k_soft, k_repeat, offset x/y, rotate, quality, background.
8. Presets: `LayerPreset.svg_offset: Option<SvgOffsetPreset { path, settings }>`.
   Capture it in app.rs save_preset and restore it after the effect load, in app.rs and in headless/scene_renderer.rs.
9. Default art: our own procedural ornament in `assets/svg/ornament.svg`, embedded with include_str! as the fallback.
10. Shipped-effect tests also need `assets/thumbs/svg_offset.webp` and `.anim.webp`
    (catalog_panel tests: every_shipped_effect_has_a_picture and every_shipped_effect_has_a_hover_preview), produced with
    scripts/build_thumbnails.py. Otherwise mark the effect hidden=false and generate the thumbs.
    Check the templates test as well: every audio template must bind something.

## Implementation status
- Implemented in `91a0b95`, `049002b`, `9a3fb6b`, `981662a`; native GPU catalog previews in `1887e75`.
- The implementation follows the required Rgba32Float field (not the rgba16float proposal above) and uses tiny-skia directly for rasterization.
- Required mode/color/order enums are real Int inputs, using the existing f32 uniform/value ABI. Thirteen input slots plus speed/hue-speed integrals occupy slots 0..14.
- Presets carry serde-defaulted `svg_offset_path` and `svg_offset_settings` fields.
- Usage, limitations, build and test commands: [SVG Offset](svg-offset.md).
