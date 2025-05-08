use crate::config::AppConfig;

pub struct AudioPlayer {
    use_mpv: bool,
    process: Option<std::process::Child>,
    config: AppConfig,
}

impl AudioPlayer {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            use_mpv: config.player.use_mpv,
            process: None,
            config: config.clone(),
        }
    }

    pub fn play_song(&mut self, song_id: &str) {
        self.stop();
        
        let stream_url = format!("{}/rest/stream?id={}&{}", 
            self.config.server_url,
            song_id,
            self.auth_params()
        );

        if self.use_mpv {
            self.process = std::process::Command::new("mpv")
                .arg("--no-video")
                .arg("--quiet")
                .arg(&stream_url)
                .spawn()
                .ok();
        } else {
            println!("Playing: {}", stream_url);
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
        }
    }

    fn auth_params(&self) -> String {
        let token = format!("{:x}", md5::compute(format!("{}:{}", self.config.username, self.config.password)));
        format!("u={}&t={}&s=termnavi&v=1.16.1&c=termnavi", self.config.username, token)
    }
}
