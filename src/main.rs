// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

mod application;
mod audio;
mod config;
mod cover_picture;
mod drag_overlay;
mod i18n;
mod playback_control;
mod playlist_view;
mod queue_row;
mod song_details;
mod utils;
mod volume_control;
mod waveform_view;
mod window;

use std::env;

use config::{APPLICATION_ID, GETTEXT_PACKAGE, LOCALEDIR, PKGDATADIR, PROFILE};
use gettextrs::{bind_textdomain_codeset, bindtextdomain, setlocale, textdomain, LocaleCategory};
use gtk::{gio, glib, prelude::*};
use log::{debug, error};

use self::application::Application;

fn main() {
    pretty_env_logger::init();

    // Set up gettext translations
    debug!("Setting up locale data");
    setlocale(LocaleCategory::LcAll, "");

    bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR).expect("Unable to bind the text domain");
    bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8")
        .expect("Unable to set the text domain encoding");
    textdomain(GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    debug!("Setting up pulseaudio environment");
    let app_id = APPLICATION_ID.trim_end_matches(".Devel");
    env::set_var("PULSE_PROP_application.icon_name", &app_id);
    env::set_var("PULSE_PROP_application.metadata().name", "Amberol");
    env::set_var("PULSE_PROP_media.role", "music");

    debug!("Loading resources");
    let resources = match env::var("MESON_DEVENV") {
        Err(_) => gio::Resource::load(PKGDATADIR.to_owned() + "/amberol.gresource")
            .expect("Unable to find amberol.gresource"),
        Ok(_) => match env::current_exe() {
            Ok(path) => {
                let mut resource_path = path;
                resource_path.pop();
                resource_path.push("amberol.gresource");
                gio::Resource::load(&resource_path)
                    .expect("Unable to find amberol.gresource in devenv")
            }
            Err(err) => {
                error!("Unable to find the current path: {}", err);
                return;
            }
        },
    };
    gio::resources_register(&resources);

    debug!("Setting up application (profile: {})", &PROFILE);
    glib::set_application_name("Amberol");
    glib::set_program_name(Some("amberol"));

    gst::init().expect("Failed to initialize gstreamer");

    // We need to acquire the default context before starting the
    // application, as we create the AudioPlayer instance when we
    // create the Application, and the Application will need to
    // create a glib::MainContext::channel() for it
    let ctx = glib::MainContext::default();
    let _guard = ctx.acquire().unwrap();

    let app = Application::new();
    std::process::exit(app.run());
}
