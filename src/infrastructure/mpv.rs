use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

use musikpl_core::audio::{AudioOutput, Fade, FadeCurve, PositionSample};

use crate::infrastructure::process::{hide_console, ProcessRegistry};

static NEXT_PIPE_ID: AtomicU64 = AtomicU64::new(1);

/// Low-level MPV process and IPC integration.
///
/// Playback state such as pause, position, and volume belongs to
/// `playback::Player`; this backend owns only the child process and pipe.
#[derive(Default)]
pub(crate) struct MpvBackend {
    child: Option<Child>,
    ipc_path: Option<String>,
    registry: ProcessRegistry,
    probe: Option<PositionProbe>,
}

/// Background reader of MPV's `time-pos`, so position never has to be
/// guessed from when the process was started and the UI thread never waits
/// on the pipe.
struct PositionProbe {
    latest: Arc<Mutex<Option<PositionSample>>>,
    stop: Arc<AtomicBool>,
}

impl Drop for PositionProbe {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl MpvBackend {
    pub(crate) fn set_registry(&mut self, registry: ProcessRegistry) {
        self.registry = registry;
    }

    pub(crate) fn send(&mut self, command: &str) {
        if let Some(path) = &self.ipc_path {
            let payload = format!("{{\"command\":{command}}}\n");
            #[cfg(windows)]
            write_windows_pipe(path, payload.as_bytes());
            #[cfg(unix)]
            if let Ok(mut socket) = std::os::unix::net::UnixStream::connect(path) {
                use std::io::Write;
                let _ = socket.write_all(payload.as_bytes());
                let _ = socket.flush();
            }
        }
    }

    pub(crate) fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let pid = child.id();
            let _ = child.kill();
            let _ = child.wait();
            self.registry.remove(pid);
        }
        #[cfg(unix)]
        if let Some(path) = &self.ipc_path {
            let _ = std::fs::remove_file(path);
        }
        self.ipc_path = None;
        self.probe = None;
    }

    /// Whether this platform can read MPV's real position.
    pub(crate) fn reports_position(&self) -> bool {
        cfg!(any(windows, unix))
    }

    /// The most recent position MPV reported for the current stream.
    pub(crate) fn latest_position(&self) -> Option<PositionSample> {
        let probe = self.probe.as_ref()?;
        probe.latest.lock().ok().and_then(|sample| *sample)
    }

    /// Start `stream_url`, optionally at an offset and paused, so a Jam guest
    /// can preload at the host's position and start exactly on time.
    pub(crate) fn play_stream(
        &mut self,
        stream_url: &str,
        mpv: &Path,
        volume: u8,
        start_secs: f64,
        paused: bool,
        fade_in: Option<Fade>,
    ) -> Result<(), String> {
        self.stop();
        let pipe_name = ipc_path(NEXT_PIPE_ID.fetch_add(1, Ordering::Relaxed));
        let mut command = Command::new(mpv);
        hide_console(&mut command);
        command.args([
            "--no-video",
            "--force-window=no",
            "--audio-display=no",
            "--idle=no",
            &format!("--input-ipc-server={pipe_name}"),
            &format!("--volume={volume}"),
        ]);
        if start_secs > 0.0 {
            command.arg(format!("--start={start_secs:.3}"));
        }
        if paused {
            command.arg("--pause=yes");
        }
        // Crossfading in: mpv's own fade, from the first sound.
        if let Some(fade) = fade_in {
            let curve = match fade.curve {
                FadeCurve::Linear => "tri",
                FadeCurve::Smooth => "hsin",
                FadeCurve::EqualPower => "qsin",
                FadeCurve::Exponential => "exp",
            };
            command.arg(format!(
                "--af=lavfi=[afade=t=in:st={:.3}:d={:.3}:curve={curve}]",
                start_secs.max(0.0),
                fade.seconds
            ));
        }
        command
            .arg(stream_url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = command
            .spawn()
            .map_err(|e| format!("Could not start mpv at {}: {e}", mpv.display()))?;
        self.registry.add(child.id());
        self.registry.assign_for_player(&child);
        self.child = Some(child);
        self.probe = spawn_probe(&pipe_name);
        self.ipc_path = Some(pipe_name);
        Ok(())
    }

    pub(crate) fn is_finished(&mut self) -> bool {
        self.child
            .as_mut()
            .map(|child| child.try_wait().ok().flatten().is_some())
            .unwrap_or(false)
    }
}

/// mpv as the core's [`AudioOutput`]: one mpv process per stream, steered
/// over its IPC pipe.
pub(crate) struct MpvOutput {
    backend: MpvBackend,
    mpv: PathBuf,
    registry: ProcessRegistry,
    volume: u8,
    /// The song fading out under a crossfade, until its fade ends.
    fading: Arc<Mutex<Option<MpvBackend>>>,
}

impl MpvOutput {
    pub(crate) fn new(mpv: PathBuf, registry: ProcessRegistry) -> Self {
        let mut backend = MpvBackend::default();
        backend.set_registry(registry.clone());
        Self {
            backend,
            mpv,
            registry,
            volume: 100,
            fading: Arc::default(),
        }
    }

    /// Stop the song fading out now (a pause, a stop, another fade).
    fn end_fade(&mut self) {
        if let Some(mut old) = self.fading.lock().ok().and_then(|mut fading| fading.take()) {
            old.stop();
        }
    }
}

/// Turn the song fading out down along the curve, then close it. mpv's
/// volume is cubic, so the loudness `gain` is volume × ∛gain.
fn fade_out(fading: &Mutex<Option<MpvBackend>>, volume: u8, fade: Fade) {
    const STEP: std::time::Duration = std::time::Duration::from_millis(40);
    let started = Instant::now();
    loop {
        let t = started.elapsed().as_secs_f64() / fade.seconds.max(0.1);
        let Ok(mut guard) = fading.lock() else { return };
        let Some(old) = guard.as_mut() else { return };
        if t >= 1.0 {
            old.stop();
            *guard = None;
            return;
        }
        let level = f64::from(volume) * fade.curve.gain(1.0 - t).cbrt();
        old.send(&format!("[\"set_property\",\"volume\",{level:.1}]"));
        drop(guard);
        std::thread::sleep(STEP);
    }
}

impl AudioOutput for MpvOutput {
    fn play(&mut self, url: &str, volume: u8, start_secs: f64, paused: bool) -> Result<(), String> {
        self.end_fade();
        self.volume = volume;
        self.backend
            .play_stream(url, &self.mpv, volume, start_secs, paused, None)
    }

    fn crossfade(
        &mut self,
        url: &str,
        volume: u8,
        start_secs: f64,
        fade: Fade,
    ) -> Result<(), String> {
        self.end_fade();
        self.volume = volume;
        // The playing mpv keeps going and fades out; a new one fades in.
        let mut fresh = MpvBackend::default();
        fresh.set_registry(self.registry.clone());
        let old = std::mem::replace(&mut self.backend, fresh);
        if let Ok(mut fading) = self.fading.lock() {
            *fading = Some(old);
        }
        let fading = self.fading.clone();
        std::thread::spawn(move || fade_out(&fading, volume, fade));
        self.backend
            .play_stream(url, &self.mpv, volume, start_secs, false, Some(fade))
    }

    fn set_paused(&mut self, paused: bool) {
        if paused {
            self.end_fade();
        }
        self.backend.send(if paused {
            "[\"set_property\",\"pause\",true]"
        } else {
            "[\"set_property\",\"pause\",false]"
        });
    }

    fn seek(&mut self, position: f64) {
        self.backend
            .send(&format!("[\"set_property\",\"time-pos\",{position}]"));
    }

    fn set_speed(&mut self, speed: f64) {
        self.backend
            .send(&format!("[\"set_property\",\"speed\",{speed:.3}]"));
    }

    fn set_volume(&mut self, volume: u8) {
        self.volume = volume;
        self.backend
            .send(&format!("[\"set_property\",\"volume\",{volume}]"));
    }

    fn stop(&mut self) {
        self.end_fade();
        self.backend.stop();
    }

    fn latest_position(&self) -> Option<PositionSample> {
        self.backend.latest_position()
    }

    fn reports_position(&self) -> bool {
        self.backend.reports_position()
    }

    fn is_finished(&mut self) -> bool {
        self.backend.is_finished()
    }
}

/// Where mpv listens for commands: a named pipe on Windows, a socket file
/// in the temporary folder elsewhere.
fn ipc_path(id: u64) -> String {
    let name = format!("musikpl-mpv-{}-{id}", std::process::id());
    if cfg!(windows) {
        format!(r"\\.\pipe\{name}")
    } else {
        std::env::temp_dir()
            .join(format!("{name}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(not(any(windows, unix)))]
fn spawn_probe(_pipe_name: &str) -> Option<PositionProbe> {
    None
}

#[cfg(any(windows, unix))]
fn spawn_probe(pipe_name: &str) -> Option<PositionProbe> {
    let probe = PositionProbe {
        latest: Arc::new(Mutex::new(None)),
        stop: Arc::new(AtomicBool::new(false)),
    };
    let (path, latest, stop) = (
        pipe_name.to_owned(),
        probe.latest.clone(),
        probe.stop.clone(),
    );
    std::thread::spawn(move || probe_positions(&path, &latest, &stop));
    Some(probe)
}

/// Open a connection to mpv's command channel: two handles, one to write
/// requests and one to read replies.
#[cfg(windows)]
fn connect_ipc(path: &str) -> std::io::Result<(std::fs::File, std::fs::File)> {
    let pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let read_half = pipe.try_clone()?;
    Ok((pipe, read_half))
}

#[cfg(unix)]
fn connect_ipc(
    path: &str,
) -> std::io::Result<(
    std::os::unix::net::UnixStream,
    std::os::unix::net::UnixStream,
)> {
    let socket = std::os::unix::net::UnixStream::connect(path)?;
    let read_half = socket.try_clone()?;
    Ok((socket, read_half))
}

/// Ask MPV for `time-pos` a few times per second over its own pipe
/// connection until playback stops or MPV exits.
#[cfg(any(windows, unix))]
fn probe_positions(path: &str, latest: &Mutex<Option<PositionSample>>, stop: &AtomicBool) {
    use std::{
        io::{BufRead, BufReader, Write},
        thread::sleep,
        time::Duration,
    };

    // Frequent enough to re-anchor drift estimates, cheap enough to ignore.
    const INTERVAL: Duration = Duration::from_millis(200);
    // MPV creates the pipe shortly after it starts; give a slow start room.
    const CONNECT_WINDOW: Duration = Duration::from_secs(15);

    let started = Instant::now();
    let (mut pipe, read_half) = loop {
        if stop.load(Ordering::Relaxed) || started.elapsed() > CONNECT_WINDOW {
            return;
        }
        match connect_ipc(path) {
            Ok(halves) => break halves,
            Err(_) => sleep(Duration::from_millis(50)),
        }
    };
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    let mut request_id = 0u64;

    while !stop.load(Ordering::Relaxed) {
        request_id += 1;
        let sent = Instant::now();
        let request = format!(
            "{{\"command\":[\"get_property\",\"time-pos\"],\"request_id\":{request_id}}}\n"
        );
        if pipe.write_all(request.as_bytes()).is_err() {
            return;
        }
        // Skip MPV's unrelated event lines until our reply arrives.
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(read) if read > 0 => {}
                _ => return,
            }
            let Ok(reply) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if reply.get("request_id").and_then(|id| id.as_u64()) != Some(request_id) {
                continue;
            }
            // `time-pos` is unavailable until the stream has loaded.
            if let Some(position) = reply.get("data").and_then(|data| data.as_f64()) {
                let taken = sent + sent.elapsed() / 2;
                if let Ok(mut latest) = latest.lock() {
                    *latest = Some(PositionSample { position, taken });
                }
            }
            break;
        }
        sleep(INTERVAL);
    }
}

#[cfg(windows)]
fn write_windows_pipe(path: &str, payload: &[u8]) {
    use std::ptr::null_mut;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    };
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        // MPV re-creates its pipe instance after each client connects, so the
        // pipe can be busy for an instant; retry briefly rather than silently
        // dropping a pause or seek.
        let mut handle = INVALID_HANDLE_VALUE;
        for _ in 0..5 {
            handle = CreateFileW(
                wide.as_ptr(),
                FILE_GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null_mut(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                0,
            );
            if handle != INVALID_HANDLE_VALUE {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
        if handle == INVALID_HANDLE_VALUE {
            return;
        }
        let mut written = 0u32;
        let _ = WriteFile(
            handle,
            payload.as_ptr(),
            payload.len() as u32,
            &mut written,
            null_mut(),
        );
        CloseHandle(handle);
    }
}
