//! Runtime SVG geometry and cached distance fields for the SVG Offset layer.
//!
//! IDs are painter-ordered: 0 is background, shape N is N+1. usvg normalizes
//! curves, primitives, transforms and uses; tiny-skia rasterizes fills and strokes.

use bytemuck::{Pod, Zeroable};
use tiny_skia::{FillRule, Paint, Path, Pixmap, Transform};

pub const DEFAULT_SVG: &str = include_str!("../../../../assets/svg/ornament.svg");
const MAX_SHAPES: usize = 65_535;

/// Storage-buffer record. Index zero is a dummy background entry.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable)]
pub struct ShapeMeta {
    pub color: [f32; 4],
    /// Centroid x/y (normalized canvas), group index, visible area (pixels).
    pub geometry: [f32; 4],
    pub bounds: [f32; 4],
}

struct Shape {
    parts: Vec<(Path, FillRule)>,
    meta: ShapeMeta,
}

pub struct SvgDocument {
    shapes: Vec<Shape>,
    size: [f32; 2],
}

impl SvgDocument {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
            return Err("SVG exceeds the 16 MiB geometry limit".into());
        }
        Self::parse(&std::fs::read(path).map_err(|e| e.to_string())?)
    }

    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let tree = usvg::Tree::from_data(data, &usvg::Options::default())
            .map_err(|e| format!("Invalid SVG: {e}"))?;
        let mut doc = Self {
            shapes: Vec::new(),
            size: [tree.size().width(), tree.size().height()],
        };
        doc.walk(tree.root(), 0, &mut 0, 1.0)?;
        if doc.shapes.is_empty() {
            return Err("SVG contains no visible vector fills or strokes".into());
        }
        Ok(doc)
    }

    pub fn shape_count(&self) -> usize { self.shapes.len() }

    fn walk(&mut self, group: &usvg::Group, group_id: u32, next_group: &mut u32, opacity: f32) -> Result<(), String> {
        let opacity = opacity * group.opacity().get();
        if opacity == 0.0 { return Ok(()); }
        for node in group.children() {
            match node {
                usvg::Node::Group(child) => {
                    *next_group += 1;
                    self.walk(child, *next_group, next_group, opacity)?;
                }
                usvg::Node::Path(path) if path.is_visible() => {
                    let mut parts = Vec::new();
                    let mut color = [1.0; 4];
                    if let Some(fill) = path.fill().filter(|f| f.opacity().get() > 0.0) {
                        if let Some(data) = path.data().clone().transform(path.abs_transform()) {
                            let rule = match fill.rule() {
                                usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                                usvg::FillRule::NonZero => FillRule::Winding,
                            };
                            parts.push((data, rule));
                        }
                        color = paint_color(fill.paint(), fill.opacity().get() * opacity);
                    }
                    if let Some(stroke) = path.stroke().filter(|s| s.opacity().get() > 0.0) {
                        if let Some(data) = path.data().stroke(&stroke.to_tiny_skia(), 1.0)
                            .and_then(|p| p.transform(path.abs_transform())) {
                            parts.push((data, FillRule::Winding));
                        }
                        if path.fill().is_none() {
                            color = paint_color(stroke.paint(), stroke.opacity().get() * opacity);
                        }
                    }
                    if !parts.is_empty() {
                        if self.shapes.len() == MAX_SHAPES { return Err("SVG exceeds the 65535-shape limit".into()); }
                        self.shapes.push(Shape { parts, meta: ShapeMeta {
                            color, geometry: [0.0, 0.0, group_id as f32, 0.0], ..ShapeMeta::default()
                        }});
                    }
                }
                // Raster images and text without vector outlines are not shapes.
                _ => {}
            }
        }
        Ok(())
    }

    /// Fit the SVG viewport, preserving aspect and one empty border pixel. IDs
    /// are opaque RGB with AA off, not blended colors (including >255 shapes).
    pub fn rasterize(&self, width: u32, height: u32) -> Result<IdMask, String> {
        if width < 3 || height < 3 || width > 8192 || height > 8192 {
            return Err("SVG mask dimensions must be in 3..=8192".into());
        }
        let mut pixmap = Pixmap::new(width, height).ok_or("Cannot allocate SVG mask")?;
        let scale = ((width - 2) as f32 / self.size[0]).min((height - 2) as f32 / self.size[1]);
        let transform = Transform::from_row(scale, 0.0, 0.0, scale,
            (width as f32 - self.size[0] * scale) * 0.5, (height as f32 - self.size[1] * scale) * 0.5);
        let mut metadata = vec![ShapeMeta::default()];
        for (index, shape) in self.shapes.iter().enumerate() {
            let id = index as u32 + 1;
            let mut paint = Paint { anti_alias: false, ..Paint::default() };
            paint.set_color_rgba8(id as u8, (id >> 8) as u8, (id >> 16) as u8, 255);
            for (path, rule) in &shape.parts { pixmap.fill_path(path, &paint, *rule, transform, None); }
            let mut meta = shape.meta;
            meta.bounds = [1.0, 1.0, 0.0, 0.0];
            metadata.push(meta);
        }
        let ids: Vec<u32> = pixmap.pixels().iter().map(|p| {
            u32::from(p.red()) | (u32::from(p.green()) << 8) | (u32::from(p.blue()) << 16)
        }).collect();
        for (i, &id) in ids.iter().enumerate().filter(|(_, id)| **id != 0) {
            let x = (i as u32 % width) as f32 / width as f32;
            let y = (i as u32 / width) as f32 / height as f32;
            let m = &mut metadata[id as usize];
            m.geometry[0] += x; m.geometry[1] += y; m.geometry[3] += 1.0;
            m.bounds[0] = m.bounds[0].min(x); m.bounds[1] = m.bounds[1].min(y);
            m.bounds[2] = m.bounds[2].max(x); m.bounds[3] = m.bounds[3].max(y);
        }
        for m in &mut metadata[1..] {
            let area = m.geometry[3].max(1.0);
            m.geometry[0] /= area; m.geometry[1] /= area;
        }
        Ok(IdMask { width, height, ids, metadata })
    }
}

fn paint_color(paint: &usvg::Paint, opacity: f32) -> [f32; 4] {
    // One representative original color per shape; gradients use the first stop.
    // Geometry deliberately does not describe SVG filter/clip compositing.
    let color = match paint {
        usvg::Paint::Color(c) => *c,
        usvg::Paint::LinearGradient(g) => g.stops()[0].color(),
        usvg::Paint::RadialGradient(g) => g.stops()[0].color(),
        usvg::Paint::Pattern(_) => usvg::Color::white(),
    };
    [color.red as f32 / 255.0, color.green as f32 / 255.0, color.blue as f32 / 255.0, opacity]
}

pub struct IdMask {
    pub width: u32,
    pub height: u32,
    pub ids: Vec<u32>,
    pub metadata: Vec<ShapeMeta>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document(body: &str) -> SvgDocument {
        SvgDocument::parse(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">{body}</svg>"#).as_bytes()).unwrap()
    }
    /// Integer boundary seeds, Euclidean metric, JFA+1 (same schedule as GPU).
    fn jfa(mask: &IdMask, inside: bool) -> Vec<Option<usize>> {
        let w = mask.width as i32;
        let h = mask.height as i32;
        let mut seeds = vec![None; mask.ids.len()];
        for y in 0..h { for x in 0..w {
            let i = (y*w+x) as usize;
            if mask.ids[i] != 0 && [(1,0),(-1,0),(0,1),(0,-1)].iter().any(|(dx,dy)| {
                let (nx, ny) = (x+dx, y+dy);
                nx < 0 || ny < 0 || nx >= w || ny >= h || mask.ids[(ny*w+nx) as usize] != mask.ids[i]
            }) { seeds[i] = Some(i); }
        }}
        let mut step = mask.width.max(mask.height).next_power_of_two() / 2;
        let mut steps = Vec::new();
        while step > 0 { steps.push(step as i32); step /= 2; }
        steps.push(1);
        for step in steps {
            let mut next = seeds.clone();
            for y in 0..h { for x in 0..w {
                let i = (y*w+x) as usize;
                let d2 = |s: usize| { let dx = s as i32 % w-x; let dy = s as i32 / w-y; dx*dx+dy*dy };
                for dy in -1..=1 { for dx in -1..=1 {
                    let (nx, ny) = (x+dx*step, y+dy*step);
                    if nx < 0 || ny < 0 || nx >= w || ny >= h { continue; }
                    if let Some(s) = seeds[(ny*w+nx) as usize] {
                        if inside && mask.ids[s] != mask.ids[i] { continue; }
                        if next[i].is_none_or(|old| d2(s) < d2(old)) { next[i] = Some(s); }
                    }
                }}
            }}
            seeds = next;
        }
        seeds
    }

    #[test]
    fn cpu_jfa_matches_brute_force_small_grid() {
        let mask = document(r#"<rect x="2" y="2" width="7" height="13"/><circle cx="14" cy="9" r="4"/>"#).rasterize(26, 26).unwrap();
        for inside in [false, true] {
            let result = jfa(&mask, inside);
            let boundary: Vec<usize> = result.iter().enumerate().filter_map(|(i,s)| (*s == Some(i)).then_some(i)).collect();
            for (i, seed) in result.iter().enumerate() {
                if inside && mask.ids[i] == 0 { continue; }
                let d2 = |s: usize| { let dx = (s % 26) as i32-(i % 26) as i32; let dy = (s / 26) as i32-(i / 26) as i32; dx*dx+dy*dy };
                let expected = boundary.iter().filter(|&&s| !inside || mask.ids[s] == mask.ids[i]).map(|&s| d2(s)).min().unwrap();
                assert_eq!(d2(seed.unwrap()), expected, "pixel {i}, inside={inside}");
            }
        }
    }

    #[test]
    fn svg_load_and_flatten() {
        let doc = document(r##"<defs><rect id="r" width="4" height="4"/></defs><g id="a" transform="translate(3 2)"><use href="#r"/><path d="M0 10 Q5 0 10 10" fill="none" stroke="red"/></g>"##);
        assert_eq!(doc.shape_count(), 2);
        assert!(doc.shapes.iter().all(|s| !s.parts.is_empty()));
        assert!(SvgDocument::parse(DEFAULT_SVG.as_bytes()).unwrap().shape_count() > 5);
        assert!(SvgDocument::parse(b"broken").is_err());
        assert!(doc.rasterize(0, 1).is_err());
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), DEFAULT_SVG).unwrap();
        assert!(SvgDocument::load(file.path()).is_ok());
    }
    #[test]
    fn id_mask_preserves_overlap_holes_transforms_and_strokes() {
        let doc = document(r##"<rect width="20" height="20" fill="#ff0000"/><g transform="translate(4 4)"><path fill="blue" fill-rule="evenodd" d="M0 0H12V12H0Z M4 4H8V8H4Z"/></g><path d="M2 2H18" stroke="lime" stroke-width="2"/>"##);
        let mask = doc.rasterize(22, 22).unwrap();
        let at = |x, y| mask.ids[y * 22 + x];
        assert_eq!(at(0, 0), 0); assert_eq!(at(2, 18), 1); assert_eq!(at(6, 6), 2);
        assert_eq!(at(11, 11), 1); assert_eq!(at(11, 3), 3);
        assert!(mask.metadata[2].geometry[3] > 0.0);
        assert_eq!(mask.metadata[1].color, [1.0, 0.0, 0.0, 1.0]);
    }
}
