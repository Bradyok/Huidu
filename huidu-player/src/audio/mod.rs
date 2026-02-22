/// Background music player.
///
/// Plays a playlist of audio files (MP3 / WAV / OGG) when a program with a
/// `<backgroundMusic>` element is active. Audio is optional — if no audio
/// output device is available (common on embedded boards without a DAC) the
/// module silently becomes a no-op rather than crashing.
use std::path::Path;
use tracing::warn;

pub struct BackgroundMusicPlayer {
    /// The rodio output stream must stay alive as long as the sink is used.
    _stream: Option<rodio::OutputStream>,
    /// Handle used to create new sinks.
    handle: Option<rodio::OutputStreamHandle>,
    /// Active sink (None = nothing playing).
    sink: Option<rodio::Sink>,
    /// Files currently playing (used to avoid restarting identical playlists).
    current_files: Vec<String>,
}

impl BackgroundMusicPlayer {
    /// Initialise the player. Falls back to no-op if no audio device is found.
    pub fn new() -> Self {
        match rodio::OutputStream::try_default() {
            Ok((stream, handle)) => Self {
                _stream: Some(stream),
                handle: Some(handle),
                sink: None,
                current_files: Vec::new(),
            },
            Err(e) => {
                warn!("Audio output unavailable (no background music): {}", e);
                Self { _stream: None, handle: None, sink: None, current_files: Vec::new() }
            }
        }
    }

    /// Start playing `files` from `program_dir`.
    ///
    /// If the same playlist is already playing this is a no-op.
    /// Unsupported / missing files are silently skipped.
    pub fn play(&mut self, files: &[String], program_dir: &Path) {
        // No-op if the same playlist is still active
        if self.current_files == files
            && let Some(ref s) = self.sink
                && !s.empty() {
                    return;
                }

        let handle = match &self.handle {
            Some(h) => h,
            None => return, // no audio device
        };

        // Drop the old sink first so its thread terminates cleanly
        self.sink = None;

        let new_sink = match rodio::Sink::try_new(handle) {
            Ok(s) => s,
            Err(e) => {
                warn!("Could not create audio sink: {}", e);
                return;
            }
        };

        for file_name in files {
            let path = program_dir.join(file_name);
            match std::fs::File::open(&path) {
                Ok(file) => {
                    let reader = std::io::BufReader::new(file);
                    match rodio::Decoder::new(reader) {
                        Ok(source) => new_sink.append(source),
                        Err(e) => warn!("Cannot decode {}: {}", path.display(), e),
                    }
                }
                Err(e) => warn!("Cannot open {}: {}", path.display(), e),
            }
        }

        if !new_sink.empty() {
            new_sink.play();
            self.sink = Some(new_sink);
            self.current_files = files.to_vec();
        }
    }

    /// Stop any currently playing audio.
    pub fn stop(&mut self) {
        self.sink = None;
        self.current_files.clear();
    }

    /// Set playback volume (0–100).
    pub fn set_volume(&self, level: u8) {
        if let Some(ref s) = self.sink {
            s.set_volume(level as f32 / 100.0);
        }
    }

    /// Returns `true` if audio is currently playing.
    pub fn is_playing(&self) -> bool {
        self.sink.as_ref().map(|s| !s.empty()).unwrap_or(false)
    }
}
