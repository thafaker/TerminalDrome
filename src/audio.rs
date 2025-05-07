// src/audio.rs
use crate::config::AppConfig;

pub struct AudioPlayer {
    use_mpv: bool,
    process: Option<std::process::Child>,
}

impl AudioPlayer {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            use_mpv: config.player.use_mpv,
            process: None,
        }
    }

    pub fn play(&mut self, url: &str) {
        self.stop();
        
        if self.use_mpv {
            self.process = std::process::Command::new("mpv")
                .arg("--no-video")
                .arg("--quiet")
                .arg(url)
                .spawn()
                .ok();
        } else {
            // Alternative Implementierung für Nicht-MPV-Systeme
            println!("Playing: {}", url);
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
        }
    }
}
