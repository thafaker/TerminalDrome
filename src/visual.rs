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
///   mpv    ──plays──▶  speakers           (untouched, no restart)
///   ffmpeg ──reads same URL──▶  s16le PCM ──▶  named FIFO  (metered to real-time)
///   cava   ──reads FIFO──▶  bar levels    ──▶  this widget
///
/// Fallback: if cava or ffmpeg is missing, a demo animation runs.
#[derive(Debug)]
pub struct Visualizer {
    bars: usize,
    levels: Vec<f32>,
    last_tick: Instant,
    phase: f32,
    pub fps: u32,

    cava_child: Option<Child>,
    cava_reader: Option<std::thread::JoinHandle<()>>,
    shared_levels: std::sync::Arc<std::sync::Mutex<Vec<f32>>>,
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    last_cava_frame: std::sync::Arc<std::sync::Mutex<Option<Instant>>>,

    ffmpeg_child: Option<Child>, // kept for potential future use
    ffmpeg_stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
            ffmpeg_stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
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

    /// True only when cava is running and has sent data within the last 600 ms.
    pub fn is_audio_attached(&self) -> bool {
        self.cava_child.is_some()
            && self.last_cava_frame
                .lock()
                .ok()
                .and_then(|g| *g)
                .map(|t| t.elapsed() < Duration::from_millis(600))
                .unwrap_or(false)
    }

    // O_NONBLOCK without the libc crate
    #[cfg(target_os = "macos")]
    fn o_nonblock() -> i32 { 4 }
    #[cfg(not(target_os = "macos"))]
    fn o_nonblock() -> i32 { 2048 }

    // ── cava ─────────────────────────────────────────────────────────────────

    pub fn try_attach_cava(&mut self) -> Result<()> {
        if self.cava_child.is_some() { return Ok(()); }

        // On macOS $TMPDIR is /var/folders/... which always works.
        // std::env::temp_dir() returns /tmp (a symlink) which may behave oddly.
        let tmp_base = std::env::var("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));
        let fifo = tmp_base.join("terminaldrome_cava.fifo");
        let _ = std::fs::remove_file(&fifo);

        let ok = Command::new("mkfifo").arg(&fifo).status()
            .map(|s| s.success()).unwrap_or(false);
        if !ok { return Ok(()); }

        self.fifo_path = Some(fifo.clone());

        let bars      = self.bars;
        let ascii_max = 100u32;
        let fps       = self.fps.max(1).min(60);
        let cfg = format!(
            "[general]\nframerate={fps}\nbars={bars}\n\
             lower_cutoff_freq=50\nhigher_cutoff_freq=10000\n\n\
             [input]\nmethod=fifo\nsource={fifo}\n\n\
             [output]\nmethod=raw\nraw_target=/dev/stdout\n\
             data_format=ascii\nascii_max_range={ascii_max}\ndelim=59\n",
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

        let child = Command::new("cava")
            .args(["-p", cfg_path.to_str().unwrap_or("")])
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else {
            let _ = std::fs::remove_file(&fifo);
            self.fifo_path = None;
            return Ok(());
        };
        let stdout = match child.stdout.take() {
            Some(o) => o,
            None => {
                let _ = child.kill();
                let _ = std::fs::remove_file(&fifo);
                self.fifo_path = None;
                return Ok(());
            }
        };

        let shared     = self.shared_levels.clone();
        let stop       = self.stop_flag.clone();
        let last_frame = self.last_cava_frame.clone();
        self.stop_flag.store(false, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut g) = self.last_cava_frame.lock() { *g = None; }

        let handle = std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                if stop.load(std::sync::atomic::Ordering::Relaxed) { break; }
                let parts: Vec<&str> = line.split(';').collect();
                // Mark cava as live on every frame, even silent ones
                if let Ok(mut t) = last_frame.lock() { *t = Some(Instant::now()); }
                let all_zero = parts.iter().all(|p| matches!(p.trim(), "0" | ""));
                if all_zero { continue; }
                let mut out = vec![0.0f32; bars];
                for (i, p) in parts.iter().take(bars).enumerate() {
                    if let Ok(v) = p.trim().parse::<f32>() {
                        out[i] = (v / ascii_max as f32).clamp(0.0, 1.0);
                    }
                }
                if let Ok(mut g) = shared.lock() { *g = out; }
            }
            let _ = std::fs::remove_file(&cfg_path);
        });

        self.cava_reader = Some(handle);
        self.cava_child  = Some(child);
        Ok(())
    }

    // ── ffmpeg feeder ─────────────────────────────────────────────────────────

    /// Start a fire-and-forget thread that:
    /// 1. Waits until cava has opened the FIFO for reading (up to 3 s).
    /// 2. Spawns ffmpeg to decode the stream URL to raw s16le PCM on stdout.
    /// 3. Forwards that PCM into the FIFO at real-time speed (176 400 B/s)
    ///    so cava receives a continuous stream instead of a burst then silence.
    pub fn start_ffmpeg_feeder(&mut self, stream_url: &str, fifo: &std::path::Path, start_secs: u64) {
        if let Some(mut c) = self.ffmpeg_child.take() { let _ = c.kill(); }

        let fifo_path = fifo.to_path_buf();
        let url       = stream_url.to_string();
        // Use a dedicated stop flag for ffmpeg so we can kill just the feeder
        // (e.g. when playback stops) without tearing down cava.
        self.ffmpeg_stop.store(false, std::sync::atomic::Ordering::Relaxed);
        let stop      = self.ffmpeg_stop.clone();
        let cava_stop = self.stop_flag.clone();

        let _handle = std::thread::spawn(move || {
            // Wait for cava to open the FIFO (non-blocking probe)
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if stop.load(std::sync::atomic::Ordering::Relaxed) { return; }
                if Instant::now() > deadline { break; }
                use std::os::unix::fs::OpenOptionsExt;
                if std::fs::OpenOptions::new()
                    .write(true)
                    .custom_flags(Self::o_nonblock())
                    .open(&fifo_path)
                    .is_ok()
                {
                    break; // cava is ready
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if stop.load(std::sync::atomic::Ordering::Relaxed) { return; }
            if cava_stop.load(std::sync::atomic::Ordering::Relaxed) { return; }

            // Seek to current playback position so ffmpeg is in sync with mpv.
            // We add a small back-offset to account for cava's startup latency.
            let seek_secs = start_secs.saturating_sub(1);
            let seek_str  = seek_secs.to_string();

            let child = Command::new("ffmpeg")
                .args([
                    "-loglevel", "quiet",
                    "-ss",       &seek_str,  // seek to current position
                    "-i",        &url,
                    "-vn",
                    "-ar",       "44100",
                    "-ac",       "2",
                    "-f",        "s16le",
                    "pipe:1",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();

            if let Ok(mut c) = child {
                // 44100 Hz × 2 ch × 2 bytes = 176 400 B/s
                const RATE: u64 = 176_400;
                const CHUNK: usize = 4096;
                let chunk_dur = Duration::from_secs_f64(CHUNK as f64 / RATE as f64);

                if let Some(mut ffmpeg_out) = c.stdout.take() {
                    use std::io::{Read, Write};
                    if let Ok(mut fifo_writer) = std::fs::OpenOptions::new()
                        .write(true).open(&fifo_path)
                    {
                        let mut buf = vec![0u8; CHUNK];
                        loop {
                            if stop.load(std::sync::atomic::Ordering::Relaxed) { break; }
                            if cava_stop.load(std::sync::atomic::Ordering::Relaxed) { break; }
                            let t0 = Instant::now();
                            match ffmpeg_out.read(&mut buf) {
                                Ok(0) => break,
                                Ok(n) => { if fifo_writer.write_all(&buf[..n]).is_err() { break; } }
                                Err(_) => break,
                            }
                            let elapsed = t0.elapsed();
                            if elapsed < chunk_dur {
                                std::thread::sleep(chunk_dur - elapsed);
                            }
                        }
                    }
                }
                let _ = c.wait();
            }
        });
    }

    pub fn stop_ffmpeg_feeder(&mut self) {
        // Signal the feeder thread to stop (it checks this flag in the copy loop)
        self.ffmpeg_stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(mut c) = self.ffmpeg_child.take() { let _ = c.kill(); }
    }

    // ── detach ────────────────────────────────────────────────────────────────

    pub fn detach_audio(&mut self) {
        self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        self.stop_ffmpeg_feeder();
        if let Some(mut c) = self.cava_child.take() { let _ = c.kill(); }
        if let Some(h) = self.cava_reader.take() { let _ = h.join(); }
        if let Some(ref p) = self.fifo_path.take() { let _ = std::fs::remove_file(p); }
        if let Ok(mut g) = self.last_cava_frame.lock() { *g = None; }
    }

    // ── tick & render ─────────────────────────────────────────────────────────

    pub fn tick(&mut self) {
        if self.cava_child.is_some() {
            if let Ok(guard) = self.shared_levels.lock() {
                for i in 0..self.bars {
                    let target = guard.get(i).copied().unwrap_or(0.0);
                    let prev   = self.levels[i];
                    // Attack fast, decay slower — gives punchy beat response
                    self.levels[i] = if target > prev {
                        prev * 0.3 + target * 0.7   // fast attack
                    } else {
                        prev * 0.6 + target * 0.4   // moderate decay
                    };
                }
            }
            // Decay to zero when no fresh data (silence / song stopped)
            if !self.is_audio_attached() {
                for lvl in self.levels.iter_mut() {
                    *lvl *= 0.8;
                }
            }
            return;
        }

        // Demo animation (no cava)
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
        let w = area.width  as i32;
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
