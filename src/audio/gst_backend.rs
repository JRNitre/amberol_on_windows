// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

use async_channel::Sender;
use gst::prelude::*;
use gtk::{glib, prelude::*};
use log::{debug, error};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use crate::audio::{PlaybackAction, ReplayGainMode, SeekDirection};

#[derive(Debug)]
pub struct GstBackend {
    pipeline: gst::Element,
    replaygain: Option<GstReplayGain>,
    desired_volume: Arc<AtomicU64>,
    gain: Arc<AtomicU64>,
    generation: Arc<AtomicU64>,
    next_uri: Arc<std::sync::Mutex<Option<String>>>,
    queued_uri: Arc<std::sync::Mutex<Option<String>>>,
    pending_seek: Arc<AtomicU64>,
    watch: Option<gst::bus::BusWatchGuard>,
    tick: Option<glib::SourceId>,
}

#[derive(Debug)]
pub struct GstReplayGain {
    rg_filter_bin: gst::Element,
    rg_volume: gst::Element,
}

impl GstReplayGain {
    pub fn new() -> Result<GstReplayGain, Box<dyn std::error::Error>> {
        let rg_volume = gst::ElementFactory::make_with_name("rgvolume", Some("rg volume"))?;
        let rg_limiter = gst::ElementFactory::make_with_name("rglimiter", Some("rg limiter"))?;

        let filter_bin = gst::Bin::builder().name("filter bin").build();
        filter_bin.add(&rg_volume)?;
        filter_bin.add(&rg_limiter)?;
        rg_volume.link(&rg_limiter)?;

        let pad_src = rg_limiter.static_pad("src").unwrap();
        pad_src.set_active(true).unwrap();
        let ghost_src = gst::GhostPad::with_target(&pad_src)?;
        filter_bin.add_pad(&ghost_src)?;

        let pad_sink = rg_volume.static_pad("sink").unwrap();
        pad_sink.set_active(true).unwrap();
        let ghost_sink = gst::GhostPad::with_target(&pad_sink)?;
        filter_bin.add_pad(&ghost_sink)?;

        Ok(Self {
            rg_filter_bin: filter_bin.upcast(),
            rg_volume,
        })
    }

    pub fn set_mode(&self, playbin: gst::Element, replaygain: ReplayGainMode) {
        let identity = gst::ElementFactory::make_with_name("identity", None).unwrap();

        let (filter, album_mode) = match replaygain {
            ReplayGainMode::Album => (self.rg_filter_bin.as_ref(), true),
            ReplayGainMode::Track => (self.rg_filter_bin.as_ref(), false),
            ReplayGainMode::Off => (&identity, true),
        };

        self.rg_volume.set_property("album-mode", album_mode);
        playbin.set_property("audio-filter", filter);
    }
}

impl GstBackend {
    pub fn new(sender: Sender<PlaybackAction>) -> Self {
        let pipeline = gst::ElementFactory::make("playbin")
            .build()
            .expect("Missing playbin");
        pipeline.set_property_from_str("flags", "audio");
        #[cfg(test)]
        pipeline.set_property(
            "audio-sink",
            gst::ElementFactory::make("fakesink")
                .property("sync", true)
                .build()
                .unwrap(),
        );
        let desired_volume = Arc::new(AtomicU64::new(1.0_f64.to_bits()));
        let gain = Arc::new(AtomicU64::new(1.0_f64.to_bits()));
        let generation = Arc::new(AtomicU64::new(0));
        let next_uri = Arc::new(std::sync::Mutex::new(None::<String>));
        let queued_uri = Arc::new(std::sync::Mutex::new(None::<String>));
        let pending_seek = Arc::new(AtomicU64::new(u64::MAX));
        let upcoming = next_uri.clone();
        let queued = queued_uri.clone();
        pipeline.connect("about-to-finish", false, move |values| {
            let pipeline = values[0].get::<gst::Element>().unwrap();
            if let Some(uri) = upcoming.lock().unwrap().take() {
                *queued.lock().unwrap() = Some(uri.clone());
                pipeline.set_property("uri", &uri);
            }
            None
        });
        let bus = pipeline.bus().unwrap();
        let bus_pipeline = pipeline.clone();
        let bus_sender = sender.clone();
        let seek = pending_seek.clone();
        let queued = queued_uri.clone();
        let watch = bus
            .add_watch_local(move |_, message| {
                match message.view() {
                    gst::MessageView::Eos(_) => {
                        let _ = bus_sender.send_blocking(PlaybackAction::PlayNext);
                    }
                    gst::MessageView::Error(err) => {
                        error!("Playback error: {} ({:?})", err.error(), err.debug());
                        let _ = bus_sender.send_blocking(PlaybackAction::Stop);
                    }
                    gst::MessageView::StreamStart(_) => {
                        if let Some(uri) = queued.lock().unwrap().take() {
                            let _ = bus_sender.send_blocking(PlaybackAction::StreamStarted(uri));
                        }
                    }
                    gst::MessageView::AsyncDone(_) => {
                        let position = seek.swap(u64::MAX, Ordering::Relaxed);
                        if position != u64::MAX {
                            let _ = bus_pipeline.seek_simple(
                                gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                                gst::ClockTime::from_seconds(position),
                            );
                        }
                    }
                    _ => (),
                }
                glib::ControlFlow::Continue
            })
            .unwrap();
        let tick_pipeline = pipeline.clone();
        let tick_sender = sender.clone();
        let tick = glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
            if let Some(position) = tick_pipeline.query_position::<gst::ClockTime>() {
                let _ =
                    tick_sender.send_blocking(PlaybackAction::UpdatePosition(position.seconds()));
            }
            glib::ControlFlow::Continue
        });
        Self {
            pipeline,
            replaygain: GstReplayGain::new().ok(),
            desired_volume,
            gain,
            generation,
            next_uri,
            queued_uri,
            pending_seek,
            watch: Some(watch),
            tick: Some(tick),
        }
    }

    pub fn set_next_uri(&self, uri: Option<String>) {
        *self.next_uri.lock().unwrap() = uri;
    }

    pub fn set_song_uri(&self, uri: Option<&str>) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.gain.store(1.0f64.to_bits(), Ordering::Relaxed);
        *self.next_uri.lock().unwrap() = None;
        let _ = self.pipeline.set_state(gst::State::Ready);
        *self.queued_uri.lock().unwrap() = None;
        self.pending_seek.store(u64::MAX, Ordering::Relaxed);
        if let Some(uri) = uri {
            self.pipeline.set_property("uri", uri);
        }
        self.restore_volume();
    }

    pub fn restore_position(&self, position: u64) {
        self.pending_seek.store(position, Ordering::Relaxed);
        let _ = self.pipeline.set_state(gst::State::Paused);
    }

    pub fn seek(&self, position: u64, duration: u64, offset: u64, direction: SeekDirection) {
        let destination = match direction {
            SeekDirection::Backwards => position.saturating_sub(offset),
            SeekDirection::Forward => position.saturating_add(offset).min(duration),
        };
        self.seek_position(destination);
    }
    pub fn seek_position(&self, position: u64) {
        if let Err(err) = self.pipeline.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
            gst::ClockTime::from_seconds(position),
        ) {
            debug!("Seek failed: {err}");
        }
    }
    pub fn seek_start(&self) {
        self.seek_position(0);
    }

    fn fade(&self, target: f64, pause_after: bool) {
        let token = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let generation = self.generation.clone();
        let gain = self.gain.clone();
        let volume = self.desired_volume.clone();
        let pipeline = self.pipeline.clone();
        let start = f64::from_bits(gain.load(Ordering::Relaxed));
        let began = std::time::Instant::now();
        glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
            if token != generation.load(Ordering::Relaxed) {
                return glib::ControlFlow::Break;
            }
            let t = (began.elapsed().as_secs_f64() / 0.18).min(1.0);
            let level = start + (target - start) * t;
            gain.store(level.to_bits(), Ordering::Relaxed);
            let desired = f64::from_bits(volume.load(Ordering::Relaxed));
            let linear = gst_audio::StreamVolume::convert_volume(
                gst_audio::StreamVolumeFormat::Cubic,
                gst_audio::StreamVolumeFormat::Linear,
                desired,
            ) * level;
            pipeline.set_property("volume", linear);
            if t >= 1.0 {
                if pause_after {
                    let _ = pipeline.set_state(gst::State::Paused);
                }
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
    pub fn play(&self) {
        if crate::utils::settings_manager().boolean("fade-playback") {
            // A fresh track starts at zero; reversing a pending pause starts at its current gain.
            if self.pipeline.current_state() != gst::State::Playing {
                self.gain.store(0f64.to_bits(), Ordering::Relaxed);
            }
            self.restore_volume();
            let _ = self.pipeline.set_state(gst::State::Playing);
            self.fade(1.0, false);
        } else {
            self.generation.fetch_add(1, Ordering::Relaxed);
            self.gain.store(1f64.to_bits(), Ordering::Relaxed);
            self.restore_volume();
            let _ = self.pipeline.set_state(gst::State::Playing);
        }
    }
    pub fn pause(&self) {
        if crate::utils::settings_manager().boolean("fade-playback")
            && self.pipeline.current_state() == gst::State::Playing
        {
            self.fade(0.0, true);
        } else {
            self.generation.fetch_add(1, Ordering::Relaxed);
            let _ = self.pipeline.set_state(gst::State::Paused);
        }
    }
    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        *self.next_uri.lock().unwrap() = None;
        *self.queued_uri.lock().unwrap() = None;
        let _ = self.pipeline.set_state(gst::State::Ready);
    }
    pub fn set_volume(&self, volume: f64) {
        self.desired_volume
            .store(volume.to_bits(), Ordering::Relaxed);
        self.restore_volume();
    }
    fn restore_volume(&self) {
        let desired = f64::from_bits(self.desired_volume.load(Ordering::Relaxed));
        let gain = f64::from_bits(self.gain.load(Ordering::Relaxed));
        let linear = gst_audio::StreamVolume::convert_volume(
            gst_audio::StreamVolumeFormat::Cubic,
            gst_audio::StreamVolumeFormat::Linear,
            desired,
        ) * gain;
        self.pipeline.set_property("volume", linear);
    }
    pub fn set_replaygain(&self, replaygain: ReplayGainMode) {
        if let Some(ref r) = self.replaygain {
            r.set_mode(self.pipeline.clone(), replaygain);
        }
    }
    pub fn replaygain_available(&self) -> bool {
        self.replaygain.is_some()
    }

    #[cfg(test)]
    pub fn actual_volume(&self) -> f64 {
        self.pipeline.property("volume")
    }

    #[cfg(test)]
    pub fn actual_state(&self) -> gst::State {
        self.pipeline.current_state()
    }
}
impl Drop for GstBackend {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        if let Some(tick) = self.tick.take() {
            tick.remove();
        }
        self.watch.take();
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
