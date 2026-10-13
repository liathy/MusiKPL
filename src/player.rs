use std::{
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use crate::{
    models::Track,
    process::{hide_console, ProcessRegistry},
};

static NEXT_PIPE_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct Player {
    child: Option<Child>,
    ipc_path: Option<String>,
    position_anchor: Option<Instant>,
    position: f64,
    pub(crate) paused: bool,
    pub(crate) volume: u8,
    registry: ProcessRegistry,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            child: None,
            ipc_path: None,
            position_anchor: None,
            position: 0.0,
            paused: false,
            volume: 100,
            registry: ProcessRegistry::default(),
        }
    }
}

impl Player {
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
        self.position_anchor = None;
        self.paused = false;
        self.position = 0.0;
    }

    pub(crate) fn position_seconds(&self) -> f64 {
        if self.paused {
            self.position
        } else {
            self.position
                + self
                    .position_anchor
                    .map(|anchor| anchor.elapsed().as_secs_f64())
                    .unwrap_or(0.0)
        }
    }

    pub(crate) fn set_paused(&mut self, paused: bool) {
        self.position = self.position_seconds();
        self.paused = paused;
        self.position_anchor = if paused { None } else { Some(Instant::now()) };
        self.send(if paused {
            "[\"set_property\",\"pause\",true]"
        } else {
            "[\"set_property\",\"pause\",false]"
        });
    }

    pub(crate) fn seek(&mut self, position: f64) {
        self.position = position.max(0.0);
        self.position_anchor = if self.paused {
            None
        } else {
            Some(Instant::now())
        };
        self.send(&format!(
            "[\"set_property\",\"time-pos\",{}]",
            self.position
        ));
    }

    pub(crate) fn play_stream(
        &mut self,
        track: &Track,
        stream_url: &str,
        mpv: &std::path::Path,
    ) -> Result<(), String> {
        let _ = track;
        self.stop();
        let pipe_name = format!(
            "\\\\.\\pipe\\ytmusic-{}-{}",
            std::process::id(),
            NEXT_PIPE_ID.fetch_add(1, Ordering::Relaxed)
        );
        let mut command = Command::new(mpv);
        hide_console(&mut command);
        command
            .args([
                "--no-video",
                "--force-window=no",
                "--audio-display=no",
                "--idle=no",
                &format!("--input-ipc-server={pipe_name}"),
                &format!("--volume={}", self.volume),
                stream_url,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = command
            .spawn()
            .map_err(|e| format!("Could not start mpv at {}: {e}", mpv.display()))?;
        self.registry.add(child.id());
        self.registry.assign_for_player(&child);
        self.child = Some(child);
        self.ipc_path = Some(pipe_name);
        self.position = 0.0;
        self.position_anchor = Some(Instant::now());
        Ok(())
    }

    pub(crate) fn is_finished(&mut self) -> bool {
        self.child
            .as_mut()
            .map(|child| child.try_wait().ok().flatten().is_some())
            .unwrap_or(false)
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
        let handle = CreateFileW(
            wide.as_ptr(),
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null_mut(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            0,
        );
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
