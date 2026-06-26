use anyhow::Result;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders},
    Frame,
};

/// Fullscreen Audio Visualizer.
///
/// Audio pipeline (Linux + macOS, no loopback device needed):
///
///   mpv  ──plays──▶  speakers  (untouched, no restart)
///   ffmpeg ──reads same URL──▶  raw s16le PCM ──▶  named FIFO
///   cava  ──reads FIFO──▶  bar levels ──▶  this widget
///
/// Fallback: if cava or ffmpeg is missing, a demo animation runs.
#[derive(Debug)]
pub struct Visualizer {
    bars: usize,
    levels: Vec<f32>,
    last_tick: Instant,
    phase: f32,
    pub fps: u32,

    // cava
    cava_child: Option<Child>,
    cava_reader: Option<std::thread::JoinHandle<()>>,
    shared_levels: std::sync::Arc<std::sync::Mutex<Vec<f32>>>,
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    last_cava_frame: std::sync::Arc<std::sync::Mutex<Option<Instant>>>,

    // ffmpeg feeder (separate process, reads stream URL → FIFO)
    ffmpeg_child: Option<Child>,

    // FIFO path
    fifo_path: Option<std::path::PathBuf>,
}

impl Visualizer {
    pub fn new(bars: usize) -> Self {
        let bars = bars.max(1).min(32);
        let levels = vec![0.0; bars];
        let shared = std::sync::Arc::new(std::sync::Mutex::new(levels.clone()));
        Self {
            bars,
            levels,
            last_tick: Instant::now(),
            phase: 0.0,
            fps: 30,
            cava_child: None,
            cava_reader: None,
            shared_levels: shared,
            stop_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_cava_frame: std::sync::Arc::new(std::sync::Mutex::new(None)),
            ffmpeg_child: None,
            fifo_path: None,
        }
    }

    pub fn bars(&self) -> usize { self.bars }

    pub fn frame_budget(&self) -> Duration {
        Duration::from_millis((1000 / self.fps.max(1).min(60)) as u64)
    }

    pub fn fifo_path(&self) -> Option<&std::path::Path> {
        self.fifo_path.as_deref()
    }

    /// True only when cava is live and recently sent non-silent data.
    pub fn is_audio_attached(&self) -> bool {
        self.cava_child.is_some()
            && self.last_cava_frame
                .lock()
                .ok()
                .and_then(|g| *g)
                .map(|t| t.elapsed() < Duration::from_millis(600))
                .unwrap_or(false)
    }

    // ── FIFO + cava ──────────────────────────────────────────────────────────

    fn log(msg: &str) {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true).append(true)
            .open("/tmp/terminaldrome_vis.log")
        {
            let _ = writeln!(f, "{}", msg);
        }
    }

    pub fn try_attach_cava(&mut self) -> Result<()> {
        if self.cava_child.is_some() { return Ok(()); }
        Self::log("try_attach_cava: starting");

        // Create FIFO.
        // On macOS, std::env::temp_dir() returns /tmp which is a symlink and
        // mkfifo may fail on it. Use $TMPDIR explicitly (e.g. /var/folders/...).
        let tmp_base = std::env::var("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));
        let fifo = tmp_base.join("terminaldrome_cava.fifo");
        let _ = std::fs::remove_file(&fifo);
        let ok = Command::new("mkfifo").arg(&fifo).status()
            .map(|s| s.success()).unwrap_or(false);
        if !ok {
            Self::log(&format!("mkfifo FAILED for {:?}", fifo));
            return Ok(());
        }
        Self::log(&format!("mkfifo OK: {:?}", fifo));

        self.fifo_path = Some(fifo.clone());

        // Write cava config to temp file
        let bars      = self.bars;
        let ascii_max = 100u32;
        let fps       = self.fps.max(1).min(60);
        let cfg = format!(
            "[general]\nframerate={fps}\nbars={bars}\nlower_cutoff_freq=50\nhigher_cutoff_freq=10000\n\n\
             [input]\nmethod=fifo\nsource={fifo}\n\n\
             [output]\nmethod=raw\nraw_target=/dev/stdout\ndata_format=ascii\nascii_max_range={ascii_max}\ndelim=59\n",
            fps=fps, bars=bars, fifo=fifo.display(), ascii_max=ascii_max
        );

        let mut cfg_file = match tempfile::NamedTempFile::new() {
            Ok(f) => f, Err(_) => return Ok(()),
        };
        use std::io::Write as _;
        if cfg_file.write_all(cfg.as_bytes()).is_err() { return Ok(()); }
        let cfg_tmp  = cfg_file.into_temp_path();
        let cfg_path = cfg_tmp.to_path_buf();
        let _ = cfg_tmp.keep();

        // Spawn cava
        let child = Command::new("cava")
            .args(["-p", cfg_path.to_str().unwrap_or("")])
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else {
            Self::log("cava spawn FAILED (not installed?)");
            let _ = std::fs::remove_file(&fifo);
            self.fifo_path = None;
            return Ok(());
        };
        Self::log("cava spawned OK");
        let stdout = match child.stdout.take() {
            Some(o) => o,
            None => { let _ = child.kill(); let _ = std::fs::remove_file(&fifo); self.fifo_path = None; return Ok(()); }
        };

        // Reader thread
        let shared     = self.shared_levels.clone();
        let stop       = self.stop_flag.clone();
        let last_frame = self.last_cava_frame.clone();
        self.stop_flag.store(false, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut g) = self.last_cava_frame.lock() { *g = None; }

        let cfg_path_t = cfg_path.clone();
        let handle = std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                if stop.load(std::sync::atomic::Ordering::Relaxed) { break; }
                let parts: Vec<&str> = line.split(';').collect();
                if parts.iter().all(|p| matches!(p.trim(), "0" | "")) { continue; }
                let mut out = vec![0.0f32; bars];
                for (i, p) in parts.iter().take(bars).enumerate() {
                    if let Ok(v) = p.trim().parse::<f32>() {
                        out[i] = (v / ascii_max as f32).clamp(0.0, 1.0);
                    }
                }
                if let Ok(mut g) = shared.lock() { *g = out; }
                if let Ok(mut t) = last_frame.lock() { *t = Some(Instant::now()); }
            }
            let _ = std::fs::remove_file(&cfg_path_t);
        });

        self.cava_reader = Some(handle);
        self.cava_child  = Some(child);
        Ok(())
    }

    // ── ffmpeg feeder ────────────────────────────────────────────────────────

    /// Spawn a lightweight ffmpeg process that:
    ///   - reads the given stream URL (same URL mpv is already playing)
    ///   - decodes audio to raw s16le, 44100 Hz, stereo
    ///   - writes it into the FIFO that cava is reading
    ///
    /// mpv keeps running unchanged — zero audio interruption.
    /// On macOS this sidesteps the CoreAudio loopback requirement entirely.
    pub fn start_ffmpeg_feeder(&mut self, stream_url: &str, fifo: &std::path::Path) {
        // Kill previous feeder if any
        if let Some(mut c) = self.ffmpeg_child.take() { let _ = c.kill(); }

        // ffmpeg: read stream, output raw PCM s16le into FIFO
        // -vn           : skip video
        // -ar 44100     : sample rate cava expects
        // -ac 2         : stereo
        // -f s16le      : raw PCM format cava's fifo input reads
        // -               would be stdout, but we write directly to the FIFO path
        let child = Command::new("ffmpeg")
            .args([
                "-loglevel", "quiet",
                "-i",        stream_url,
                "-vn",
                "-ar",       "44100",
                "-ac",       "2",
                "-f",        "s16le",
                fifo.to_str().unwrap_or(""),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match child {
            Ok(c) => {
                Self::log(&format!("ffmpeg feeder spawned OK for {}", stream_url));
                self.ffmpeg_child = Some(c);
            }
            Err(e) => {
                Self::log(&format!("ffmpeg spawn FAILED: {}", e));
            }
        }
        // If ffmpeg is not installed → cava stays in demo mode (no crash)
    }

    pub fn stop_ffmpeg_feeder(&mut self) {
        if let Some(mut c) = self.ffmpeg_child.take() { let _ = c.kill(); }
    }

    // ── detach ───────────────────────────────────────────────────────────────

    pub fn detach_audio(&mut self) {
        self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        self.stop_ffmpeg_feeder();
        if let Some(mut c) = self.cava_child.take() { let _ = c.kill(); }
        if let Some(h) = self.cava_reader.take() { let _ = h.join(); }
        if let Some(ref p) = self.fifo_path.take() { let _ = std::fs::remove_file(p); }
        if let Ok(mut g) = self.last_cava_frame.lock() { *g = None; }
    }

    // ── tick & render ────────────────────────────────────────────────────────

    pub fn tick(&mut self) {
        if self.is_audio_attached() {
            if let Ok(guard) = self.shared_levels.lock() {
                for i in 0..self.bars {
                    let t = guard.get(i).copied().unwrap_or(0.0);
                    self.levels[i] = self.levels[i] * 0.75 + t * 0.25;
                }
            }
            return;
        }
        // Demo animation
        let dt = Instant::now().duration_since(self.last_tick);
        self.last_tick = Instant::now();
        let secs = dt.as_secs_f32().min(0.1);
        self.phase = (self.phase + secs * 2.2) % std::f32::consts::TAU;
        for i in 0..self.bars {
            let x = self.phase + i as f32 * 0.55;
            let v = (x.sin() * 0.6 + (x * 0.5).cos() * 0.35 + 0.75).clamp(0.0, 1.0);
            self.levels[i] = self.levels[i] * 0.82 + v * 0.18;
        }
    }

    pub fn render(&self, f: &mut Frame<'_>, area: Rect) {
        f.render_widget(Block::default().borders(Borders::NONE), area);
        let w = area.width as i32;
        let h = area.height as i32;
        if w <= 0 || h <= 0 { return; }

        let bars  = self.bars as i32;
        let gap   = 2i32;
        let bar_w = ((w - (bars - 1) * gap) / bars).max(1);
        let left  = ((w - (bars * bar_w + (bars - 1) * gap)) / 2).max(0);

        for i in 0..bars {
            let lvl   = self.levels.get(i as usize).copied().unwrap_or(0.0);
            let bar_h = ((lvl * h as f32) as i32).clamp(0, h);
            let x0    = area.x as i32 + left + i * (bar_w + gap);
            let x1    = (x0 + bar_w).min(area.x as i32 + w);

            for y in 0..h {
                let yy     = area.y as i32 + (h - 1 - y);
                let filled = y < bar_h;
                let frac   = y as f32 / h.max(1) as f32;
                let color  = if frac > 0.66 { Color::LightCyan }
                             else if frac > 0.33 { Color::Cyan }
                             else { Color::Blue };
                for xx in x0..x1 {
                    if xx >= area.x as i32 && xx < area.x as i32 + w {
                        let cell = f.buffer_mut().get_mut(xx as u16, yy as u16);
                        if filled { cell.set_char('█').set_style(Style::default().fg(color)); }
                        else      { cell.set_char(' ').set_style(Style::default()); }
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    pub async fn attach_to_mpv(&mut self, _mpv_socket_path: &str) -> Result<()> { Ok(()) }
}

impl Drop for Visualizer {
    fn drop(&mut self) { self.detach_audio(); }
}
