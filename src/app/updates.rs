//! Check for updates / Install update, at the bottom of the menu.

use std::sync::{
    atomic::{AtomicU32, Ordering},
    mpsc::{self, Receiver, TryRecvError},
    Arc,
};

use eframe::egui;

use crate::{
    infrastructure::updater::{self, Release},
    ui::overlays::UpdateView,
};

use super::YtMusicApp;

#[derive(Default)]
pub(super) enum Updates {
    #[default]
    Idle,
    Checking(Receiver<Result<Option<Release>, String>>),
    UpToDate,
    Available(Release),
    Installing {
        /// Download progress in per-mille.
        progress: Arc<AtomicU32>,
        done: Receiver<Result<(), String>>,
    },
    Failed(String),
}

impl Updates {
    pub(super) fn busy(&self) -> bool {
        matches!(self, Self::Checking(_) | Self::Installing { .. })
    }

    pub(super) fn view(&self) -> UpdateView<'_> {
        match self {
            Self::Idle => UpdateView::Idle,
            Self::Checking(_) => UpdateView::Checking,
            Self::UpToDate => UpdateView::UpToDate,
            Self::Available(release) => UpdateView::Available {
                version: &release.version,
            },
            Self::Installing { progress, .. } => UpdateView::Downloading {
                percent: progress.load(Ordering::Relaxed) / 10,
            },
            Self::Failed(error) => UpdateView::Failed(error),
        }
    }
}

impl YtMusicApp {
    pub(super) fn check_for_updates(&mut self) {
        if self.updates.busy() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(updater::check());
        });
        self.updates = Updates::Checking(receiver);
    }

    pub(super) fn install_update(&mut self) {
        let Updates::Available(release) = &self.updates else {
            return;
        };
        let release = release.clone();
        let progress = Arc::new(AtomicU32::new(0));
        let (sender, done) = mpsc::channel();
        let shared = progress.clone();
        std::thread::spawn(move || {
            let _ = sender.send(updater::install(&release, &shared));
        });
        self.updates = Updates::Installing { progress, done };
    }

    /// Collect check and install results. Once the new version is ready to
    /// take over, MusiKPL closes (saving everything) and the update reopens it.
    pub(super) fn poll_updates(&mut self, ctx: &egui::Context) {
        match &self.updates {
            Updates::Checking(receiver) => match receiver.try_recv() {
                Ok(Ok(Some(release))) => self.updates = Updates::Available(release),
                Ok(Ok(None)) => self.updates = Updates::UpToDate,
                Ok(Err(error)) => self.updates = Updates::Failed(error),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.updates = Updates::Failed("The update check stopped unexpectedly".into())
                }
            },
            Updates::Installing { done, .. } => match done.try_recv() {
                Ok(Ok(())) => {
                    self.updates = Updates::Idle;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Ok(Err(error)) => self.updates = Updates::Failed(error),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.updates = Updates::Failed("The update stopped unexpectedly".into())
                }
            },
            _ => {}
        }
    }
}
