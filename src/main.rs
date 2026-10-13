// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod application;
mod audio;
mod config;
mod cover_picture;
mod drag_overlay;
mod i18n;
mod playback_control;
mod playlist_view;
mod queue_row;
mod search;
mod song_cover;
mod song_details;
mod sort;
mod utils;
mod volume_control;
mod waveform_view;
mod window;

use std::env;

use config::{APPLICATION_ID, GETTEXT_PACKAGE, LOCALEDIR, PKGDATADIR, PROFILE};
use gettextrs::{bind_textdomain_codeset, bindtextdomain, setlocale, textdomain, LocaleCategory};
use gtk::{gio, glib, prelude::*};
use log::{debug, error, LevelFilter};

use self::application::Application;

#[cfg(target_os = "windows")]
fn configure_portable_runtime() {
    let Some(executable_dir) = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
    else {
        return;
    };
    let share_dir = executable_dir.join("share");
    if !share_dir.join("glib-2.0/schemas/gschemas.compiled").is_file() {
        return;
    }

    env::set_var("GSETTINGS_SCHEMA_DIR", share_dir.join("glib-2.0/schemas"));
    env::set_var(
        "GST_PLUGIN_SYSTEM_PATH_1_0",
        executable_dir.join("lib/gstreamer-1.0"),
    );
    env::set_var(
        "GDK_PIXBUF_MODULEDIR",
        executable_dir.join("lib/gdk-pixbuf-2.0/2.10.0/loaders"),
    );
    if env::var_os("LANG").is_none() {
        env::set_var("LANG", "zh_CN.UTF-8");
    }
    if env::var_os("LC_ALL").is_none() {
        env::set_var("LC_ALL", "zh_CN.UTF-8");
    }
}

fn main() -> glib::ExitCode {
    #[cfg(target_os = "windows")]
    configure_portable_runtime();

    let mut builder = pretty_env_logger::formatted_builder();
    if APPLICATION_ID.ends_with("Devel") {
        builder.filter(Some("amberol"), LevelFilter::Debug);
    } else {
        builder.filter(Some("amberol"), LevelFilter::Info);
    }
    builder.init();

    // Set up gettext translations
    debug!("Setting up locale data");
    setlocale(LocaleCategory::LcAll, "");

    let locale_dir = env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent().map(|dir| {
                let beside = dir.join("share").join("locale");
                if beside.is_dir() {
                    beside
                } else {
                    dir.join("..").join("share").join("locale")
                }
            })
        })
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| std::path::PathBuf::from(LOCALEDIR));
    bindtextdomain(GETTEXT_PACKAGE, &locale_dir).expect("Unable to bind the text domain");
    bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8")
        .expect("Unable to set the text domain encoding");
    textdomain(GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    debug!("Setting up pulseaudio environment");
    let app_id = APPLICATION_ID.trim_end_matches(".Devel");
    env::set_var("PULSE_PROP_application.icon_name", app_id);
    env::set_var("PULSE_PROP_application.metadata().name", "Amberol");
    env::set_var("PULSE_PROP_media.role", "music");

    debug!("Loading resources");
    let installed_resource = std::path::Path::new(PKGDATADIR).join("amberol.gresource");
    let executable_dir = match env::current_exe() {
        Ok(path) => path.parent().map(std::path::Path::to_path_buf),
        Err(err) => {
            error!("Unable to find the current path: {}", err);
            None
        }
    };
    let resources = executable_dir
        .as_ref()
        .map(|dir| dir.join("amberol.gresource"))
        .filter(|path| path.is_file())
        .or_else(|| {
            executable_dir.as_ref().map(|dir| {
                dir.join("..").join("share").join("amberol").join("amberol.gresource")
            }).filter(|path| path.is_file())
        })
        .or_else(|| installed_resource.is_file().then_some(installed_resource))
        .and_then(|path| gio::Resource::load(&path).ok());
    let Some(resources) = resources else {
        error!("Unable to find amberol.gresource beside the executable or in the install directory");
        return glib::ExitCode::FAILURE;
    };
    gio::resources_register(&resources);

    debug!("Setting up application (profile: {})", &PROFILE);
    glib::set_application_name("Amberol");
    glib::set_program_name(Some("amberol"));

    gst::init().expect("Failed to initialize gstreamer");

    let ctx = glib::MainContext::default();
    let _guard = ctx.acquire().unwrap();

    Application::new().run()
}
