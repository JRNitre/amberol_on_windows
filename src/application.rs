// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{cell::RefCell, rc::Rc};

use adw::subclass::prelude::*;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use ashpd::{desktop::background::Background, WindowIdentifier};
use async_channel::Receiver;
use glib::clone;
use gtk::{gio, gio::ApplicationHoldGuard, glib, prelude::*};
use log::{debug, warn};

use crate::{
    audio::AudioPlayer,
    config::{APPLICATION_ID, VERSION},
    i18n::i18n,
    utils,
    window::Window,
};

pub enum ApplicationAction {
    Present,
}

#[cfg(target_os = "windows")]
const WINDOWS_PORT_GITHUB_URL: Option<&str> = None;

mod imp {
    use super::*;

    #[derive(Debug)]
    pub struct Application {
        pub player: Rc<AudioPlayer>,
        pub receiver: RefCell<Option<Receiver<ApplicationAction>>>,
        pub background_hold: RefCell<Option<ApplicationHoldGuard>>,
        pub settings: gio::Settings,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Application {
        const NAME: &'static str = "AmberolApplication";
        type Type = super::Application;
        type ParentType = adw::Application;

        fn new() -> Self {
            let (sender, r) = async_channel::unbounded();
            let receiver = RefCell::new(Some(r));

            Self {
                player: AudioPlayer::new(sender),
                receiver,
                background_hold: RefCell::default(),
                settings: utils::settings_manager(),
            }
        }
    }

    impl ObjectImpl for Application {
        fn constructed(&self) {
            self.parent_constructed();

            let obj = self.obj();
            obj.setup_channel();
            obj.setup_gactions();
            obj.setup_settings();

            obj.set_accels_for_action("app.quit", &["<primary>q"]);

            obj.set_accels_for_action("queue.add-song", &["<primary>s"]);
            obj.set_accels_for_action("queue.add-folder", &["<primary>a"]);
            obj.set_accels_for_action("queue.clear", &["<primary>L"]);
            obj.set_accels_for_action("queue.toggle", &["F9"]);
            obj.set_accels_for_action("queue.search", &["<primary>F"]);
            obj.set_accels_for_action("queue.shuffle", &["<primary>r"]);

            obj.set_accels_for_action("win.seek-backwards", &["<primary>Left"]);
            obj.set_accels_for_action("win.seek-forward", &["<primary>Right"]);
            obj.set_accels_for_action("win.previous", &["<primary>b"]);
            obj.set_accels_for_action("win.next", &["<primary>n"]);
            obj.set_accels_for_action("win.play", &["<primary>p"]);
            obj.set_accels_for_action("win.copy", &["<primary>c"]);
            obj.set_accels_for_action("win.mini", &["<primary>m"]);
            obj.set_accels_for_action("win.show-lyrics", &["<primary>y"]);
            obj.set_accels_for_action("win.favorite", &["<primary>d"]);
            obj.set_accels_for_action("win.topmost", &["<primary>t"]);
        }
    }

    impl ApplicationImpl for Application {
        fn startup(&self) {
            self.parent_startup();

            #[cfg(target_os = "windows")]
            if let Ok(executable) = std::env::current_exe() {
                if let Some(bin_dir) = executable.parent() {
                    let local_icons = bin_dir.join("share").join("icons");
                    let icons_dir = if local_icons.is_dir() {
                        local_icons
                    } else {
                        bin_dir.join("..").join("share").join("icons")
                    };
                    if icons_dir.join("hicolor").join("index.theme").is_file() {
                        if let Some(display) = gtk::gdk::Display::default() {
                            let icon_theme = gtk::IconTheme::for_display(&display);
                            icon_theme.add_search_path(&icons_dir);
                            debug!(
                                "Bundled Amberol icon available: {}",
                                icon_theme.has_icon(APPLICATION_ID)
                            );
                        }
                    }
                }
            }

            gtk::Window::set_default_icon_name(APPLICATION_ID);
        }

        fn activate(&self) {
            debug!("Application::activate");

            self.obj().present_main_window();
        }

        fn shutdown(&self) {
            crate::session::save(&self.player);
            self.player.stop();
            self.parent_shutdown();
        }

        fn open(&self, files: &[gio::File], _hint: &str) {
            debug!("Application::open");

            let application = self.obj();
            application.present_main_window();
            if let Some(window) = application.active_window() {
                window.downcast_ref::<Window>().unwrap().open_files(files);
            }
        }
    }

    impl GtkApplicationImpl for Application {}
    impl AdwApplicationImpl for Application {}
}

glib::wrapper! {
    pub struct Application(ObjectSubclass<imp::Application>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl Default for Application {
    fn default() -> Self {
        glib::Object::builder::<Application>()
            .property("application-id", APPLICATION_ID)
            .property("flags", gio::ApplicationFlags::HANDLES_OPEN)
            .property("resource-base-path", "/io/bassi/Amberol")
            .build()
    }
}

impl Application {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn player(&self) -> Rc<AudioPlayer> {
        self.imp().player.clone()
    }

    fn setup_settings(&self) {
        self.imp().settings.connect_changed(
            Some("background-play"),
            clone!(@weak self as this => move |settings, _| {
                let background_play = settings.boolean("background-play");
                debug!("GSettings:background-play: {background_play}");
                if background_play {
                    this.request_background();
                } else {
                    debug!("Dropping background hold");
                    this.imp().background_hold.replace(None);
                }
            }),
        );

        let _dummy = self.imp().settings.boolean("background-play");
    }

    fn setup_channel(&self) {
        let receiver = self.imp().receiver.borrow_mut().take().unwrap();
        glib::MainContext::default().spawn_local(clone!(@strong self as this => async move {
            use futures::prelude::*;

            let mut receiver = std::pin::pin!(receiver);

            while let Some(action) = receiver.next().await {
                this.process_action(action);
            }
        }));
    }

    fn process_action(&self, action: ApplicationAction) -> glib::ControlFlow {
        match action {
            ApplicationAction::Present => self.present_main_window(),
            // _ => debug!("Received action {:?}", action),
        }

        glib::ControlFlow::Continue
    }

    fn present_main_window(&self) {
        let window = if let Some(window) = self.windows().first() {
            window.clone()
        } else {
            let window = Window::new(self);
            window.upcast()
        };

        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        self.request_background();

        window.present();
        self.player().setup_media_controls(&window);
    }

    fn setup_gactions(&self) {
        self.add_action_entries([
            gio::ActionEntry::builder("quit")
                .activate(|app: &Application, _, _| {
                    crate::session::save(&app.player());
                    for window in app.windows() {
                        if let Some(window) = window.downcast_ref::<Window>() {
                            window.release_background();
                        }
                    }
                    app.quit();
                })
                .build(),
            gio::ActionEntry::builder("about")
                .activate(|app: &Application, _, _| {
                    app.show_about();
                })
                .build(),
        ]);

        let background_play = self.imp().settings.boolean("background-play");
        self.add_action_entries([gio::ActionEntry::builder("background-play")
            .state(background_play.to_variant())
            .activate(|this: &Application, action, _| {
                let state = action.state().unwrap();
                let action_state: bool = state.get().unwrap();
                let background_play = !action_state;
                action.set_state(&background_play.to_variant());

                this.imp()
                    .settings
                    .set_boolean("background-play", background_play)
                    .expect("Unable to store background-play setting");
            })
            .build()]);
    }

    fn show_about(&self) {
        let window = self.active_window().unwrap();
        let builder = adw::AboutWindow::builder()
            .transient_for(&window)
            .application_icon(APPLICATION_ID)
            .application_name("Amberol")
            .version(VERSION)
            .copyright("© 2022 Emmanuele Bassi")
            .license_type(gtk::License::Gpl30)
            // Translators: Replace "translator-credits" with your names, one name per line
            .translator_credits(i18n("translator-credits"));

        #[cfg(target_os = "windows")]
        let builder = builder
            .developer_name("JRNitre")
            .developers(vec!["JRNitre"])
            .comments("由 JRNitre 移植到 Windows 平台。\n原项目作者：Emmanuele Bassi。");

        #[cfg(not(target_os = "windows"))]
        let builder = builder
            .developer_name("Emmanuele Bassi")
            .developers(vec!["Emmanuele Bassi"])
            .website("https://apps.gnome.org/Amberol/")
            .issue_url("https://gitlab.gnome.org/World/amberol/-/issues/new");

        let dialog = builder.build();

        #[cfg(target_os = "windows")]
        {
            dialog.add_credit_section(Some("原项目作者"), &["Emmanuele Bassi"]);
            dialog.add_link("Amberol 原项目", "https://apps.gnome.org/Amberol/");
            if let Some(url) = WINDOWS_PORT_GITHUB_URL {
                dialog.add_link("Windows 版 GitHub", url);
            }
        }

        dialog.present();
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    async fn portal_request_background(&self) {
        if let Some(window) = self.active_window() {
            let root = window.native().unwrap();
            let identifier = WindowIdentifier::from_native(&root).await;
            let request = Background::request().identifier(identifier).reason(&*i18n(
                "Amberol needs to run in the background to play music",
            ));

            match request.send().await.and_then(|r| r.response()) {
                Ok(response) => {
                    debug!("Background request successful: {:?}", response);
                    self.imp().background_hold.replace(Some(self.hold()));
                }
                Err(err) => {
                    warn!("Background request denied: {}", err);
                    self.imp()
                        .settings
                        .set_boolean("background-play", false)
                        .expect("Unable to set background-play settings key");
                }
            }
        }
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn request_background(&self) {
        let background_play = self.imp().settings.boolean("background-play");
        if background_play {
            let ctx = glib::MainContext::default();
            ctx.spawn_local(clone!(@weak self as app => async move {
                app.portal_request_background().await
            }));
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    fn request_background(&self) {}
}
