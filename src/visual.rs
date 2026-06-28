use anyhow::Result;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
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
///   mpv    ──plays──▶  speakers
///   ffmpeg ──reads same URL──▶  s16le PCM ──▶  named FIFO  (metered, real-time)
///   cava   ──reads FIFO──▶  bar levels ──▶  this widget
///
/// A watchdog thread monitors both cava and ffmpeg and restarts them when they
/// die (e.g. after a track change or EOF). This keeps the visualizer alive
/// across the entire playback session.
///
/// Fallback: demo animation when cava/ffmpeg are unavailable.
#[derive(Debug)]
pub struct Visualizer {
    bars:   usize,
    levels: Vec<f32>,
    last_tick: Instant,
    phase:  f32,
    pub fps: u32,

    // Shared state between the main thread and the watchdog/reader threads
    shared_levels:   Arc<Mutex<Vec<f32>>>,
    last_cava_frame: Arc<Mutex<Option<Instant>>>,

    // Flags
    stop_flag:    Arc<AtomicBool>,   // set → shut everything down
    ffmpeg_stop:  Arc<AtomicBool>,   // set → stop current ffmpeg feeder only

    // Current stream info, written by main thread, read by watchdog
    current_url:      Arc<Mutex<String>>,
    current_seek_sec: Arc<AtomicU64>,

    // FIFO path
    fifo_path: Option<std::path::PathBuf>,

    // Watchdog thread handle (keeps cava + ffmpeg alive)
    watchdog_handle: Option<std::thread::JoinHandle<()>>,

    // We no longer store cava_child / ffmpeg_child here — the watchdog owns them
    cava_running: Arc<AtomicBool>,
}

impl Visualizer {
    pub fn new(bars: usize) -> Self {
        let bars   = bars.max(1).min(32);
        let levels = vec![0.0; bars];
        let shared = Arc::new(Mutex::new(levels.clone()));
        Self {
            bars,
            levels,
            last_tick: Instant::now(),
            phase:  0.0,
            fps:    30,
            shared_levels:    shared,
            last_cava_frame:  Arc::new(Mutex::new(None)),
            stop_flag:        Arc::new(AtomicBool::new(false)),
            ffmpeg_stop:      Arc::new(AtomicBool::new(false)),
            current_url:      Arc::new(Mutex::new(String::new())),
            current_seek_sec: Arc::new(AtomicU64::new(0)),
            fifo_path:        None,
            watchdog_handle:  None,
            cava_running:     Arc::new(AtomicBool::new(false)),
        }
    }

    #[allow(dead_code)]
    pub fn bars(&self) -> usize { self.bars }

    #[allow(dead_code)]
    pub fn frame_budget(&self) -> Duration {
        Duration::from_millis((1000 / self.fps.max(1).min(60)) as u64)
    }

    pub fn fifo_path(&self) -> Option<&std::path::Path> {
        self.fifo_path.as_deref()
    }

    pub fn is_audio_attached(&self) -> bool {
        self.cava_running.load(Ordering::Relaxed)
            && self.last_cava_frame
                .lock().ok()
                .and_then(|g| *g)
                .map(|t| t.elapsed() < Duration::from_millis(800))
                .unwrap_or(false)
    }

    // O_NONBLOCK without pulling in the libc crate
    #[cfg(target_os = "macos")]
    fn o_nonblock() -> i32 { 4 }
    #[cfg(not(target_os = "macos"))]
    fn o_nonblock() -> i32 { 2048 }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Start cava + ffmpeg watchdog. Call once when entering visualizer mode.
    pub fn try_attach_cava(&mut self) -> Result<()> {
        if self.watchdog_handle.is_some() { return Ok(()); }

        // Create FIFO
        let tmp_base = std::env::var("TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));
        let fifo = tmp_base.join("terminaldrome_cava.fifo");
        let _ = std::fs::remove_file(&fifo);
        let ok = Command::new("mkfifo").arg(&fifo).status()
            .map(|s| s.success()).unwrap_or(false);
        if !ok { return Ok(()); }
        self.fifo_path = Some(fifo.clone());

        // Write cava config
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

        self.stop_flag.store(false, Ordering::Relaxed);
        self.ffmpeg_stop.store(false, Ordering::Relaxed);
        self.cava_running.store(false, Ordering::Relaxed);
        if let Ok(mut g) = self.last_cava_frame.lock() { *g = None; }

        // Clone Arcs for the watchdog thread
        let stop         = self.stop_flag.clone();
        let ffmpeg_stop  = self.ffmpeg_stop.clone();
        let shared       = self.shared_levels.clone();
        let last_frame   = self.last_cava_frame.clone();
        let cur_url      = self.current_url.clone();
        let cur_seek     = self.current_seek_sec.clone();
        let cava_running = self.cava_running.clone();
        let fifo_path    = fifo.clone();
        let cfg_path_w   = cfg_path.clone();

        let handle = std::thread::spawn(move || {
            Self::watchdog(
                stop, ffmpeg_stop, shared, last_frame,
                cur_url, cur_seek, cava_running,
                fifo_path, cfg_path_w,
                bars, ascii_max,
            );
        });

        self.watchdog_handle = Some(handle);
        Ok(())
    }

    /// Tell the feeder about a new track (or seek position). Safe to call at any time.
    pub fn start_ffmpeg_feeder(&mut self, stream_url: &str, _fifo: &std::path::Path, start_secs: u64) {
        // Update shared state — the watchdog will pick this up and restart ffmpeg
        if let Ok(mut u) = self.current_url.lock() { *u = stream_url.to_string(); }
        self.current_seek_sec.store(start_secs, Ordering::Relaxed);
        // Signal the current ffmpeg to stop so the watchdog restarts it with new URL/seek
        self.ffmpeg_stop.store(true, Ordering::Relaxed);
    }

    /// Stop only ffmpeg (e.g. playback paused / stopped). cava keeps running.
    pub fn stop_ffmpeg_feeder(&mut self) {
        if let Ok(mut u) = self.current_url.lock() { u.clear(); }
        self.ffmpeg_stop.store(true, Ordering::Relaxed);
    }

    /// Full teardown — call when leaving visualizer mode.
    pub fn detach_audio(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        self.ffmpeg_stop.store(true, Ordering::Relaxed);
        self.cava_running.store(false, Ordering::Relaxed);
        if let Ok(mut u) = self.current_url.lock() { u.clear(); }
        if let Some(h) = self.watchdog_handle.take() { let _ = h.join(); }
        if let Some(ref p) = self.fifo_path.take() { let _ = std::fs::remove_file(p); }
        if let Ok(mut g) = self.last_cava_frame.lock() { *g = None; }
    }

    // ── Watchdog ──────────────────────────────────────────────────────────────

    /// Owns cava and ffmpeg processes. Restarts them whenever they die.
    /// Runs until `stop` is set.
    fn watchdog(
        stop:         Arc<AtomicBool>,
        ffmpeg_stop:  Arc<AtomicBool>,
        shared:       Arc<Mutex<Vec<f32>>>,
        last_frame:   Arc<Mutex<Option<Instant>>>,
        cur_url:      Arc<Mutex<String>>,
        cur_seek:     Arc<AtomicU64>,
        cava_running: Arc<AtomicBool>,
        fifo_path:    std::path::PathBuf,
        cfg_path:     std::path::PathBuf,
        bars:         usize,
        ascii_max:    u32,
    ) {
        while !stop.load(Ordering::Relaxed) {
            // ── Start cava ────────────────────────────────────────────────────
            let cava_args = ["-p", cfg_path.to_str().unwrap_or("")];
            let mut cava = match Command::new("cava")
                .args(cava_args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(c)  => c,
                Err(_) => {
                    // cava not installed — sleep and stay in demo mode
                    std::thread::sleep(Duration::from_secs(5));
                    continue;
                }
            };

            let stdout = match cava.stdout.take() {
                Some(o) => o,
                None    => { let _ = cava.kill(); std::thread::sleep(Duration::from_secs(1)); continue; }
            };

            cava_running.store(true, Ordering::Relaxed);

            // Start ffmpeg for current URL
            ffmpeg_stop.store(false, Ordering::Relaxed);
            let mut ffmpeg_child: Option<Child> = None;
            let mut ffmpeg_stdout_reader: Option<std::thread::JoinHandle<()>> = None;

            // Reset seek to 0 when cava (and ffmpeg) restarts — we don't know
            // where in the song we are, so start from the beginning of the stream.
            // This is better than seeking to a stale position and getting EOF.
            cur_seek.store(0, Ordering::Relaxed);

            Self::maybe_start_ffmpeg(
                &cur_url, &cur_seek, &fifo_path,
                &stop, &ffmpeg_stop,
                &mut ffmpeg_child,
                &mut ffmpeg_stdout_reader,
            );

            // ── cava reader loop ──────────────────────────────────────────────
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                if stop.load(Ordering::Relaxed) { break; }

                // Update liveness timestamp
                if let Ok(mut t) = last_frame.lock() { *t = Some(Instant::now()); }

                // Parse bar values
                let parts: Vec<&str> = line.split(';').collect();
                let all_zero = parts.iter().all(|p| matches!(p.trim(), "0" | ""));
                if !all_zero {
                    let mut out = vec![0.0f32; bars];
                    for (i, p) in parts.iter().take(bars).enumerate() {
                        if let Ok(v) = p.trim().parse::<f32>() {
                            out[i] = (v / ascii_max as f32).clamp(0.0, 1.0);
                        }
                    }
                    if let Ok(mut g) = shared.lock() { *g = out; }
                }

                // Check if ffmpeg needs to be restarted (new URL or stop signal cleared)
                if ffmpeg_stop.load(Ordering::Relaxed) {
                    // Kill current ffmpeg
                    if let Some(mut c) = ffmpeg_child.take() { let _ = c.kill(); }
                    if let Some(h) = ffmpeg_stdout_reader.take() { let _ = h.join(); }

                    // Wait a moment for the FIFO to drain
                    std::thread::sleep(Duration::from_millis(100));
                    ffmpeg_stop.store(false, Ordering::Relaxed);

                    // Start new ffmpeg if we have a URL
                    Self::maybe_start_ffmpeg(
                        &cur_url, &cur_seek, &fifo_path,
                        &stop, &ffmpeg_stop,
                        &mut ffmpeg_child,
                        &mut ffmpeg_stdout_reader,
                    );
                }
            }

            // cava exited — kill ffmpeg immediately so it doesn't get
            // SIGPIPE writing to a FIFO with no reader, then recreate the FIFO.
            cava_running.store(false, Ordering::Relaxed);
            ffmpeg_stop.store(true, Ordering::Relaxed);
            if let Some(mut c) = ffmpeg_child.take() { let _ = c.kill(); }
            let _ = cava.wait();

            if stop.load(Ordering::Relaxed) { break; }

            // Recreate the FIFO (cava will have closed it)
            let _ = std::fs::remove_file(&fifo_path);
            let _ = Command::new("mkfifo").arg(&fifo_path).status();

            // Brief pause before restarting cava
            std::thread::sleep(Duration::from_millis(300));
            ffmpeg_stop.store(false, Ordering::Relaxed);
        }

        let _ = std::fs::remove_file(&cfg_path);
    }

    /// Spawn an ffmpeg feeder for the current URL, if one is set.
    fn maybe_start_ffmpeg(
        cur_url:   &Arc<Mutex<String>>,
        cur_seek:  &Arc<AtomicU64>,
        fifo_path: &std::path::PathBuf,
        stop:      &Arc<AtomicBool>,
        ffmpeg_stop: &Arc<AtomicBool>,
        child_out: &mut Option<Child>,
        _reader_out: &mut Option<std::thread::JoinHandle<()>>,
    ) {
        let url = match cur_url.lock() {
            Ok(u) if !u.is_empty() => u.clone(),
            _ => return, // no URL — nothing to play
        };
        let seek_sec = cur_seek.load(Ordering::Relaxed).saturating_sub(1);

        // Wait for cava to open FIFO for reading (up to 3 s)
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if stop.load(Ordering::Relaxed) || ffmpeg_stop.load(Ordering::Relaxed) { return; }
            if Instant::now() > deadline { break; }
            use std::os::unix::fs::OpenOptionsExt;
            if std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(Self::o_nonblock())
                .open(fifo_path)
                .is_ok()
            { break; }
            std::thread::sleep(Duration::from_millis(50));
        }

        if stop.load(Ordering::Relaxed) || ffmpeg_stop.load(Ordering::Relaxed) { return; }

        let seek_str = seek_sec.to_string();
        let child = Command::new("ffmpeg")
            .args([
                "-loglevel", "quiet",
                "-ss",       &seek_str,
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
            if let Some(mut ffmpeg_out) = c.stdout.take() {
                // Spawn a thread to meter PCM into the FIFO at real-time speed
                let fifo_p      = fifo_path.clone();
                let stop_c      = stop.clone();
                let ffstop_c    = ffmpeg_stop.clone();
                std::thread::spawn(move || {
                    const RATE:  u64   = 176_400; // 44100 * 2 ch * 2 bytes
                    const CHUNK: usize = 4096;
                    let chunk_dur = Duration::from_secs_f64(CHUNK as f64 / RATE as f64);
                    use std::io::{Read, Write};
                    if let Ok(mut fifo_writer) = std::fs::OpenOptions::new()
                        .write(true).open(&fifo_p)
                    {
                        let mut buf = vec![0u8; CHUNK];
                        loop {
                            if stop_c.load(Ordering::Relaxed)   { break; }
                            if ffstop_c.load(Ordering::Relaxed) { break; }
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
                });
            }
            *child_out = Some(c);
        }
    }

    // ── tick & render ─────────────────────────────────────────────────────────

    pub fn tick(&mut self) {
        if self.cava_running.load(Ordering::Relaxed) {
            if let Ok(guard) = self.shared_levels.lock() {
                for i in 0..self.bars {
                    let target = guard.get(i).copied().unwrap_or(0.0);
                    let prev   = self.levels[i];
                    self.levels[i] = if target > prev {
                        prev * 0.3 + target * 0.7  // fast attack
                    } else {
                        prev * 0.6 + target * 0.4  // moderate decay
                    };
                }
            }
            // Decay toward zero if no fresh cava data
            if !self.is_audio_attached() {
                for lvl in self.levels.iter_mut() { *lvl *= 0.8; }
            }
            return;
        }

        // Demo animation (cava not available)
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
