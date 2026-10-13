// SPDX-FileCopyrightText: 2026 Amberol contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use async_channel::Sender;
use gtk::{gio, prelude::*};
use log::warn;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};

use super::{Controller, PlaybackAction, PlaybackState, RepeatMode, Song};

pub struct MediaController {
    controls: RefCell<Option<MediaControls>>,
    sender: Sender<PlaybackAction>,
    playing: Arc<AtomicBool>,
    state: Cell<PlaybackState>,
    position: Arc<AtomicU64>,
}

impl MediaController {
    pub fn new(sender: Sender<PlaybackAction>) -> Self {
        Self {
            controls: RefCell::new(None),
            sender,
            playing: Arc::new(AtomicBool::new(false)),
            state: Cell::new(PlaybackState::Stopped),
            position: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn setup(&self, window: &gtk::Window) {
        if self.controls.borrow().is_some() {
            return;
        }

        #[cfg(target_os = "windows")]
        let hwnd = {
            #[link(name = "gtk-4")]
            extern "C" {
                fn gdk_win32_surface_get_handle(
                    surface: *mut std::ffi::c_void,
                ) -> *mut std::ffi::c_void;
            }
            let Some(surface) = window.surface() else {
                return;
            };
            // The native surface belongs to the presented GTK window.
            Some(unsafe { gdk_win32_surface_get_handle(surface.as_ptr().cast()) })
        };
        #[cfg(not(target_os = "windows"))]
        let hwnd = None;

        let config = PlatformConfig {
            dbus_name: "io.bassi.Amberol",
            display_name: "Amberol",
            hwnd,
        };
        let mut controls = match MediaControls::new(config) {
            Ok(controls) => controls,
            Err(err) => {
                warn!("Unable to create system media controls: {err:?}");
                return;
            }
        };
        let sender = self.sender.clone();
        let playing = self.playing.clone();
        let position = self.position.clone();
        if let Err(err) = controls.attach(move |event| {
            let current = position.load(Ordering::Relaxed);
            let action = match event {
                MediaControlEvent::Play => Some(PlaybackAction::Play),
                MediaControlEvent::Pause => Some(PlaybackAction::Pause),
                MediaControlEvent::Toggle => Some(if playing.load(Ordering::Relaxed) {
                    PlaybackAction::Pause
                } else {
                    PlaybackAction::Play
                }),
                MediaControlEvent::Next => Some(PlaybackAction::SkipNext),
                MediaControlEvent::Previous => Some(PlaybackAction::SkipPrevious),
                MediaControlEvent::Stop => Some(PlaybackAction::Stop),
                MediaControlEvent::SetPosition(pos) => Some(PlaybackAction::Seek(pos.0.as_secs())),
                MediaControlEvent::SeekBy(SeekDirection::Forward, offset) => Some(
                    PlaybackAction::Seek(current.saturating_add(offset.as_secs())),
                ),
                MediaControlEvent::SeekBy(SeekDirection::Backward, offset) => Some(
                    PlaybackAction::Seek(current.saturating_sub(offset.as_secs())),
                ),
                MediaControlEvent::Seek(SeekDirection::Forward) => {
                    Some(PlaybackAction::Seek(current.saturating_add(10)))
                }
                MediaControlEvent::Seek(SeekDirection::Backward) => {
                    Some(PlaybackAction::Seek(current.saturating_sub(10)))
                }
                MediaControlEvent::SetVolume(volume) => {
                    Some(PlaybackAction::VolumeChanged(volume.clamp(0.0, 1.0)))
                }
                MediaControlEvent::Raise => Some(PlaybackAction::Raise),
                _ => None,
            };
            if let Some(action) = action {
                let _ = sender.send_blocking(action);
            }
        }) {
            warn!("Unable to attach system media controls: {err:?}");
            return;
        }
        self.controls.replace(Some(controls));
    }
}

impl Controller for Rc<MediaController> {
    fn set_playback_state(&self, state: &PlaybackState) {
        self.state.set(*state);
        self.playing
            .store(*state == PlaybackState::Playing, Ordering::Relaxed);
        let playback = match state {
            PlaybackState::Playing => MediaPlayback::Playing {
                progress: Some(MediaPosition(Duration::from_secs(
                    self.position.load(Ordering::Relaxed),
                ))),
            },
            PlaybackState::Paused => MediaPlayback::Paused {
                progress: Some(MediaPosition(Duration::from_secs(
                    self.position.load(Ordering::Relaxed),
                ))),
            },
            PlaybackState::Stopped => MediaPlayback::Stopped,
        };
        if let Some(controls) = self.controls.borrow_mut().as_mut() {
            if let Err(err) = controls.set_playback(playback) {
                warn!("Unable to update media playback: {err:?}");
            }
        }
    }

    fn set_song(&self, song: &Song) {
        let title = song.title();
        let artist = song.artist();
        let album = song.album();
        #[cfg(target_os = "windows")]
        let cover = song
            .cover_cache()
            .map(|path| format!("file://{}", path.display()));
        #[cfg(not(target_os = "windows"))]
        let cover = song
            .cover_cache()
            .map(|path| gio::File::for_path(path).uri());
        let metadata = MediaMetadata {
            title: Some(&title),
            artist: Some(&artist),
            album: Some(&album),
            cover_url: cover.as_deref(),
            duration: Some(Duration::from_secs(song.duration())),
        };
        if let Some(controls) = self.controls.borrow_mut().as_mut() {
            if let Err(err) = controls.set_metadata(metadata) {
                warn!("Unable to update media artwork: {err:?}");
                let _ = controls.set_metadata(MediaMetadata {
                    title: Some(&title),
                    artist: Some(&artist),
                    album: Some(&album),
                    cover_url: None,
                    duration: Some(Duration::from_secs(song.duration())),
                });
            }
        }
    }

    fn set_position(&self, position: u64) {
        self.position.store(position, Ordering::Relaxed);
        if self.state.get() != PlaybackState::Stopped {
            self.set_playback_state(&self.state.get());
        }
    }

    fn set_repeat_mode(&self, _mode: RepeatMode) {}
}
