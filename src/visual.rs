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
/// - Fallback: deterministic demo animation (so the UI still works)
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
        }
    }

    pub fn bars(&self) -> usize {
        self.bars
    }

    pub fn frame_budget(&self) -> Duration {
        let fps = self.fps.max(1).min(60);
        Duration::from_millis((1000 / fps) as u64)
    }

    pub fn is_audio_attached(&self) -> bool {
        self.cava_child.is_some()
    }

    /// Try to start cava.
    ///
    /// This is intentionally "best effort": if cava is missing or fails, we silently
    /// keep demo mode.
    pub fn try_attach_cava(&mut self) -> Result<()> {
        if self.cava_child.is_some() {
            return Ok(());
        }

        // Build a tiny cava config on stdin.
        // Output: raw ASCII values 0..ascii_max separated by ';' and newline per frame.
        // We'll map those to 0..1.
        let bars = self.bars;
        let ascii_max = 100;
        let config = format!(
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
            ascii_max = ascii_max
        );

        let mut child = Command::new("cava")
            .arg("-p")
            .arg("/dev/stdin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();

        let Ok(mut child) = child else {
            return Ok(());
        };

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(config.as_bytes());
        }

        let stdout = match child.stdout.take() {
            Some(o) => o,
            None => {
                let _ = child.kill();
                return Ok(());
            }
        };

        let shared = self.shared_levels.clone();
        let stop = self.stop_flag.clone();

        self.stop_flag
            .store(false, std::sync::atomic::Ordering::Relaxed);

        let handle = std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                // line: "12;0;55;..."
                let mut out = vec![0.0f32; bars];
                for (i, part) in line.split(';').take(bars).enumerate() {
                    if let Ok(v) = part.trim().parse::<f32>() {
                        out[i] = (v / ascii_max as f32).clamp(0.0, 1.0);
                    }
                }
                if let Ok(mut guard) = shared.lock() {
                    *guard = out;
                }
            }
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
    }

    pub fn tick(&mut self) {
        // Prefer real audio levels if available
        if self.cava_child.is_some() {
            if let Ok(guard) = self.shared_levels.lock() {
                // mild smoothing
                for i in 0..self.bars {
                    let target = guard.get(i).copied().unwrap_or(0.0);
                    let prev = self.levels[i];
                    self.levels[i] = prev * 0.75 + target * 0.25;
                }
            }
            return;
        }

        // Demo animation fallback
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

            for y in 0..h {
                let yy = area.y as i32 + (h - 1 - y);
                let filled = y < bar_h;
                if !filled {
                    continue;
                }

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
                        f.buffer_mut()
                            .get_mut(xx as u16, yy as u16)
                            .set_char('█')
                            .set_style(Style::default().fg(color));
                    }
                }
            }
        }
    }

    // Placeholder for a future true mpv-only tap.
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
