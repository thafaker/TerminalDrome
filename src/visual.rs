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

/// Fullscreen Audio Visualizer (8 wide bars) for an 80x25-friendly terminal.
///
/// Pipeline:
/// - Preferred: spawn `cava` and read 8 bar levels (0..1) from stdout
/// - Fallback: deterministic demo animation (so the UI still works without cava)
///
/// macOS note: cava via Homebrew behaves differently from the Linux package.
/// We detect if cava is actually delivering data via a liveness timestamp,
/// and fall back to demo mode if it goes silent for > 500 ms.
#[derive(Debug)]
pub struct Visualizer {
    bars: usize,

    /// 0.0..1.0
    levels: Vec<f32>,

    last_tick: Instant,
    phase: f32,

    // tuning
    pub fps: u32,

    // audio backend
    cava_child: Option<Child>,
    cava_reader: Option<std::thread::JoinHandle<()>>,
    shared_levels: std::sync::Arc<std::sync::Mutex<Vec<f32>>>,
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,

    /// Timestamp of the last frame actually received from cava.
    /// If this is too old we treat cava as dead and use demo mode.
    last_cava_frame: std::sync::Arc<std::sync::Mutex<Option<Instant>>>,
}

impl Visualizer {
    pub fn new(bars: usize) -> Self {
        let bars = bars.max(1).min(32);
        let levels = vec![0.0; bars];
        let shared_levels = std::sync::Arc::new(std::sync::Mutex::new(levels.clone()));
        Self {
            bars,
            levels,
            last_tick: Instant::now(),
            phase: 0.0,
            fps: 30,
            cava_child: None,
            cava_reader: None,
            shared_levels,
            stop_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_cava_frame: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn bars(&self) -> usize {
        self.bars
    }

    pub fn frame_budget(&self) -> Duration {
        let fps = self.fps.max(1).min(60);
        Duration::from_millis((1000 / fps) as u64)
    }

    /// Returns true only if cava is running AND has delivered a frame recently.
    pub fn is_audio_attached(&self) -> bool {
        if self.cava_child.is_none() {
            return false;
        }
        // If cava hasn't sent us any data within 500 ms, treat as dead/silent.
        if let Ok(guard) = self.last_cava_frame.lock() {
            match *guard {
                Some(t) => t.elapsed() < Duration::from_millis(500),
                None => false, // spawned but never sent a frame yet – use demo
            }
        } else {
            false
        }
    }

    /// Try to start cava.
    ///
    /// Best-effort: if cava is missing or the pipe fails, we silently keep demo mode.
    /// Works on both Linux (ALSA/PulseAudio/Pipewire) and macOS (Homebrew cava with portaudio).
    pub fn try_attach_cava(&mut self) -> Result<()> {
        if self.cava_child.is_some() {
            return Ok(());
        }

        let bars = self.bars;
        let ascii_max = 100u32;

        // We write the config to a temp file instead of piping via /dev/stdin,
        // because macOS cava does not reliably read config from a stdin pipe.
        let config_content = format!(
            r#"[general]
framerate={fps}
bars={bars}

[output]
method=raw
raw_target=/dev/stdout
data_format=ascii
ascii_max_range={ascii_max}
delim=59
"#,
            fps = self.fps.max(1).min(60),
            bars = bars,
            ascii_max = ascii_max,
        );

        // Write config to a temp file
        let mut config_file = match tempfile::NamedTempFile::new() {
            Ok(f) => f,
            Err(_) => return Ok(()), // can't create temp file, stay in demo mode
        };
        use std::io::Write as _;
        if config_file.write_all(config_content.as_bytes()).is_err() {
            return Ok(());
        }
        // Keep the file alive for the duration of cava's lifetime by leaking
        // the NamedTempFile into a path we can pass. We persist it manually.
        let config_path = config_file.into_temp_path();
        let config_path_str = config_path.to_path_buf();
        // Persist so it isn't deleted when config_path drops
        let _ = config_path.keep();

        let child = Command::new("cava")
            .arg("-p")
            .arg(&config_path_str)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();

        let Ok(mut child) = child else {
            // cava not found or failed to start
            let _ = std::fs::remove_file(&config_path_str);
            return Ok(());
        };

        let stdout = match child.stdout.take() {
            Some(o) => o,
            None => {
                let _ = child.kill();
                let _ = std::fs::remove_file(&config_path_str);
                return Ok(());
            }
        };

        let shared = self.shared_levels.clone();
        let stop = self.stop_flag.clone();
        let last_frame = self.last_cava_frame.clone();

        self.stop_flag
            .store(false, std::sync::atomic::Ordering::Relaxed);

        // Reset liveness timestamp
        if let Ok(mut g) = self.last_cava_frame.lock() {
            *g = None;
        }

        let config_path_for_thread = config_path_str.clone();
        let handle = std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                // line example: "12;0;55;33;80;20;44;10"
                let parts: Vec<&str> = line.split(';').collect();
                // Ignore empty or obviously wrong frames
                if parts.iter().all(|p| p.trim() == "0" || p.trim().is_empty()) {
                    continue;
                }
                let mut out = vec![0.0f32; bars];
                for (i, part) in parts.iter().take(bars).enumerate() {
                    if let Ok(v) = part.trim().parse::<f32>() {
                        out[i] = (v / ascii_max as f32).clamp(0.0, 1.0);
                    }
                }
                if let Ok(mut guard) = shared.lock() {
                    *guard = out;
                }
                // Update liveness timestamp
                if let Ok(mut t) = last_frame.lock() {
                    *t = Some(Instant::now());
                }
            }
            // Cleanup temp config file when thread exits
            let _ = std::fs::remove_file(&config_path_for_thread);
        });

        self.cava_reader = Some(handle);
        self.cava_child = Some(child);
        Ok(())
    }

    pub fn detach_audio(&mut self) {
        self.stop_flag
            .store(true, std::sync::atomic::Ordering::Relaxed);

        if let Some(mut child) = self.cava_child.take() {
            let _ = child.kill();
        }

        if let Some(handle) = self.cava_reader.take() {
            let _ = handle.join();
        }

        if let Ok(mut g) = self.last_cava_frame.lock() {
            *g = None;
        }
    }

    pub fn tick(&mut self) {
        // Use real audio only if cava is alive AND sending frames
        if self.is_audio_attached() {
            if let Ok(guard) = self.shared_levels.lock() {
                for i in 0..self.bars {
                    let target = guard.get(i).copied().unwrap_or(0.0);
                    let prev = self.levels[i];
                    self.levels[i] = prev * 0.75 + target * 0.25;
                }
            }
            return;
        }

        // Demo animation fallback (also used on macOS when cava is silent/absent)
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick);
        self.last_tick = now;

        let secs = dt.as_secs_f32().min(0.1);
        self.phase = (self.phase + secs * 2.2) % (std::f32::consts::TAU);

        for i in 0..self.bars {
            let x = self.phase + (i as f32 * 0.55);
            let v = (x.sin() * 0.6 + (x * 0.5).cos() * 0.35 + 0.75).clamp(0.0, 1.0);
            let prev = self.levels[i];
            self.levels[i] = prev * 0.82 + v * 0.18;
        }
    }

    pub fn render(&self, f: &mut Frame<'_>, area: Rect) {
        let block = Block::default().borders(Borders::NONE).style(Style::default());
        f.render_widget(block, area);

        let w = area.width as i32;
        let h = area.height as i32;
        if w <= 0 || h <= 0 {
            return;
        }

        let bars = self.bars as i32;
        let gap: i32 = 2;
        let bar_w: i32 = ((w - (bars - 1) * gap) / bars).max(1);
        let total_used = bars * bar_w + (bars - 1) * gap;
        let left = ((w - total_used) / 2).max(0);

        for i in 0..bars {
            let lvl = self.levels.get(i as usize).copied().unwrap_or(0.0);
            let bar_h = ((lvl * (h as f32)) as i32).clamp(0, h);

            let x0 = area.x as i32 + left + i * (bar_w + gap);
            let x1 = (x0 + bar_w).min(area.x as i32 + w);

            // Fill entire column first with background (avoids leftover chars)
            for y in 0..h {
                let yy = area.y as i32 + (h - 1 - y);
                let filled = y < bar_h;

                let frac = y as f32 / (h.max(1) as f32);
                let color = if frac > 0.66 {
                    Color::LightCyan
                } else if frac > 0.33 {
                    Color::Cyan
                } else {
                    Color::Blue
                };

                for xx in x0..x1 {
                    if xx >= area.x as i32 && xx < area.x as i32 + w {
                        let cell = f.buffer_mut().get_mut(xx as u16, yy as u16);
                        if filled {
                            cell.set_char('█').set_style(Style::default().fg(color));
                        } else {
                            cell.set_char(' ').set_style(Style::default());
                        }
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    pub async fn attach_to_mpv(&mut self, _mpv_socket_path: &str) -> Result<()> {
        Ok(())
    }
}

impl Drop for Visualizer {
    fn drop(&mut self) {
        self.detach_audio();
    }
}