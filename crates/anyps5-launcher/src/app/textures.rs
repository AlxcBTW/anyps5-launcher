//! Game icons and artwork as egui textures: decoded on worker threads, downscaled to the size the
//! screen needs, uploaded on the UI thread when they arrive.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{ColorImage, Context, TextureHandle, TextureId, TextureOptions};

/// Decodes running at the same time.
const MAX_IN_FLIGHT: usize = 3;
/// Pass as `max_dim` to get the frosted-glass version of an image (see [`material`]).
pub const MATERIAL: u32 = 0;
/// White tint of the frosted artwork under a glass card (the original shows its cards clearly
/// lighter than the artwork around them; the glass shader uses 0.06 on the dark theme backdrop).
const MATERIAL_TINT: f32 = 0.30;

/// A texture ready to draw.
#[derive(Clone, Copy, Debug)]
pub struct Loaded {
    pub id: TextureId,
    /// Width over height of the decoded image.
    pub aspect: f32,
}

enum Entry {
    Queued,
    Loading,
    Ready(TextureHandle, f32),
    Failed,
}

struct Decoded {
    key: (PathBuf, u32),
    image: Option<ColorImage>,
    /// The frosted version, made in the same pass for artwork-sized requests.
    material: Option<ColorImage>,
}

/// Requests at least this big also deliver the [`MATERIAL`] version (the artwork behind the home screen).
const ARTWORK_DIM: u32 = 1024;

pub struct Textures {
    entries: HashMap<(PathBuf, u32), Entry>,
    queue: Vec<(PathBuf, u32)>,
    in_flight: usize,
    tx: Sender<Decoded>,
    rx: Receiver<Decoded>,
    ctx: Context,
}

impl Textures {
    pub fn new(ctx: &Context) -> Self {
        let (tx, rx) = channel();
        Self {
            entries: HashMap::new(),
            queue: Vec::new(),
            in_flight: 0,
            tx,
            rx,
            ctx: ctx.clone(),
        }
    }

    /// The image at `path`, decoded to at most `max_dim` pixels on its longer side. While it
    /// loads, any other size of the same file that is ready is returned instead. Asking for a file
    /// starts its decode; asking again is free.
    pub fn get(&mut self, path: &Path, max_dim: u32) -> Option<Loaded> {
        let key = (path.to_path_buf(), max_dim);
        let delivered_with_artwork = max_dim == MATERIAL
            && self.entries.iter().any(|((p, dim), entry)| {
                p == path && *dim >= ARTWORK_DIM && matches!(entry, Entry::Queued | Entry::Loading)
            });
        if !delivered_with_artwork && !self.entries.contains_key(&key) {
            self.entries.insert(key.clone(), Entry::Queued);
            self.queue.push(key.clone());
            self.pump();
        }
        if let Some(Entry::Ready(handle, aspect)) = self.entries.get(&key) {
            return Some(Loaded {
                id: handle.id(),
                aspect: *aspect,
            });
        }
        if max_dim == MATERIAL {
            return None;
        }
        self.entries
            .iter()
            .find_map(|((p, dim), entry)| match entry {
                Entry::Ready(handle, aspect) if p == path && *dim != MATERIAL => Some(Loaded {
                    id: handle.id(),
                    aspect: *aspect,
                }),
                _ => None,
            })
    }

    /// Uploads what the workers finished and starts the next decodes. Call once per frame.
    pub fn poll(&mut self) {
        while let Ok(done) = self.rx.try_recv() {
            self.in_flight = self.in_flight.saturating_sub(1);
            let entry = match done.image {
                Some(image) => {
                    let aspect = image.size[0] as f32 / image.size[1].max(1) as f32;
                    let name = format!("{}@{}", done.key.0.display(), done.key.1);
                    Entry::Ready(
                        self.ctx.load_texture(name, image, TextureOptions::LINEAR),
                        aspect,
                    )
                }
                None => Entry::Failed,
            };
            if let Some(material) = done.material {
                let aspect = material.size[0] as f32 / material.size[1].max(1) as f32;
                let name = format!("{}@material", done.key.0.display());
                let handle = self
                    .ctx
                    .load_texture(name, material, TextureOptions::LINEAR);
                self.entries
                    .insert((done.key.0.clone(), MATERIAL), Entry::Ready(handle, aspect));
            }
            self.entries.insert(done.key, entry);
            self.ctx.request_repaint();
        }
        self.pump();
    }

    /// Puts an already decoded image in the cache (previews and tests).
    pub fn insert(&mut self, path: &Path, max_dim: u32, image: ColorImage) {
        let aspect = image.size[0] as f32 / image.size[1].max(1) as f32;
        let key = (path.to_path_buf(), max_dim);
        let name = format!("{}@{}", path.display(), max_dim);
        let handle = self.ctx.load_texture(name, image, TextureOptions::LINEAR);
        self.entries.insert(key, Entry::Ready(handle, aspect));
    }

    fn pump(&mut self) {
        while self.in_flight < MAX_IN_FLIGHT && !self.queue.is_empty() {
            let key = self.queue.remove(0);
            self.entries.insert(key.clone(), Entry::Loading);
            self.in_flight += 1;
            let tx = self.tx.clone();
            let ctx = self.ctx.clone();
            std::thread::Builder::new()
                .name("decode".into())
                .spawn(move || {
                    let (image, material) = decode(&key.0, key.1);
                    let _ = tx.send(Decoded {
                        key,
                        image,
                        material,
                    });
                    ctx.request_repaint();
                })
                .ok();
        }
    }
}

/// Reads a PNG and scales it down so that its longer side is at most `max_dim`. Artwork-sized
/// requests also return the frosted version, made from the same decode. `max_dim == MATERIAL`
/// returns only the frosted version as the first element.
fn decode(path: &Path, max_dim: u32) -> (Option<ColorImage>, Option<ColorImage>) {
    let image = match image::open(path) {
        Ok(image) => image.to_rgba8(),
        Err(error) => {
            tracing::warn!("cannot read {}: {error}", path.display());
            return (None, None);
        }
    };
    if max_dim == MATERIAL {
        return (Some(material(&image)), None);
    }
    let frosted = (max_dim >= ARTWORK_DIM).then(|| material(&image));
    let (w, h) = image.dimensions();
    let longer = w.max(h);
    let image = if longer > max_dim {
        let factor = max_dim as f32 / longer as f32;
        let (nw, nh) = (
            ((w as f32 * factor).round() as u32).max(1),
            ((h as f32 * factor).round() as u32).max(1),
        );
        // Bilinear is enough for the big artwork and a lot faster than a cubic kernel.
        let filter = if max_dim >= ARTWORK_DIM {
            image::imageops::FilterType::Triangle
        } else {
            image::imageops::FilterType::CatmullRom
        };
        image::imageops::resize(&image, nw, nh, filter)
    } else {
        image
    };
    let (w, h) = image.dimensions();
    (
        Some(ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            image.as_raw(),
        )),
        frosted,
    )
}

/// The frosted-glass version of an image: 80 px on the long side, blurred, saturation 1.25 and a
/// 6 % white tint (the material of `glass.wgsl`, baked). Glass cards over the game artwork sample
/// it through the same uv rectangle as the artwork itself.
fn material(image: &image::RgbaImage) -> ColorImage {
    let (w, h) = image.dimensions();
    let factor = 80.0 / w.max(h).max(1) as f32;
    let (nw, nh) = (
        ((w as f32 * factor).round() as u32).max(1),
        ((h as f32 * factor).round() as u32).max(1),
    );
    let small = image::imageops::thumbnail(image, nw, nh);
    let blurred = image::imageops::blur(&small, 1.0);
    let mut pixels = Vec::with_capacity((nw * nh) as usize);
    for p in blurred.pixels() {
        let c = [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])];
        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let out = c.map(|v| {
            let saturated = (luma + (v - luma) * 1.25).clamp(0.0, 255.0);
            (saturated + (255.0 - saturated) * MATERIAL_TINT) as u8
        });
        pixels.push(egui::Color32::from_rgb(out[0], out[1], out[2]));
    }
    ColorImage {
        size: [nw as usize, nh as usize],
        source_size: egui::vec2(nw as f32, nh as f32),
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_scales_down_and_keeps_the_aspect() {
        let dir = std::env::temp_dir().join(format!("anyps5-tex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("wide.png");
        image::RgbaImage::from_pixel(800, 400, image::Rgba([10, 20, 30, 255]))
            .save(&file)
            .unwrap();
        let decoded = decode(&file, 400).0.unwrap();
        assert_eq!(decoded.size, [400, 200]);
        assert!(decode(&dir.join("missing.png"), 400).0.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod real_file_tests {
    use super::*;

    /// `ANYPS5_REAL_PNG=<file>`: a big picture decodes at every size the screens ask for.
    #[test]
    fn big_pictures_decode_if_given() {
        let Some(file) = std::env::var_os("ANYPS5_REAL_PNG") else {
            return;
        };
        let file = std::path::PathBuf::from(file);
        for dim in [2048, 400, MATERIAL] {
            let started = std::time::Instant::now();
            let image = decode(&file, dim).0.expect("decodes");
            eprintln!("{dim}: {:?} in {:?}", image.size, started.elapsed());
        }
    }
}
