// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use glib::{clone, closure, Sender};
use gst::prelude::*;
use gtk::glib;
use gtk_macros::send;
use log::{debug, error, warn};

use crate::audio::{PlaybackAction, SeekDirection};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlayerState {
    Play,
    Pause,
    Stop,
}

impl Default for PlayerState {
    fn default() -> Self {
        PlayerState::Stop
    }
}

#[derive(Debug)]
pub struct PlayerBackend {
    sender: Sender<PlaybackAction>,
    playbin: RefCell<gst::Element>,
    state: Cell<PlayerState>,
    position_source: RefCell<Option<glib::SourceId>>,
    volume_signal: RefCell<Option<glib::signal::SignalHandlerId>>,
}

impl PlayerBackend {
    fn check_pulse_support() -> bool {
        let pulsesink = gst::ElementFactory::make("pulsesink", Some("pulsesink"));
        pulsesink.is_ok()
    }

    fn setup_pipeline(self: Rc<Self>) {
        let playbin = self.playbin.borrow();
        let bus = playbin
            .bus()
            .expect("Pipeline without bus. Shouldn't happen!");

        debug!("Adding playbin watch");
        bus.add_watch_local(
            clone!(@weak self as this => @default-return glib::Continue(false), move |_, msg| {
                use gst::MessageView;

                match msg.view() {
                    MessageView::Eos(..) => {
                        debug!("End of playbin stream");
                        send!(this.sender, PlaybackAction::PlayNext);
                        return glib::Continue(true);
                    }
                    MessageView::Error(err) => {
                        warn!("Pipeline error: {:?}", err);
                        send!(this.sender, PlaybackAction::Stop);
                        return glib::Continue(false);
                    }
                    _ => (),
                };

                glib::Continue(true)
            }),
        )
        .expect("failed to add bus watch");
    }

    fn setup_signals(self: Rc<Self>) {
        let playbin = self.playbin.borrow();

        let volume_id = playbin.connect_closure(
            "notify",
            false,
            closure!(@strong self.sender as sender => move |e: gst::Element, p: glib::ParamSpec| {
                if p.name() == "volume" {
                    let volume = e.property::<f64>("volume");
                    let cubic_volume = gst_audio::StreamVolume::convert_volume(
                        gst_audio::StreamVolumeFormat::Linear,
                        gst_audio::StreamVolumeFormat::Cubic,
                        volume,
                    );
                    send!(sender, PlaybackAction::VolumeChanged(cubic_volume));
                }
            }),
        );
        self.volume_signal.replace(Some(volume_id));

        playbin.connect_closure(
            "about-to-finish",
            false,
            closure!(@strong self.sender as sender => move |_playbin: gst::Element| {
                send!(sender, PlaybackAction::AboutToFinish);
            }),
        );
    }

    pub fn new(sender: Sender<PlaybackAction>) -> Rc<Self> {
        let audiosink = if Self::check_pulse_support() {
            gst::ElementFactory::make("pulsesink", Some("pulsesink")).expect("Missing pulsesink")
        } else {
            // If not, use autoaudiosink as fallback
            warn!("Cannot find PulseAudio");
            gst::ElementFactory::make("autoaudiosink", None).expect("Missing autoaudiosink")
        };

        let playbin = match gst::ElementFactory::make("playbin3", None) {
            Ok(e) => e,
            Err(_) => gst::ElementFactory::make("playbin", None).expect("Missing playbin"),
        };

        playbin.set_property("audio-sink", &audiosink);

        let res = Rc::new(Self {
            sender,
            playbin: RefCell::new(playbin),
            state: Cell::new(PlayerState::default()),
            position_source: RefCell::default(),
            volume_signal: RefCell::default(),
        });

        res.clone().setup_signals();
        res.clone().setup_pipeline();

        res
    }

    pub fn set_song_uri(&self, uri: Option<&str>) {
        let playbin = self.playbin.borrow();
        match playbin.set_state(gst::State::Null) {
            Ok(_) => {
                playbin.set_property("uri", uri);
            }
            Err(err) => {
                warn!("Unable to pause the pipeline: {}", err);
            }
        };
    }

    pub fn seek(&self, position: u64, duration: u64, offset: u64, direction: SeekDirection) {
        let offset = gst::ClockTime::from_seconds(offset);
        let position = gst::ClockTime::from_seconds(position);
        let duration = gst::ClockTime::from_seconds(duration);

        let destination = match direction {
            SeekDirection::Backwards if position >= offset => position.checked_sub(offset),
            SeekDirection::Backwards if position < offset => Some(gst::ClockTime::from_seconds(0)),
            SeekDirection::Forward if !duration.is_zero() && position + offset <= duration => {
                position.checked_add(offset)
            }
            SeekDirection::Forward if !duration.is_zero() && position + offset > duration => {
                Some(duration)
            }
            _ => None,
        };

        if let Some(destination) = destination {
            let playbin = self.playbin.borrow();
            playbin
                .set_state(gst::State::Paused)
                .expect("Pause to seek");
            match playbin.seek_simple(gst::SeekFlags::FLUSH, destination) {
                Ok(_) => {
                    playbin
                        .set_state(gst::State::Playing)
                        .expect("Play after seek");
                }
                Err(err) => warn!("Unable to seek {} in the pipeline: {}", destination, err),
            };
        }
    }

    pub fn seek_position(&self, position: u64) {
        let playbin = self.playbin.borrow();

        // We seek only after pausing
        playbin
            .set_state(gst::State::Paused)
            .expect("Pause to seek");

        match playbin.seek_simple(gst::SeekFlags::FLUSH, position * gst::ClockTime::SECOND) {
            Ok(_) => {
                if self.state.get() == PlayerState::Play {
                    playbin.set_state(gst::State::Playing).expect("Resume play after seek");
                }
            }
            Err(err) => warn!(
                "Unable to seek {} seconds in the pipeline: {}",
                position, err
            ),
        };
    }

    pub fn seek_start(&self) {
        self.seek_position(0);
    }

    pub fn play(&self) {
        if self.state.replace(PlayerState::Play) != PlayerState::Play {
            let playbin = self.playbin.borrow();

            match playbin.seek_simple(gst::SeekFlags::FLUSH, 0 * gst::ClockTime::SECOND) {
                Ok(_) => (),
                Err(err) => warn!("Unable to seek to the start: {}", err),
            };

            match playbin.set_state(gst::State::Playing) {
                Ok(_) => {
                    let pipeline_weak = playbin.downgrade();
                    let sender = self.sender.clone();
                    let id = glib::timeout_add_local(Duration::from_millis(250), move || {
                        let playbin = match pipeline_weak.upgrade() {
                            Some(playbin) => playbin,
                            None => return glib::Continue(false),
                        };
                        if let Some(position) = playbin.query_position::<gst::ClockTime>() {
                            send!(sender, PlaybackAction::UpdatePosition(position.seconds()));
                        }
                        glib::Continue(true)
                    });
                    self.position_source.replace(Some(id));
                }
                Err(err) => {
                    warn!("Unable to start playback: {}", err);
                    playbin
                        .set_state(gst::State::Ready)
                        .expect("Pipeline reset failed");
                }
            };
        }
    }

    pub fn pause(&self) {
        if self.state.replace(PlayerState::Pause) != PlayerState::Pause {
            let playbin = self.playbin.borrow();

            if let Some(position) = playbin.query_position::<gst::ClockTime>() {
                send!(self.sender, PlaybackAction::UpdatePosition(position.seconds()));
            }

            self.position_source.replace(None);

            match playbin.set_state(gst::State::Paused) {
                Ok(_) => (),
                Err(err) => {
                    warn!("Unable to pause playback: {}", err);
                    playbin
                        .set_state(gst::State::Null)
                        .expect("Pipeline reset failed");
                }
            }
        }
    }

    pub fn stop(&self) {
        if self.state.replace(PlayerState::Stop) != PlayerState::Stop {
            let playbin = self.playbin.borrow();

            if let Some(position) = playbin.query_position::<gst::ClockTime>() {
                send!(self.sender, PlaybackAction::UpdatePosition(position.seconds()));
            }

            self.position_source.replace(None);

            match playbin.set_state(gst::State::Ready) {
                Ok(_) => (),
                Err(err) => {
                    warn!("Unable to stop playback: {}", err);
                }
            };
        }
    }

    pub fn set_volume(&self, volume: f64) {
        let linear_volume = gst_audio::StreamVolume::convert_volume(
            gst_audio::StreamVolumeFormat::Cubic,
            gst_audio::StreamVolumeFormat::Linear,
            volume,
        );

        let playbin = &*self.playbin.borrow();

        // We need to block the signal handler when we know we're
        // mutating the volume, otherwise we're going to end up in
        // a cycle
        if let Some(volume_id) = self.volume_signal.borrow().as_ref() {
            debug!("Setting volume to: {}", &linear_volume);

            glib::signal::signal_handler_block(playbin, &volume_id);
            playbin.set_property("volume", volume);
            glib::signal::signal_handler_unblock(playbin, &volume_id);
        }
    }
}
