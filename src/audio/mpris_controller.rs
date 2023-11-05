// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc};

use async_channel::Sender;
use glib::clone;
use gtk::{gio, glib, prelude::*};
use log::error;
use mpris_server::{LoopStatus, Metadata, PlaybackStatus, Player, Time};

use crate::{
    audio::{Controller, PlaybackAction, PlaybackState, RepeatMode, Song},
    config::APPLICATION_ID,
};

#[derive(Debug)]
pub struct MprisController {
    sender: Sender<PlaybackAction>,
    mpris: Rc<Player>,

    song: RefCell<Option<Song>>,
}

impl MprisController {
    pub fn new(sender: Sender<PlaybackAction>) -> Self {
        let mpris = Rc::new(
            Player::builder(APPLICATION_ID)
                .identity("Amberol")
                .desktop_entry(APPLICATION_ID)
                .can_raise(true)
                .can_play(false)
                .can_pause(true)
                .can_seek(true)
                .can_go_next(true)
                .can_go_previous(true)
                .can_set_fullscreen(false)
                .build(),
        );

        let mpris_task = mpris.init_and_run();
        glib::spawn_future_local(async move {
            if let Err(err) = mpris_task.await {
                error!("Failed to run MPRIS server: {:?}", err);
            }
        });

        let res = Self {
            sender,
            mpris,
            song: RefCell::new(None),
        };

        res.setup_signals();

        res
    }

    fn setup_signals(&self) {
        self.mpris
            .connect_play_pause(clone!(@strong self.sender as sender => move |mpris| {
                match mpris.playback_status() {
                    PlaybackStatus::Paused => {
                        if let Err(e) = sender.send_blocking(PlaybackAction::Play) {
                            error!("Unable to send Play: {e}");
                        }
                    },
                    PlaybackStatus::Stopped => {
                        if let Err(e) = sender.send_blocking(PlaybackAction::Stop) {
                            error!("Unable to send Stop: {e}");
                        }
                    },
                    _ => {
                        if let Err(e) = sender.send_blocking(PlaybackAction::Pause) {
                            error!("Unable to send Pause: {e}");
                        }
                    },
                };
            }));

        self.mpris
            .connect_play(clone!(@strong self.sender as sender => move |_| {
                if let Err(e) = sender.send_blocking(PlaybackAction::Play) {
                    error!("Unable to send Play: {e}");
                }
            }));

        self.mpris
            .connect_stop(clone!(@strong self.sender as sender => move |_| {
                if let Err(e) = sender.send_blocking(PlaybackAction::Stop) {
                    error!("Unable to send Stop: {e}");
                }
            }));

        self.mpris
            .connect_pause(clone!(@strong self.sender as sender => move |_| {
                if let Err(e) = sender.send_blocking(PlaybackAction::Pause) {
                    error!("Unable to send Pause: {e}");
                }
            }));

        self.mpris
            .connect_previous(clone!(@strong self.sender as sender => move |_| {
                if let Err(e) = sender.send_blocking(PlaybackAction::SkipPrevious) {
                    error!("Unable to send SkipPrevious: {e}");
                }
            }));

        self.mpris
            .connect_next(clone!(@strong self.sender as sender => move |_| {
                if let Err(e) = sender.send_blocking(PlaybackAction::SkipNext) {
                    error!("Unable to send SkipNext: {e}");
                }
            }));

        self.mpris
            .connect_raise(clone!(@strong self.sender as sender => move |_| {
                if let Err(e) = sender.send_blocking(PlaybackAction::Raise) {
                    error!("Unable to send Raise: {e}");
                }
            }));

        self.mpris.connect_set_loop_status(
            clone!(@strong self.sender as sender => move |_, status| {
                let mode = match status {
                    LoopStatus::None => RepeatMode::Consecutive,
                    LoopStatus::Track => RepeatMode::RepeatOne,
                    LoopStatus::Playlist => RepeatMode::RepeatAll,
                };

                if let Err(e) = sender.send_blocking(PlaybackAction::Repeat(mode)) {
                    error!("Unable to send Repeat({mode}): {e}");
                }
            }),
        );

        self.mpris
            .connect_seek(clone!(@strong self.sender as sender => move |_, position| {
                let pos = position.as_secs().unsigned_abs();

                if let Err(e) = sender.send_blocking(PlaybackAction::Seek(pos)) {
                    error!("Unable to send Seek({pos}): {e}");
                }
            }));
    }

    fn update_metadata(&self) {
        let mut metadata = Metadata::new();

        if let Some(song) = self.song.take() {
            metadata.set_artist(Some(vec![song.artist()]));
            metadata.set_title(Some(song.title()));
            metadata.set_album(Some(song.album()));

            let length = Time::from_secs(song.duration() as i64);
            metadata.set_length(Some(length));

            // MPRIS should really support passing a bytes buffer for
            // the cover art, instead of requiring this ridiculous
            // charade
            if let Some(cache) = song.cover_cache() {
                let file = gio::File::for_path(cache);
                match file.query_info(
                    "standard::type",
                    gio::FileQueryInfoFlags::NONE,
                    gio::Cancellable::NONE,
                ) {
                    Ok(info) if info.file_type() == gio::FileType::Regular => {
                        metadata.set_art_url(Some(file.uri()));
                    }
                    _ => metadata.set_art_url(None::<String>),
                }
            }

            self.song.replace(Some(song));
        }

        glib::spawn_future_local(clone!(@weak self.mpris as mpris => async move {
            if let Err(err) = mpris.set_metadata(metadata).await {
                error!("Unable to set MPRIS metadata: {err:?}");
            }
        }));
    }
}

impl Controller for MprisController {
    fn set_playback_state(&self, state: &PlaybackState) {
        let status = match state {
            PlaybackState::Playing => PlaybackStatus::Playing,
            PlaybackState::Paused => PlaybackStatus::Paused,
            _ => PlaybackStatus::Stopped,
        };

        glib::spawn_future_local(clone!(@weak self.mpris as mpris => async move {
            if let Err(err) = mpris.set_can_play(true).await {
                error!("Unable to set MPRIS play capability: {err:?}");
            }
            if let Err(err) = mpris.set_playback_status(status).await {
                error!("Unable to set MPRIS playback status: {err:?}");
            }
        }));
    }

    fn set_song(&self, song: &Song) {
        self.song.replace(Some(song.clone()));
        self.update_metadata();
    }

    fn set_position(&self, position: u64) {
        let pos = Time::from_secs(position as i64);
        self.mpris.set_position(pos);
    }

    fn set_repeat_mode(&self, repeat: RepeatMode) {
        let status = match repeat {
            RepeatMode::Consecutive => LoopStatus::None,
            RepeatMode::RepeatOne => LoopStatus::Track,
            RepeatMode::RepeatAll => LoopStatus::Playlist,
        };

        glib::spawn_future_local(clone!(@weak self.mpris as mpris => async move {
            if let Err(err) = mpris.set_loop_status(status).await {
                error!("Unable to set MPRIS loop status: {err:?}");
            }
        }));
    }
}
