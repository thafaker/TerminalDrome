use std::{
    collections::HashMap,
    io::Cursor,
    sync::{Mutex, OnceLock},
};
use anyhow::Result;
use image::{
    imageops::{colorops::grayscale, FilterType},
    io::Reader as ImageReader,
};

use crate::api::endpoints::{
    build_auth_query_for_source, get_target_config, MusicSource,
};
use crate::api::models::Album;
use crate::config::Config;

lazy_static! {
    pub static ref COVER_CACHE: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
}

/// Cover width in terminal cells.
const COVER_CELLS_W: u32 = 30;

/// A terminal cell is roughly twice as tall as it is wide.
///
/// Dividing the cell count by this turns a pixel width into a cell height that
/// looks about square on screen. It is deliberately identical for every
/// [`CoverMode`] so that switching modes never changes the panel layout.
const CELL_ASPECT: f32 = 2.2;

/// Overrides the cover renderer. See [`CoverMode`] for the accepted values.
pub const COVER_MODE_ENV: &str = "TERMINALDROME_COVER";

/// How album art is turned into characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverMode {
    /// `" .:-=+*#%@"` — pure ASCII with ten tonal steps. Renders in any
    /// terminal, including one whose font has no block glyphs at all.
    Ascii,
    /// `" ░▒▓█"` — the Unicode block ramp with five tonal steps.
    Blocks,
    /// `" ▀▄█"` — two pixels stacked per cell, so twice the vertical detail at
    /// the same cell footprint. The sub-pixels come out square rather than
    /// stretched, which is the whole reason this is its own mode.
    HalfBlock,
}

impl CoverMode {
    /// Used when the environment variable is unset or names something unknown.
    ///
    /// `Blocks` is the default because it keeps tonal gradation. `HalfBlock`
    /// resolves shapes more sharply but is binary per sub-pixel, so a photo
    /// collapses into flat silhouettes rather than showing its tones.
    pub const DEFAULT: Self = Self::Blocks;

    /// Parses a mode name, ignoring case, surrounding space and `_`/`-`.
    /// Returns `None` for anything unrecognised.
    pub fn parse(value: &str) -> Option<Self> {
        let normalised = value.trim().to_ascii_lowercase().replace(['-', '_'], "");
        match normalised.as_str() {
            "ascii" | "a" => Some(Self::Ascii),
            "blocks" | "block" | "b" => Some(Self::Blocks),
            "halfblock" | "half" | "h" => Some(Self::HalfBlock),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ascii => "ascii",
            Self::Blocks => "blocks",
            Self::HalfBlock => "halfblock",
        }
    }
}

/// The active mode, read from the environment exactly once.
///
/// [`get_ascii_cover`] is called on every redraw for the prefetch, so the
/// lookup is cached rather than hitting the process environment each time.
pub fn cover_mode() -> CoverMode {
    static MODE: OnceLock<CoverMode> = OnceLock::new();
    *MODE.get_or_init(|| match std::env::var(COVER_MODE_ENV) {
        Ok(raw) => CoverMode::parse(&raw).unwrap_or_else(|| {
            eprintln!(
                "{COVER_MODE_ENV}: unknown cover mode {raw:?}, falling back to {}",
                CoverMode::DEFAULT.as_str()
            );
            CoverMode::DEFAULT
        }),
        Err(_) => CoverMode::DEFAULT,
    })
}

pub fn cover_cache_key(source: MusicSource, cover_id: &str) -> String {
    // The rendered art depends on the mode, so the mode has to be part of the
    // key. Without it, switching modes keeps serving the previously rendered
    // string out of the cache.
    format!("{:?}:{}:{}", source, cover_id, cover_mode().as_str())
}

pub async fn get_ascii_cover(
    album: Option<&Album>,
    source: MusicSource,
    config: &Config,
) -> String {
    let Some(album)    = album else { return default_cover_art(); };
    let Some(cover_id) = &album.cover_art else { return default_cover_art(); };

    let cache_key = cover_cache_key(source, cover_id);

    {
        let cache = COVER_CACHE.lock().unwrap();
        if let Some(cached) = cache.get(&cache_key) {
            return cached.clone();
        }
    }

    match fetch_cover_art(cover_id, source, config).await {
        Ok(img_data) => {
            let ascii = image_to_ascii(&img_data, COVER_CELLS_W).unwrap_or_else(|_| default_cover_art());
            COVER_CACHE.lock().unwrap().insert(cache_key, ascii.clone());
            ascii
        }
        Err(e) => {
            eprintln!("Error loading cover art ({:?}): {}", source, e);
            default_cover_art()
        }
    }
}

async fn fetch_cover_art(
    cover_id: &str,
    source: MusicSource,
    config: &Config,
) -> Result<Vec<u8>> {
    let target     = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("id", cover_id.to_string()));

    let response = reqwest::Client::new()
        .get(format!("{}/rest/getCoverArt", target.url))
        .query(&params)
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!(
            "getCoverArt HTTP {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        );
    }

    Ok(response.bytes().await?.to_vec())
}

/// Both ramps run from empty to solid, so a brighter pixel always yields more
/// ink.
///
/// The previous ramp was `" ░▒▓█@#S%?*+;:,."`, which concatenated the block ramp
/// with the reversed ASCII density ramp. Ink rose to `@` and then fell again,
/// which rendered near-white art almost blank and made ordinary covers look
/// noisy.
const ASCII_RAMP: [&str; 10] = [" ", ".", ":", "-", "=", "+", "*", "#", "%", "@"];
const BLOCK_RAMP: [&str; 5]  = [" ", "░", "▒", "▓", "█"];

/// Maps a normalised luminance onto a ramp index.
///
/// Kept separate so the monotonicity can be tested directly; a ramp that stops
/// being monotonic is the exact defect this replaced.
fn ramp_index(value: f32, len: usize) -> usize {
    debug_assert!(len > 0);
    let top = len - 1;
    (value.clamp(0.0, 1.0) * top as f32).round() as usize
}

fn luminance(gray: &image::GrayImage, x: usize, y: usize) -> f32 {
    gray.get_pixel(x as u32, y as u32)[0] as f32 / 255.0
}

fn mean_luminance(gray: &image::GrayImage) -> f32 {
    let raw = gray.as_raw();
    if raw.is_empty() {
        return 0.5;
    }
    (raw.iter().map(|&v| v as f64).sum::<f64>() / raw.len() as f64 / 255.0) as f32
}

/// Appends a row, padded to the full width so the block keeps a straight right
/// edge inside the panel.
fn pad_and_push(out: &mut String, line: &str, width: usize) {
    out.push_str(line);
    out.push_str(&" ".repeat(width.saturating_sub(line.chars().count())));
    out.push('\n');
}

/// One character per cell, quantised onto a monotonic ramp.
fn render_ramp(gray: &image::GrayImage, out: &mut String, ramp: &[&str]) {
    let (w, h) = (gray.width() as usize, gray.height() as usize);
    for y in 0..h {
        let mut line = String::with_capacity(w);
        for x in 0..w {
            line.push_str(ramp[ramp_index(luminance(gray, x, y), ramp.len())]);
        }
        pad_and_push(out, &line, w);
    }
}

/// Two source pixels per cell, stacked into `▀`, `▄` or `█`.
///
/// A full-height cell already provides 1:2 pixels, so single-sample modes
/// stretch every row vertically. Stacking two half-height blocks makes the
/// sub-pixels square, which both doubles the vertical detail and removes that
/// stretch, at an unchanged cell footprint.
fn render_half_block(gray: &image::GrayImage, out: &mut String) {
    let (w, h) = (gray.width() as usize, gray.height() as usize);
    // A fixed cut-off pushes every midtone to black or white. Tracking the
    // mean of the actual image keeps a dark cover dark and a bright one bright.
    let threshold = mean_luminance(gray);
    for y in (0..h).step_by(2) {
        let mut line = String::with_capacity(w);
        for x in 0..w {
            let top    = luminance(gray, x, y) > threshold;
            let bottom = if y + 1 < h { luminance(gray, x, y + 1) > threshold } else { top };
            line.push(match (top, bottom) {
                (true, true)   => '█',
                (true, false)  => '▀',
                (false, true)  => '▄',
                (false, false) => ' ',
            });
        }
        pad_and_push(out, &line, w);
    }
}

pub fn image_to_ascii(img_data: &[u8], cells_w: u32) -> Result<String> {
    render_cover(img_data, cells_w, cover_mode())
}

/// Renders cover art into exactly `cells_w` columns for the given mode.
///
/// Every mode yields the same number of rows so the surrounding layout is
/// unaffected by the choice of mode.
pub fn render_cover(img_data: &[u8], cells_w: u32, mode: CoverMode) -> Result<String> {
    let cells_w  = cells_w.max(1);
    let cells_h  = ((cells_w as f32 / CELL_ASPECT) as u32).max(1);
    // HalfBlock stacks two source pixels per cell, so it needs twice the rows.
    let sample_h = match mode {
        CoverMode::HalfBlock => cells_h * 2,
        _ => cells_h,
    };

    let gray = grayscale(
        &ImageReader::new(Cursor::new(img_data))
            .with_guessed_format()?
            .decode()?
            .resize_exact(cells_w, sample_h, FilterType::Triangle),
    );
    let (w, h) = (gray.width() as usize, gray.height() as usize);

    let mut out = String::with_capacity((w * h + h + 1) * 2);
    out.push_str(&" ".repeat(w));
    out.push('\n');

    match mode {
        CoverMode::HalfBlock => render_half_block(&gray, &mut out),
        CoverMode::Blocks    => render_ramp(&gray, &mut out, &BLOCK_RAMP),
        CoverMode::Ascii     => render_ramp(&gray, &mut out, &ASCII_RAMP),
    }
    Ok(out)
}

pub fn default_cover_art() -> String {
    r#"
   ___
  / __\_____   _____ _ __
 / /  / _ \ \ / / _ \ '__|
/ /__| (_) \ V /  __/ |
\____/\___/ \_/ \___|_|
  /\  /\___ _ __ ___
 / /_/ / _ \ '__/ _ \
/ __  /  __/ | |  __/
\/ /_/ \___|_|  \___|
    "#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes raw 8-bit luminance samples as a PNG so the public byte-oriented
    /// entry point can be exercised.
    fn png_of(samples: &[u8], w: u32, h: u32) -> Vec<u8> {
        let img = image::GrayImage::from_raw(w, h, samples.to_vec()).expect("valid buffer");
        let mut buf = Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .expect("png encoding");
        buf.into_inner()
    }

    fn solid(value: u8, w: u32, h: u32) -> Vec<u8> {
        png_of(&vec![value; (w * h) as usize], w, h)
    }

    /// A horizontal gradient, so every ramp step is exercised somewhere.
    fn gradient(w: u32, h: u32) -> Vec<u8> {
        let mut buf = Vec::with_capacity((w * h) as usize);
        for _ in 0..h {
            for x in 0..w {
                buf.push(((x as f32 / (w.max(2) - 1) as f32) * 255.0).round() as u8);
            }
        }
        png_of(&buf, w, h)
    }

    /// Top half white, bottom half black.
    fn split_horizontal(w: u32, h: u32) -> Vec<u8> {
        let mut buf = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            let v = if y < h / 2 { 255 } else { 0 };
            buf.resize((y * w + w) as usize, v);
        }
        png_of(&buf, w, h)
    }

    const MODES: [CoverMode; 3] = [CoverMode::Ascii, CoverMode::Blocks, CoverMode::HalfBlock];

    #[test]
    fn ramp_index_is_monotonic() {
        // Guards the defect this replaced: ink has to rise with brightness.
        for len in [ASCII_RAMP.len(), BLOCK_RAMP.len()] {
            let mut previous = 0;
            for step in 0..=100 {
                let index = ramp_index(step as f32 / 100.0, len);
                assert!(
                    index >= previous,
                    "ramp of length {len} went backwards at brightness {step}"
                );
                assert!(index < len, "index {index} out of range for length {len}");
                previous = index;
            }
        }
    }

    #[test]
    fn both_ramps_start_empty_and_end_solid() {
        assert_eq!(ASCII_RAMP[0].trim(), "");
        assert_eq!(BLOCK_RAMP[0].trim(), "");
        assert_eq!(*ASCII_RAMP.last().unwrap(), "@");
        assert_eq!(*BLOCK_RAMP.last().unwrap(), "█");
    }

    #[test]
    fn bright_art_renders_solid_in_blocks_mode() {
        // The regression guard: a white cover used to come out as ".".
        let out = render_cover(&solid(255, 16, 16), 8, CoverMode::Blocks).unwrap();
        assert!(out.contains('█'), "white art should render as full blocks");
        for row in out.lines() {
            for ch in row.chars() {
                assert!(
                    " ░▒▓█".contains(ch),
                    "blocks mode emitted {ch:?}, which is not a block glyph"
                );
            }
        }
    }

    #[test]
    fn dark_art_renders_empty_in_blocks_mode() {
        let out = render_cover(&solid(0, 16, 16), 8, CoverMode::Blocks).unwrap();
        for ch in out.chars() {
            assert_ne!(ch, '█', "black art should not render as full blocks");
            assert_ne!(ch, '▓', "black art should not render as dark shade");
        }
    }

    #[test]
    fn ascii_mode_emits_only_ascii() {
        // This is the whole point of the fallback: it has to survive a font
        // without block glyphs.
        for value in [0u8, 64, 128, 192, 255] {
            let out = render_cover(&solid(value, 16, 16), 8, CoverMode::Ascii).unwrap();
            assert!(
                out.is_ascii(),
                "ascii mode emitted a non-ASCII byte for brightness {value}"
            );
        }
    }

    #[test]
    fn every_mode_keeps_the_same_cell_grid() {
        // Switching modes must never move the surrounding layout.
        let expected = render_cover(&gradient(24, 24), 20, CoverMode::Blocks)
            .unwrap()
            .lines()
            .count();
        for mode in MODES {
            let lines = render_cover(&gradient(24, 24), 20, mode).unwrap();
            assert_eq!(lines.lines().count(), expected, "{mode:?} changed the row count");
            for row in lines.lines() {
                assert_eq!(row.chars().count(), 20, "{mode:?} row has the wrong width");
            }
        }
    }

    #[test]
    fn half_block_resolves_the_vertical_split() {
        // The resolution win: a top/bottom split lands in the upper sub-row,
        // which a single sample per cell could not represent.
        let out = render_cover(&split_horizontal(16, 16), 8, CoverMode::HalfBlock).unwrap();
        let rows: Vec<&str> = out.lines().collect();
        // Index 0 is the spacer line, so the cells start at 1.
        let first_cell = rows[1];
        let last_cell  = rows[rows.len() - 1];
        assert!(
            first_cell.contains('▀') || first_cell.contains('█'),
            "expected the bright top half in the first cell row, got {first_cell:?}"
        );
        assert!(
            last_cell.contains('▄') || last_cell.contains(' '),
            "expected the dark bottom half in the last cell row, got {last_cell:?}"
        );
    }

    #[test]
    fn half_block_sub_pixels_are_square() {
        // Two half-height blocks per cell means the sampled grid is twice as
        // tall as the cell count, which is what keeps rows from being stretched.
        let cells = 20u32;
        let rows  = ((cells as f32 / CELL_ASPECT) as u32).max(1);
        let out   = render_cover(&gradient(24, 24), cells, CoverMode::HalfBlock).unwrap();
        assert_eq!(out.lines().count(), rows as usize + 1, "one spacer row plus the cells");
    }

    #[test]
    fn every_line_is_padded_to_the_full_width() {
        for mode in MODES {
            let out = render_cover(&gradient(24, 24), 20, mode).unwrap();
            for row in out.lines() {
                assert_eq!(row.chars().count(), 20, "{mode:?} row was not padded");
            }
        }
    }

    #[test]
    fn parse_accepts_the_documented_names() {
        for name in ["ascii", "blocks", "halfblock", "HALF-BLOCK", " half_block ", "a", "h"] {
            assert!(CoverMode::parse(name).is_some(), "{name:?} should parse");
        }
        assert_eq!(CoverMode::parse("ASCII"), Some(CoverMode::Ascii));
        assert_eq!(CoverMode::parse("blocks"), Some(CoverMode::Blocks));
        assert_eq!(CoverMode::parse("halfblock"), Some(CoverMode::HalfBlock));
    }

    #[test]
    fn parse_rejects_anything_else() {
        for name in ["", "braille", "color", "42", "ünïcödé"] {
            assert_eq!(CoverMode::parse(name), None, "{name:?} should not parse");
        }
    }

    #[test]
    fn cache_key_carries_the_active_mode() {
        // Otherwise switching modes keeps serving the previous rendering.
        let key = cover_cache_key(MusicSource::Navidrome, "abc123");
        assert!(key.contains(cover_mode().as_str()), "cache key {key:?} lacks the mode");
        assert_ne!(
            key,
            cover_cache_key(MusicSource::Bandcamp, "abc123"),
            "cover ids collide between sources"
        );
    }

    #[test]
    fn the_default_mode_keeps_tonal_gradation() {
        // Guards the reason `halfblock` is not the default: a thresholded
        // silhouette is recognisable as a shape but loses every tone, so a
        // photo reads as one flat blob. The default has to spread a gradient
        // over several distinct ink levels.
        let out = render_cover(&gradient(32, 32), COVER_CELLS_W, CoverMode::DEFAULT).unwrap();
        let levels: std::collections::BTreeSet<char> =
            out.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            levels.len() >= 3,
            "the default mode only used {levels:?} — that is a silhouette, not gradation"
        );
    }

    #[test]
    fn a_broken_payload_falls_back_instead_of_panicking() {
        assert!(render_cover(b"not an image", 20, CoverMode::HalfBlock).is_err());
    }
}
