use std::{
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

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

/// MPV's own playback position at a known instant.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PositionSample {
    pub(crate) position: f64,
    pub(crate) taken: Instant,
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
            #[cfg(not(windows))]
            if let Ok(mut pipe) = std::fs::OpenOptions::new().write(true).open(path) {
                use std::io::Write;
                let _ = pipe.write_all(payload.as_bytes());
                let _ = pipe.flush();
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
        self.ipc_path = None;
        self.probe = None;
    }

    /// Whether this platform can read MPV's real position.
    pub(crate) fn reports_position(&self) -> bool {
        cfg!(windows)
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
    ) -> Result<(), String> {
        self.stop();
        let pipe_name = format!(
            "\\\\.\\pipe\\ytmusic-{}-{}",
            std::process::id(),
            NEXT_PIPE_ID.fetch_add(1, Ordering::Relaxed)
        );
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

#[cfg(not(windows))]
fn spawn_probe(_pipe_name: &str) -> Option<PositionProbe> {
    None
}

#[cfg(windows)]
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

/// Ask MPV for `time-pos` a few times per second over its own pipe
/// connection until playback stops or MPV exits.
#[cfg(windows)]
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
    let mut pipe = loop {
        if stop.load(Ordering::Relaxed) || started.elapsed() > CONNECT_WINDOW {
            return;
        }
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
        {
            Ok(pipe) => break pipe,
            Err(_) => sleep(Duration::from_millis(50)),
        }
    };
    let Ok(read_half) = pipe.try_clone() else {
        return;
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
