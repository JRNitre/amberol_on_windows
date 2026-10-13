// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    convert::TryFrom,
    rc::Rc,
    time::Instant,
};

use adw::prelude::{MessageDialogExt, MessageDialogExtManual};
use adw::subclass::prelude::*;
use glib::{clone, closure_local};
use gtk::{gdk, gio, glib, prelude::*, CompositeTemplate};
use log::debug;

use crate::{
    audio::{AudioPlayer, RepeatMode, ReplayGainMode, Song},
    config::APPLICATION_ID,
    drag_overlay::DragOverlay,
    i18n::{i18n, i18n_k, ni18n_f, ni18n_k},
    playback_control::PlaybackControl,
    playlist_view::PlaylistView,
    queue_row::QueueRow,
    search::FuzzyFilter,
    song_cover::SongCover,
    song_details::SongDetails,
    sort::FuzzySorter,
    utils,
    volume_control::VolumeControl,
    waveform_view::WaveformView,
};

pub enum WindowMode {
    InitialView,
    MainView,
}

mod imp {
    use glib::{ParamSpec, ParamSpecBoolean, ParamSpecEnum, Value};
    use once_cell::sync::Lazy;

    use super::*;

    #[derive(CompositeTemplate)]
    #[template(resource = "/io/bassi/Amberol/window.ui")]
    pub struct Window {
        // Template widgets
        #[template_child]
        pub drag_overlay: TemplateChild<DragOverlay>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub main_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub status_page: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub song_cover: TemplateChild<SongCover>,
        #[template_child]
        pub song_details: TemplateChild<SongDetails>,
        #[template_child]
        pub waveform_view: TemplateChild<WaveformView>,
        #[template_child]
        pub elapsed_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub remaining_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub playback_control: TemplateChild<PlaybackControl>,
        #[template_child]
        pub split_view: TemplateChild<adw::OverlaySplitView>,
        #[template_child]
        pub playlist_view: TemplateChild<PlaylistView>,
        #[template_child]
        pub add_folder_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub restore_playlist_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub main_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub lyrics_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub lyric_current: TemplateChild<gtk::Label>,
        #[template_child]
        pub lyric_next: TemplateChild<gtk::Label>,
        #[template_child]
        pub lyric_offset: TemplateChild<gtk::SpinButton>,

        pub mini: Cell<bool>,
        pub full_size: Cell<(i32, i32)>,
        pub restoring: RefCell<Option<serde_json::Value>>,
        pub lyric_lines: RefCell<Vec<crate::lyrics::Line>>,
        pub background_hold: RefCell<Option<gio::ApplicationHoldGuard>>,
        #[cfg(target_os = "windows")]
        pub tray: RefCell<Option<crate::windows_integration::Tray>>,

        pub provider: gtk::CssProvider,
        pub settings: gio::Settings,

        pub playlist_shuffled: Cell<bool>,
        pub playlist_visible: Cell<bool>,
        pub playlist_selection: Cell<bool>,
        pub playlist_search: Cell<bool>,
        pub replaygain_mode: Cell<ReplayGainMode>,

        pub playlist_filtermodel: RefCell<Option<gio::ListModel>>,

        pub notify_playing_id: RefCell<Option<glib::SignalHandlerId>>,
        pub notify_position_id: RefCell<Option<glib::SignalHandlerId>>,
        pub notify_song_id: RefCell<Option<glib::SignalHandlerId>>,
        pub notify_cover_id: RefCell<Option<glib::SignalHandlerId>>,
        pub notify_nsongs_id: RefCell<Option<glib::SignalHandlerId>>,
        pub notify_current_id: RefCell<Option<glib::SignalHandlerId>>,
        pub notify_peaks_id: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "AmberolWindow";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);

            klass.install_action("win.play", None, move |win, _, _| {
                debug!("Window::win.play()");
                if let Some(p) = win.player() {
                    p.toggle_play();
                }
            });
            klass.install_action("win.seek-backwards", None, move |win, _, _| {
                debug!("Window::win.seek-backwards");
                if let Some(p) = win.player() {
                    p.seek_backwards();
                }
            });
            klass.install_action("win.seek-forward", None, move |win, _, _| {
                debug!("Window::win.seek-forward");
                if let Some(p) = win.player() {
                    p.seek_forward();
                }
            });
            klass.install_action("win.previous", None, move |win, _, _| {
                debug!("Window::win.previous()");
                if let Some(p) = win.player() {
                    p.skip_previous();
                }
            });
            klass.install_action("win.next", None, move |win, _, _| {
                debug!("Window::win.next()");
                if let Some(p) = win.player() {
                    p.skip_next();
                }
            });
            klass.install_action("queue.repeat-mode", None, move |win, _, _| {
                debug!("Window::queue.repeat()");
                if let Some(p) = win.player() {
                    p.toggle_repeat_mode();
                }
            });
            klass.install_action("queue.add-song", None, move |win, _, _| {
                debug!("Window::win.add-song()");
                win.add_song();
            });
            klass.install_action("queue.add-folder", None, move |win, _, _| {
                debug!("Window::win.add-folder()");
                win.add_folder();
            });
            klass.install_action("queue.restore-playlist", None, move |win, _, _| {
                debug!("Window::queue.restore-playlist()");
                win.restore_playlist();
            });
            klass.install_action("win.copy", None, move |win, _, _| {
                debug!("Window::win.copy()");
                win.copy_song();
            });
            klass.install_action("queue.clear", None, move |win, _, _| {
                debug!("Window::queue.clear()");
                win.clear_queue();
            });
            klass.install_property_action("queue.toggle", "playlist-visible");
            klass.install_property_action("queue.shuffle", "playlist-shuffled");
            klass.install_property_action("queue.select", "playlist-selection");
            klass.install_property_action("queue.search", "playlist-search");
            klass.install_property_action("win.replaygain", "replaygain-mode");

            klass.install_action(
                "win.skip-to",
                Some(glib::VariantTy::UINT32),
                move |win, _, param| {
                    if let Some(pos) = param.and_then(u32::from_variant) {
                        if let Some(player) = win.player() {
                            player.skip_to(pos);
                            player.play();
                        }
                    }
                },
            );
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }

        fn new() -> Self {
            Self {
                song_details: TemplateChild::default(),
                song_cover: TemplateChild::default(),
                split_view: TemplateChild::default(),
                toast_overlay: TemplateChild::default(),
                drag_overlay: TemplateChild::default(),
                playback_control: TemplateChild::default(),
                waveform_view: TemplateChild::default(),
                elapsed_label: TemplateChild::default(),
                remaining_label: TemplateChild::default(),
                main_stack: TemplateChild::default(),
                status_page: TemplateChild::default(),
                add_folder_button: TemplateChild::default(),
                restore_playlist_button: TemplateChild::default(),
                main_box: TemplateChild::default(),
                lyrics_box: TemplateChild::default(),
                lyric_current: TemplateChild::default(),
                lyric_next: TemplateChild::default(),
                lyric_offset: TemplateChild::default(),
                mini: Cell::new(false),
                full_size: Cell::new((600, 700)),
                restoring: RefCell::new(None),
                lyric_lines: RefCell::new(Vec::new()),
                background_hold: RefCell::new(None),
                #[cfg(target_os = "windows")]
                tray: RefCell::new(None),
                playlist_view: TemplateChild::default(),
                playlist_shuffled: Cell::new(false),
                playlist_visible: Cell::new(true),
                playlist_selection: Cell::new(false),
                playlist_search: Cell::new(false),
                playlist_filtermodel: RefCell::default(),
                replaygain_mode: Cell::new(ReplayGainMode::default()),
                provider: gtk::CssProvider::new(),
                settings: utils::settings_manager(),
                notify_playing_id: RefCell::new(None),
                notify_position_id: RefCell::new(None),
                notify_song_id: RefCell::new(None),
                notify_cover_id: RefCell::new(None),
                notify_nsongs_id: RefCell::new(None),
                notify_current_id: RefCell::new(None),
                notify_peaks_id: RefCell::new(None),
            }
        }
    }

    impl ObjectImpl for Window {
        fn constructed(&self) {
            self.parent_constructed();

            if APPLICATION_ID.ends_with("Devel") {
                self.obj().add_css_class("devel");
            }
        }

        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![
                    ParamSpecBoolean::builder("playlist-shuffled").build(),
                    ParamSpecBoolean::builder("playlist-visible").build(),
                    ParamSpecBoolean::builder("playlist-selection").build(),
                    ParamSpecBoolean::builder("playlist-search").build(),
                    ParamSpecEnum::builder::<ReplayGainMode>("replaygain-mode").build(),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, value: &Value, pspec: &ParamSpec) {
            let obj = self.obj();
            match pspec.name() {
                "playlist-shuffled" => obj.set_playlist_shuffled(value.get::<bool>().unwrap()),
                "playlist-visible" => obj.set_playlist_visible(value.get::<bool>().unwrap()),
                "playlist-selection" => obj.set_playlist_selection(value.get::<bool>().unwrap()),
                "playlist-search" => obj.set_playlist_search(value.get::<bool>().unwrap()),
                "replaygain-mode" => obj.set_replaygain(value.get::<ReplayGainMode>().unwrap()),
                _ => unimplemented!(),
            }
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> Value {
            let obj = self.obj();
            match pspec.name() {
                "playlist-shuffled" => obj.playlist_shuffled().to_value(),
                "playlist-visible" => obj.playlist_visible().to_value(),
                "playlist-selection" => obj.playlist_selection().to_value(),
                "playlist-search" => obj.playlist_search().to_value(),
                "replaygain-mode" => obj.replaygain().to_value(),
                _ => unimplemented!(),
            }
        }
    }

    impl WidgetImpl for Window {}
    impl WindowImpl for Window {}
    impl ApplicationWindowImpl for Window {}
    impl AdwApplicationWindowImpl for Window {}
}

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl Window {
    pub fn new<P: IsA<gtk::Application>>(application: &P) -> Self {
        let win = glib::Object::builder::<Window>()
            .property("application", application)
            .build();

        win.setup_waveform();
        win.setup_actions();
        win.setup_playlist();
        win.setup_drop_target();
        win.setup_provider();
        win.bind_state();
        win.bind_queue();
        win.connect_signals();
        win.restore_window_state();
        win.set_initial_state();
        win.setup_extensions();

        win
    }

    fn player(&self) -> Option<Rc<AudioPlayer>> {
        if let Some(app) = self.application() {
            let player = app
                .downcast::<crate::application::Application>()
                .unwrap()
                .player();
            return Some(player);
        }

        None
    }

    fn setup_extensions(&self) {
        self.add_css_class("amberol-window");
        for key in [
            "glass-background",
            "restore-session",
            "resume-playing",
            "show-lyrics",
            "fade-playback",
        ] {
            let key = key.to_owned();
            let setting = key.clone();
            self.add_action_entries([gio::ActionEntry::builder(&key)
                .state(self.imp().settings.boolean(&key).to_variant())
                .activate(move |win: &Window, action, _| {
                    let enabled = !action
                        .state()
                        .and_then(|s| s.get::<bool>())
                        .unwrap_or(false);
                    action.set_state(&enabled.to_variant());
                    let _ = win.imp().settings.set_boolean(&setting, enabled);
                    win.refresh_glass();
                    win.imp().lyrics_box.set_visible(
                        win.imp().settings.boolean("show-lyrics") && !win.imp().mini.get(),
                    );
                    win.fit_lyrics();
                })
                .build()]);
        }
        self.add_action_entries([
            gio::ActionEntry::builder("mini")
                .state(false.to_variant())
                .activate(|win: &Window, action, _| {
                    let mini = !win.imp().mini.get();
                    win.set_mini(mini);
                    action.set_state(&mini.to_variant());
                })
                .build(),
            gio::ActionEntry::builder("topmost")
                .state(false.to_variant())
                .activate(|win: &Window, action, _| {
                    let enabled = !action
                        .state()
                        .and_then(|s| s.get::<bool>())
                        .unwrap_or(false);
                    if crate::windows_integration::topmost(win, enabled) {
                        action.set_state(&enabled.to_variant());
                    } else {
                        win.add_toast("当前系统不支持窗口置顶".into());
                    }
                })
                .build(),
            gio::ActionEntry::builder("favorite")
                .activate(|win: &Window, _, _| {
                    win.toggle_favorite();
                })
                .build(),
            gio::ActionEntry::builder("favorites")
                .activate(|win: &Window, _, _| {
                    win.load_playlist_path(&crate::library::directory().join("favorites.m3u8"));
                })
                .build(),
            gio::ActionEntry::builder("save-playlist")
                .activate(|win: &Window, _, _| {
                    win.save_named_playlist();
                })
                .build(),
            gio::ActionEntry::builder("open-playlist")
                .activate(|win: &Window, _, _| {
                    win.choose_named_playlist();
                })
                .build(),
            gio::ActionEntry::builder("import-playlist")
                .activate(|win: &Window, _, _| {
                    win.playlist_dialog(false);
                })
                .build(),
            gio::ActionEntry::builder("export-playlist")
                .activate(|win: &Window, _, _| {
                    win.playlist_dialog(true);
                })
                .build(),
            gio::ActionEntry::builder("recent-folder")
                .activate(|win: &Window, _, _| {
                    win.open_recent_folder();
                })
                .build(),
            gio::ActionEntry::builder("sleep")
                .parameter_type(Some(glib::VariantTy::UINT32))
                .activate(|win: &Window, _, value| {
                    if let (Some(player), Some(minutes)) =
                        (win.player(), value.and_then(u32::from_variant))
                    {
                        player.set_sleep_timer(minutes);
                        win.add_toast(if minutes == 0 {
                            "已取消睡眠定时".into()
                        } else {
                            format!("将在 {minutes} 分钟后暂停")
                        });
                    }
                })
                .build(),
            gio::ActionEntry::builder("loop-a")
                .activate(|win: &Window, _, _| {
                    if let Some(p) = win.player() {
                        p.set_loop_a();
                        win.add_toast("已标记 A 点".into());
                    }
                })
                .build(),
            gio::ActionEntry::builder("loop-b")
                .activate(|win: &Window, _, _| {
                    if let Some(p) = win.player() {
                        win.add_toast(
                            if p.set_loop_b() {
                                "A–B 循环已开始"
                            } else {
                                "请先标记 A 点，然后在更晚的位置标记 B 点"
                            }
                            .into(),
                        );
                    }
                })
                .build(),
            gio::ActionEntry::builder("loop-clear")
                .activate(|win: &Window, _, _| {
                    if let Some(p) = win.player() {
                        p.clear_loop();
                        win.add_toast("已取消 A–B 循环".into());
                    }
                })
                .build(),
        ]);
        self.imp()
            .lyrics_box
            .set_visible(self.imp().settings.boolean("show-lyrics"));
        self.imp()
            .lyric_offset
            .connect_value_changed(clone!(@weak self as win => move |_| { win.update_lyrics(); }));
        self.connect_map(clone!(@weak self as win => move |_| {
            win.refresh_glass();
            win.fit_lyrics();
            #[cfg(target_os = "windows")]
            if win.imp().tray.borrow().is_none() { win.imp().tray.replace(crate::windows_integration::Tray::new(&win)); }
            win.imp().background_hold.replace(None);
        }));
        adw::StyleManager::default().connect_notify_local(
            None,
            clone!(@weak self as win => move |_, _| { win.refresh_glass(); }),
        );
        let mut last_save = Instant::now();
        glib::timeout_add_seconds_local(
            1,
            clone!(@weak self as win => @default-return glib::ControlFlow::Break, move || {
                if let Some(player) = win.player() {
                    player.check_sleep_timer();
                    if last_save.elapsed().as_secs() >= 10 && win.imp().restoring.borrow().is_none() {
                        crate::session::save(&player); last_save = Instant::now();
                    }
                }
                glib::ControlFlow::Continue
            }),
        );
        if self.imp().settings.boolean("restore-session") {
            glib::idle_add_local_once(clone!(@weak self as win => move || {
                if win.player().map(|p| p.queue().is_empty()).unwrap_or(false) { win.restore_playlist(); }
            }));
        }
    }

    pub fn release_background(&self) {
        self.imp().background_hold.replace(None);
        #[cfg(target_os = "windows")]
        self.imp().tray.replace(None);
    }

    fn fit_lyrics(&self) {
        if self.imp().settings.boolean("show-lyrics") && !self.imp().mini.get() {
            let (width, height) = self.default_size();
            if height < 860 {
                self.set_default_size(width, 860);
                glib::idle_add_local_once(clone!(@weak self as win => move || {
                    crate::windows_integration::resize(&win, width, 860);
                }));
            }
        }
    }

    fn refresh_glass(&self) {
        let enabled = self.imp().settings.boolean("glass-background")
            && !adw::StyleManager::default().is_high_contrast();
        let native = crate::windows_integration::backdrop(self, enabled) && enabled;
        for (class, active) in [("glass", enabled), ("native-glass", native)] {
            if active {
                self.add_css_class(class);
            } else {
                self.remove_css_class(class);
            }
        }
        if let Some(player) = self.player() {
            self.update_style(player.state().current_song().as_ref());
        }
    }

    fn set_mini(&self, mini: bool) {
        if mini == self.imp().mini.replace(mini) {
            return;
        }
        if mini {
            self.imp().full_size.set(self.default_size());
            self.unmaximize();
            self.set_size_request(360, 390);
            self.imp().main_stack.set_vhomogeneous(false);
            self.imp().main_stack.set_hhomogeneous(false);
            self.set_playlist_visible(false);
            self.imp().playlist_view.set_visible(false);
            self.imp()
                .song_cover
                .album_image()
                .set_cover_size(crate::cover_picture::CoverSize::Small);
            self.add_css_class("mini");
            self.imp().playback_control.set_margin_bottom(0);
            self.set_default_size(380, 390);
        } else {
            self.remove_css_class("mini");
            self.set_size_request(360, 740);
            self.imp().playlist_view.set_visible(true);
            self.imp()
                .song_cover
                .album_image()
                .set_cover_size(crate::cover_picture::CoverSize::Large);
            self.imp().playback_control.set_margin_bottom(24);
            let (w, h) = self.imp().full_size.get();
            self.set_default_size(w, h);
        }
        self.imp().song_details.album_label().set_visible(!mini);
        self.imp()
            .playback_control
            .volume_control()
            .set_visible(!mini);
        self.imp()
            .lyrics_box
            .set_visible(!mini && self.imp().settings.boolean("show-lyrics"));
        let size = if mini {
            (380, 390)
        } else {
            self.imp().full_size.get()
        };
        glib::idle_add_local_once(clone!(@weak self as win => move || {
            crate::windows_integration::resize(&win, size.0, size.1);
        }));
    }

    fn finish_restore(&self) {
        let Some(data) = self.imp().restoring.take() else {
            return;
        };
        let Some(player) = self.player() else {
            return;
        };
        let queue = player.queue();
        let mode = match data["repeat"].as_str() {
            Some("repeat-all") => RepeatMode::RepeatAll,
            Some("repeat-one") => RepeatMode::RepeatOne,
            _ => RepeatMode::Consecutive,
        };
        queue.set_repeat_mode(mode);
        if let Some(order) = data["shuffle"].as_array() {
            let order: Option<Vec<u32>> = order
                .iter()
                .map(|v| v.as_u64().and_then(|i| u32::try_from(i).ok()))
                .collect();
            if let Some(order) = order {
                if queue.restore_shuffle(&order) || {
                    queue.set_shuffled(true);
                    queue.is_shuffled()
                } {
                    self.imp().playlist_shuffled.set(true);
                    self.notify("playlist-shuffled");
                }
            }
        }
        let current = data["current"].as_str();
        if let Some(index) = (0..queue.n_songs()).find(|&i| {
            queue
                .song_at(i)
                .map(|s| Some(s.uri().as_str()) == current)
                .unwrap_or(false)
        }) {
            player.skip_to(index);
            player.restore_position(data["position"].as_u64().unwrap_or(0));
        }
        player.set_volume(
            data["volume"]
                .as_f64()
                .filter(|v| v.is_finite())
                .unwrap_or(1.0)
                .clamp(0.0, 1.0),
        );
        if self.imp().settings.boolean("resume-playing") {
            player.play();
        }
        self.add_toast("已恢复上次的歌曲与播放进度".into());
    }

    fn load_lyrics(&self) {
        self.imp().lyric_offset.set_value(0.0);
        let lines = self
            .player()
            .and_then(|p| p.state().current_song())
            .and_then(|s| s.file().path())
            .and_then(|p| std::fs::read_to_string(p.with_extension("lrc")).ok())
            .map(|s| crate::lyrics::parse(&s))
            .unwrap_or_default();
        self.imp().lyric_lines.replace(lines);
        self.update_lyrics();
    }

    fn update_lyrics(&self) {
        let position = self
            .player()
            .map(|p| p.state().position_ms() as i64)
            .unwrap_or(0);
        let lines = self.imp().lyric_lines.borrow();
        if lines.is_empty() {
            self.imp().lyric_current.set_label("未找到同名 LRC 歌词");
            self.imp().lyric_next.set_label("");
            return;
        }
        let index =
            crate::lyrics::current(&lines, position, self.imp().lyric_offset.value() as i64);
        self.imp()
            .lyric_current
            .set_label(index.map(|i| lines[i].text.as_str()).unwrap_or("♪"));
        self.imp().lyric_next.set_label(
            lines
                .get(index.map(|i| i + 1).unwrap_or(0))
                .map(|l| l.text.as_str())
                .unwrap_or(""),
        );
    }

    fn queue_uris(&self) -> Vec<String> {
        self.player()
            .map(|p| {
                (0..p.queue().n_songs())
                    .filter_map(|i| p.queue().song_at(i))
                    .map(|s| s.uri())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn load_playlist_path(&self, path: &std::path::Path) {
        match crate::library::read_m3u(path) {
            Ok(files) if !files.is_empty() => self.queue_songs(files),
            Ok(_) => self.add_toast("歌单中没有可访问的本地歌曲".into()),
            Err(err) => self.add_toast(format!("无法读取歌单：{err}")),
        }
    }

    fn toggle_favorite(&self) {
        let Some(song) = self.player().and_then(|p| p.state().current_song()) else {
            return;
        };
        let dir = crate::library::directory();
        let path = dir.join("favorites.m3u8");
        let mut uris: Vec<_> = crate::library::read_m3u(&path)
            .unwrap_or_default()
            .iter()
            .map(|f| f.uri().to_string())
            .collect();
        let uri = song.uri();
        let removed = uris.contains(&uri);
        if removed {
            uris.retain(|u| u != &uri);
        } else {
            uris.push(uri);
        }
        let result =
            std::fs::create_dir_all(dir).and_then(|_| crate::library::write_m3u(&path, &uris));
        self.add_toast(match result {
            Ok(_) => if removed {
                "已取消收藏"
            } else {
                "已收藏当前歌曲"
            }
            .into(),
            Err(e) => format!("保存收藏失败：{e}"),
        });
    }

    fn save_named_playlist(&self) {
        if self.queue_uris().is_empty() {
            self.add_toast("请先添加歌曲".into());
            return;
        }
        let dialog = adw::MessageDialog::new(
            Some(self),
            Some("保存歌单"),
            Some("为当前播放队列取一个名字"),
        );
        let entry = gtk::Entry::builder()
            .placeholder_text("例如：夜晚")
            .activates_default(true)
            .build();
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", "取消"), ("save", "保存")]);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");
        dialog.connect_response(None, clone!(@weak self as win => move |dialog, response| {
            if response == "save" {
                let Some(path) = crate::library::named_path(&entry.text()) else { win.add_toast("歌单名称为空或包含不支持的字符".into()); return; };
                if path.exists() { win.add_toast("已有同名歌单，请换一个名字".into()); return; }
                let result = std::fs::create_dir_all(crate::library::directory()).and_then(|_| crate::library::write_m3u(&path, &win.queue_uris()));
                win.add_toast(match result { Ok(_) => "歌单已保存".into(), Err(e) => format!("保存失败：{e}") });
            }
            dialog.close();
        }));
        dialog.present();
    }

    fn choose_named_playlist(&self) {
        let lists = crate::library::playlists();
        if lists.is_empty() {
            self.add_toast("还没有保存的歌单".into());
            return;
        }
        let dialog =
            adw::MessageDialog::new(Some(self), Some("打开歌单"), Some("歌曲将加入当前队列"));
        let names: Vec<_> = lists.iter().map(|(name, _)| name.as_str()).collect();
        let chooser = gtk::DropDown::from_strings(&names);
        dialog.set_extra_child(Some(&chooser));
        dialog.add_responses(&[("cancel", "取消"), ("open", "打开")]);
        dialog.set_default_response(Some("open"));
        dialog.set_close_response("cancel");
        dialog.connect_response(None, clone!(@weak self as win => move |dialog, response| {
            if response == "open" { if let Some((_, path)) = lists.get(chooser.selected() as usize) { win.load_playlist_path(path); } }
            dialog.close();
        }));
        dialog.present();
    }

    fn playlist_dialog(&self, export: bool) {
        let uris = self.queue_uris();
        if export && uris.is_empty() {
            self.add_toast("请先添加歌曲".into());
            return;
        }
        glib::MainContext::default().spawn_local(clone!(@weak self as win => async move {
            let filter = gtk::FileFilter::new(); filter.set_name(Some("M3U / M3U8")); filter.add_pattern("*.m3u"); filter.add_pattern("*.m3u8");
            let filters = gio::ListStore::new::<gtk::FileFilter>(); filters.append(&filter);
            let dialog = gtk::FileDialog::builder().title(if export { "导出歌单" } else { "导入歌单" }).filters(&filters).build();
            let file = if export { dialog.set_initial_name(Some("Amberol.m3u8")); dialog.save_future(Some(&win)).await } else { dialog.open_future(Some(&win)).await };
            if let Ok(file) = file { if let Some(path) = file.path() {
                if export { win.add_toast(match crate::library::write_m3u(&path, &uris) { Ok(_) => "歌单已导出".into(), Err(e) => format!("导出失败：{e}") }); }
                else { win.load_playlist_path(&path); }
            } }
        }));
    }

    fn open_recent_folder(&self) {
        let folders: Vec<String> = self
            .imp()
            .settings
            .strv("recent-folders")
            .iter()
            .map(|s| s.to_string())
            .filter(|p| std::path::Path::new(p).is_dir())
            .collect();
        if folders.is_empty() {
            self.add_toast("还没有最近打开的文件夹".into());
            return;
        }
        let dialog = adw::MessageDialog::new(
            Some(self),
            Some("最近打开的文件夹"),
            Some("歌曲将加入当前队列"),
        );
        let names: Vec<_> = folders.iter().map(|s| s.as_str()).collect();
        let chooser = gtk::DropDown::from_strings(&names);
        dialog.set_extra_child(Some(&chooser));
        dialog.add_responses(&[("cancel", "取消"), ("open", "打开")]);
        dialog.set_default_response(Some("open"));
        dialog.set_close_response("cancel");
        dialog.connect_response(None, clone!(@weak self as win => move |dialog, response| {
            if response == "open" { if let Some(path) = folders.get(chooser.selected() as usize) {
                win.queue_songs(utils::load_files_from_folder(&gio::File::for_path(path), true));
            } }
            dialog.close();
        }));
        dialog.present();
    }

    fn setup_actions(&self) {
        let enable_recoloring = self.imp().settings.boolean("enable-recoloring");
        self.add_action_entries([gio::ActionEntry::builder("enable-recoloring")
            .state(enable_recoloring.to_variant())
            .activate(|this: &Window, action, _| {
                let state = action.state().unwrap();
                let action_state: bool = state.get().unwrap();
                let enable_recoloring = !action_state;
                action.set_state(&enable_recoloring.to_variant());

                this.imp()
                    .settings
                    .set_boolean("enable-recoloring", enable_recoloring)
                    .expect("Unable to store setting");
            })
            .build()])
    }

    fn setup_waveform(&self) {
        if let Some(player) = self.player() {
            let peaks = player.waveform_generator().peaks();
            self.imp().waveform_view.set_peaks(peaks);

            let notify_peaks_id = player.waveform_generator().connect_notify_local(
                Some("has-peaks"),
                clone!(@weak self as win => move |gen, _| {
                    let peaks = gen.peaks();
                    win.imp().waveform_view.set_peaks(peaks);
                }),
            );
            self.imp().notify_peaks_id.replace(Some(notify_peaks_id));
        }
    }

    fn unbind_waveform(&self) {
        if let Some(player) = self.player() {
            if let Some(id) = self.imp().notify_peaks_id.take() {
                player.waveform_generator().disconnect(id);
            }
        }
    }

    fn restore_window_state(&self) {
        let settings = utils::settings_manager();
        let width = settings.int("window-width");
        let height = settings.int("window-height");
        self.set_default_size(width.max(360), height.max(740));
    }

    fn reset_queue(&self) {
        self.set_playlist_visible(false);
        self.set_playlist_shuffled(false);
        self.set_playlist_selection(false);
        self.update_style(None);
    }

    fn clear_queue(&self) {
        if let Some(p) = self.player() {
            p.clear_queue();
            utils::store_playlist(p.queue());
            crate::session::save(&p);
        }
    }

    fn playlist_visible(&self) -> bool {
        self.imp().playlist_visible.get()
    }

    fn set_playlist_visible(&self, visible: bool) {
        if visible != self.imp().playlist_visible.replace(visible) {
            self.imp().split_view.set_show_sidebar(visible);
            self.notify("playlist-visible");
        }
    }

    fn playlist_shuffled(&self) -> bool {
        self.imp().playlist_shuffled.get()
    }

    fn set_playlist_shuffled(&self, shuffled: bool) {
        let imp = self.imp();

        if shuffled != imp.playlist_shuffled.replace(shuffled) {
            if let Some(player) = self.player() {
                let queue = player.queue();
                let state = player.state();
                let reset_song = queue.is_first_song() && !state.playing();

                queue.set_shuffled(shuffled);

                if reset_song {
                    player.skip_to(0);
                }
            }

            self.notify("playlist-shuffled");
        }
    }

    fn playlist_selection(&self) -> bool {
        self.imp().playlist_selection.get()
    }

    fn set_playlist_selection(&self, selection: bool) {
        let imp = self.imp();

        if selection != imp.playlist_selection.replace(selection) {
            if !selection {
                if let Some(player) = self.player() {
                    let queue = player.queue();
                    queue.unselect_all_songs();
                }
            }

            self.imp()
                .playlist_view
                .queue_actionbar()
                .set_revealed(selection);

            self.notify("playlist-selection");
        }
    }

    fn playlist_search(&self) -> bool {
        self.imp().playlist_search.get()
    }

    fn set_playlist_search(&self, search: bool) {
        let imp = self.imp();

        if search != imp.playlist_search.replace(search) {
            imp.playlist_view.set_search(search);
            self.notify("playlist-search");
        }
    }

    fn add_song(&self) {
        let ctx = glib::MainContext::default();
        ctx.spawn_local(clone!(@weak self as win => async move {
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            let filter = gtk::FileFilter::new();
            gtk::FileFilter::set_name(&filter, Some(&i18n("Audio files")));
            filter.add_mime_type("audio/*");
            for pattern in utils::AUDIO_PATTERNS { filter.add_pattern(pattern); filter.add_pattern(&pattern.to_ascii_uppercase()); }
            filters.append(&filter);

            let dialog = gtk::FileDialog::builder()
                .accept_label(i18n("_Add Song"))
                .filters(&filters)
                .modal(true)
                .title(i18n("Open File"))
                .build();

            if let Ok(files) = dialog.open_multiple_future(Some(&win)).await {
                if files.n_items() == 0 {
                    win.add_toast(i18n("Unable to access files"));
                } else {
                    win.add_files_to_queue(&files);
                }
            }
        }));
    }

    fn add_folder(&self) {
        let ctx = glib::MainContext::default();
        ctx.spawn_local(clone!(@weak self as win => async move {
            let dialog = gtk::FileDialog::builder()
                .accept_label(i18n("_Add Folder"))
                .modal(true)
                .title(i18n("Open Folder"))
                .build();

            if let Ok(files) = dialog.select_multiple_folders_future(Some(&win)).await {
                if files.n_items() == 0 {
                    win.add_toast(i18n("Unable to access files"));
                } else {
                    win.add_files_to_queue(&files);
                }
            }
        }));
    }

    fn restore_playlist(&self) {
        if self
            .player()
            .map(|p| !p.queue().is_empty())
            .unwrap_or(false)
        {
            return;
        }
        if let Some(session) = crate::session::load() {
            let files = session["songs"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str())
                .map(gio::File::for_uri)
                .filter(|f| f.path().map(|p| p.is_file()).unwrap_or(false))
                .collect::<Vec<_>>();
            if files.is_empty() {
                return;
            }
            self.imp().restoring.replace(Some(session));
            self.queue_songs(files);
            return;
        }
        if let Some(songs) = utils::load_cached_songs() {
            self.queue_songs(songs);
        }
    }

    fn queue_songs(&self, queue: Vec<gio::File>) {
        if queue.is_empty() {
            self.add_toast(i18n("No available song found"));
            return;
        }

        self.switch_mode(WindowMode::MainView);

        // Disable actions on the queue; loading is "atomic"
        self.action_set_enabled("queue.add-song", false);
        self.action_set_enabled("queue.add-folder", false);
        self.action_set_enabled("queue.clear", false);

        self.imp().playlist_view.begin_loading();

        // Begin the trace
        let now = Instant::now();

        // Turn the list of files into songs one at a time into the main loop
        let n_files = queue.len() as u32;

        let mut files = queue.into_iter();
        let mut songs = Vec::new();
        let mut cur_file: u32 = 0;
        let mut duplicates: u32 = 0;

        glib::idle_add_local(
            clone!(@weak self as win => @default-return glib::ControlFlow::Break, move || {
                files.next()
                    .map(|f| {
                        win.imp().playlist_view.update_loading(cur_file, n_files);
                        if let Ok(s) = Song::from_uri(f.uri().as_str()) {
                            if let Some(player) = win.player() {
                                let queue = player.queue();
                                if queue.contains(&s) || songs.iter().any(|song: &Song| song.equals(&s)) {
                                    duplicates += 1;
                                } else {
                                    songs.push(s);
                                    cur_file += 1;
                                }
                            }
                        }
                    })
                    .map(|_| glib::ControlFlow::Continue)
                    .unwrap_or_else(|| {
                        debug!("Total loading time for {} files: {} ms", n_files, now.elapsed().as_millis());

                        // Re-enable the actions
                        win.action_set_enabled("queue.add-song", true);
                        win.action_set_enabled("queue.add-folder", true);
                        win.action_set_enabled("queue.clear", true);

                        if songs.is_empty() {
                            win.imp().restoring.replace(None);
                            win.imp().playlist_view.end_loading();
                            if duplicates == 0 {
                                win.add_toast(i18n("No songs found"));
                            }
                        } else if let Some(player) = win.player() {
                            let queue = player.queue();
                            let was_empty = queue.is_empty();

                            win.imp().playlist_view.end_loading();

                            // Bulk add to avoid hammering the UI with list model updates
                            queue.add_songs(&songs);

                            // Store the current state of the playlist
                            if win.imp().restoring.borrow().is_none() { utils::store_playlist(queue); }

                            debug!("Queue was empty: {}, new size: {}", was_empty, queue.n_songs());
                            if was_empty {
                                player.skip_to(0);
                            }

                            // Allow jumping to the song we just added
                            if win.imp().restoring.borrow().is_some() {
                                win.finish_restore();
                            } else if songs.len() == 1 {
                                // If we added a single song, and the queue was empty, we
                                // dispense with the pleasantries and we start playing
                                // immediately; otherwise, we let the user choose whether
                                // to jump to the newly added song
                                if was_empty {
                                    player.play();
                                } else {
                                    win.add_skip_to_toast(
                                        i18n("Added a new song"),
                                        i18n("Play"),
                                        queue.n_songs() - 1,
                                    );
                                }
                            } else {
                                let msg = ni18n_f(
                                    // Translators: the `{}` must be left unmodified;
                                    // it will be expanded to the number of songs added
                                    // to the playlist
                                    "Added one song",
                                    "Added {} songs",
                                    songs.len() as u32,
                                    &[&songs.len().to_string()],
                                );

                                win.add_toast(msg);
                            }
                        }

                        glib::ControlFlow::Break
                    })
            }),
        );
    }

    fn add_files_to_queue(&self, model: &gio::ListModel) {
        let mut queue: Vec<gio::File> = vec![];

        for pos in 0..model.n_items() {
            let file = model.item(pos).unwrap().downcast::<gio::File>().unwrap();

            if let Ok(info) = file.query_info(
                "standard::name,standard::display-name,standard::type,standard::content-type",
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                gio::Cancellable::NONE,
            ) {
                match info.file_type() {
                    gio::FileType::Regular => {
                        if file
                            .path()
                            .and_then(|p| {
                                p.extension()
                                    .map(|s| s.to_string_lossy().to_ascii_lowercase())
                            })
                            .map(|s| s == "m3u" || s == "m3u8")
                            .unwrap_or(false)
                        {
                            if let Some(path) = file.path() {
                                if let Ok(files) = crate::library::read_m3u(&path) {
                                    queue.extend(files);
                                }
                            }
                            continue;
                        }
                        if utils::is_audio_file(&file, info.content_type().as_deref()) {
                            debug!("Adding file '{}' to the queue", file.uri());
                            queue.push(file);
                        }
                    }
                    gio::FileType::Directory => {
                        debug!("Adding folder '{}' to the queue", file.uri());
                        if let Some(path) = file.path() {
                            let path = path.to_string_lossy().to_string();
                            let mut recent: Vec<String> = self
                                .imp()
                                .settings
                                .strv("recent-folders")
                                .iter()
                                .map(|s| s.to_string())
                                .filter(|s| *s != path)
                                .collect();
                            recent.insert(0, path);
                            recent.truncate(8);
                            let recent: Vec<&str> = recent.iter().map(|s| s.as_str()).collect();
                            let _ = self.imp().settings.set_strv("recent-folders", recent);
                        }
                        let files = utils::load_files_from_folder(&file, true);
                        queue.extend(files);
                    }
                    _ => (),
                }
            }
        }

        self.queue_songs(queue);
    }

    // Bind the PlayerState to the UI
    fn bind_state(&self) {
        let imp = self.imp();
        if let Some(player) = self.player() {
            let state = player.state();

            // Use the PlayerState:playing property to control the play/pause button
            self.update_play_button();
            let notify_playing_id = state.connect_notify_local(
                Some("playing"),
                clone!(@weak self as win => move |_, _| {
                    win.set_playlist_selection(false);
                    win.update_play_button();
                }),
            );
            imp.notify_playing_id.replace(Some(notify_playing_id));

            // Update the position labels
            self.update_position_labels();
            let notify_position_id = state.connect_notify_local(
                Some("position"),
                clone!(@weak self as win => move |_, _| {
                    win.update_position_labels();
                }),
            );
            imp.notify_position_id.replace(Some(notify_position_id));

            // Update the UI
            self.update_song();
            let notify_song_id = state.connect_notify_local(
                Some("song"),
                clone!(@weak self as win => move |_, _| {
                    win.update_song();
                }),
            );
            imp.notify_song_id.replace(Some(notify_song_id));

            // Update the cover, if any is available
            self.update_cover();
            let notify_cover_id = state.connect_notify_local(
                Some("cover"),
                clone!(@weak self as win => move |_, _| {
                    win.update_cover();
                }),
            );
            imp.notify_cover_id.replace(Some(notify_cover_id));

            // Bind the song properties to the UI
            state
                .bind_property("title", &imp.song_details.get().title_label(), "label")
                .flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE)
                .build();
            state
                .bind_property("artist", &imp.song_details.get().artist_label(), "label")
                .flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE)
                .build();
            state
                .bind_property("album", &imp.song_details.get().album_label(), "label")
                .flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE)
                .build();
            state
                .bind_property(
                    "volume",
                    &imp.playback_control.get().volume_control(),
                    "volume",
                )
                .flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE)
                .build();
        }
    }

    fn unbind_state(&self) {
        if let Some(player) = self.player() {
            let state = player.state();

            if let Some(id) = self.imp().notify_playing_id.take() {
                state.disconnect(id);
            }
            if let Some(id) = self.imp().notify_position_id.take() {
                state.disconnect(id);
            }
            if let Some(id) = self.imp().notify_song_id.take() {
                state.disconnect(id);
            }
            if let Some(id) = self.imp().notify_cover_id.take() {
                state.disconnect(id);
            }
        }
    }

    // Bind the Queue to the UI
    fn bind_queue(&self) {
        if let Some(player) = self.player() {
            let queue = player.queue();

            let notify_nsongs_id = queue.connect_notify_local(
                Some("n-songs"),
                clone!(@weak self as win => move |queue, _| {
                    debug!("queue.n_songs() = {}", queue.n_songs());
                    if queue.is_empty() {
                        win.set_initial_state();
                        win.reset_queue();
                    } else {
                        win.action_set_enabled("queue.toggle", true);
                        win.action_set_enabled("queue.shuffle", queue.n_songs() > 1);

                        win.action_set_enabled("win.play", true);
                        win.action_set_enabled("win.previous", true);
                        win.action_set_enabled("win.next", queue.n_songs() > 1);
                    }

                    if queue.n_songs() == 1 {
                        if let Some(p) = win.player() {
                            p.skip_next();
                        }
                    }

                    win.update_playlist_time();
                }),
            );
            self.imp().notify_nsongs_id.replace(Some(notify_nsongs_id));

            queue.connect_notify_local(
                Some("repeat-mode"),
                clone!(@weak self as win => move |queue, _| {
                    win.imp().playback_control.set_repeat_mode(queue.repeat_mode());
                }),
            );

            let notify_current_id = queue.connect_notify_local(
                Some("current"),
                clone!(@weak self as win => move |queue, _| {
                    if queue.is_last_song() {
                        match queue.repeat_mode() {
                            RepeatMode::Consecutive => {
                                win.action_set_enabled("win.next", false);
                            },
                            _ => {
                                win.action_set_enabled("win.next", true);
                            }
                        }
                    } else {
                        win.action_set_enabled("win.next", true);
                    }
                }),
            );
            self.imp()
                .notify_current_id
                .replace(Some(notify_current_id));
        }
    }

    fn unbind_queue(&self) {
        if let Some(player) = self.player() {
            let queue = player.queue();
            if let Some(id) = self.imp().notify_nsongs_id.take() {
                queue.disconnect(id);
            }
            if let Some(id) = self.imp().notify_current_id.take() {
                queue.disconnect(id);
            }
        }
    }

    fn connect_signals(&self) {
        self.imp().split_view.connect_notify_local(
            Some("collapsed"),
            clone!(@weak self as win => move |split_view, _| {
                win.set_playlist_visible(split_view.shows_sidebar());
                win.imp().playlist_view.back_button().set_visible(split_view.is_collapsed());
            }),
        );

        self.imp().split_view.connect_notify_local(
            Some("show-sidebar"),
            clone!(@weak self as win => move |split_view, _| {
                win.set_playlist_visible(split_view.shows_sidebar());
            }),
        );

        self.imp().waveform_view.connect_closure(
            "position-changed",
            false,
            closure_local!(@watch self as win => move |_wv: WaveformView, position: f64| {
                debug!("New position: {}", position);
                if let Some(player) = win.player() {
                    let state = player.state();
                    if state.current_song().is_some() {
                        player.seek_position_rel(position);
                        player.play();
                    }
                }
            }),
        );

        self.imp()
            .playback_control
            .volume_control()
            .connect_closure(
                "volume-changed",
                false,
                closure_local!(@watch self as win => move |_vc: VolumeControl, volume: f64| {
                    debug!("Volume changed: {}", volume);
                    if let Some(p) = win.player() {
                        p.set_volume(volume);
                    }
                }),
            );

        self.imp()
            .playlist_view
            .queue_select_all_button()
            .connect_clicked(clone!(@weak self as win => move |_| {
                if win.playlist_search() {
                    if let Some(ref model) = *win.imp().playlist_filtermodel.borrow() {
                        for idx in 0..model.n_items() {
                            let item = model.item(idx).unwrap();
                            let song = item.downcast_ref::<Song>().unwrap();
                            song.set_selected(true);
                        }
                    }
                } else if let Some(player) = win.player() {
                    let queue = player.queue();
                    for idx in 0..queue.n_songs() {
                        let song = queue.song_at(idx).unwrap();
                        song.set_selected(true);
                    }
                }
            }));

        self.imp()
            .playlist_view
            .queue_remove_button()
            .connect_clicked(clone!(@weak self as win => move |_| {
                if let Some(player) = win.player() {
                    let queue = player.queue();
                    let mut remove_songs: Vec<Song> = Vec::new();
                    // Collect all songs to be removed first, since we can't
                    // remove objects from the model while we're iterating it
                    for idx in 0..queue.n_songs() {
                        let song = queue.song_at(idx).unwrap();
                        if song.selected() {
                            remove_songs.push(song);
                        }
                    }

                    for song in remove_songs {
                        win.remove_song(&song);
                    }

                    // Store the current state of the playlist
                    utils::store_playlist(queue);
                }
            }));

        self.imp()
            .playlist_view
            .playlist_searchbar()
            .connect_notify_local(
                Some("search-mode-enabled"),
                clone!(@weak self as win => move |searchbar, _| {
                    win.set_playlist_search(searchbar.is_search_mode());
                }),
            );

        self.imp().settings.connect_changed(
            Some("enable-recoloring"),
            clone!(@weak self as this => move |settings, _| {
                debug!("GSettings:enable-recoloring: {}", settings.boolean("enable-recoloring"));
                if let Some(player) = this.player() {
                    let state = player.state();
                    this.update_style(state.current_song().as_ref());
                }
            }),
        );
        let _dummy = self.imp().settings.boolean("enable-recoloring");

        self.connect_close_request(move |window| {
            debug!("Saving window state");
            if let Some(player) = window.player() {
                crate::session::save(&player);
            }
            let (width, height) = if window.imp().mini.get() {
                window.imp().full_size.get()
            } else {
                window.default_size()
            };

            let settings = utils::settings_manager();
            settings
                .set_int("window-width", width)
                .expect("Unable to store window-width");
            settings
                .set_int("window-height", height)
                .expect("Unable to stop window-height");

            #[cfg(target_os = "windows")]
            if window.imp().tray.borrow().is_some() && settings.boolean("background-play") {
                if let Some(app) = window.application() {
                    window.imp().background_hold.replace(Some(app.hold()));
                    window.set_visible(false);
                    return glib::Propagation::Stop;
                }
            }

            window.unbind_queue();
            window.unbind_state();
            window.unbind_waveform();

            glib::Propagation::Proceed
        });

        self.imp()
            .playlist_view
            .playlist_searchbar()
            .set_key_capture_widget(Some(self.upcast_ref::<gtk::Widget>()));
    }

    // The initial state of the playback actions
    fn set_initial_state(&self) {
        if let Some(player) = self.player() {
            let queue = player.queue();
            self.action_set_enabled("win.play", !queue.is_empty());
            self.action_set_enabled("win.previous", !queue.is_empty());
            self.action_set_enabled("win.next", !queue.is_last_song());

            self.action_set_enabled("queue.toggle", !queue.is_empty());
            self.action_set_enabled("queue.shuffle", queue.n_songs() > 1);
            self.action_set_enabled("win.replaygain", player.replaygain_available());

            let replaygain = self.imp().settings.enum_("replay-gain").into();
            self.set_replaygain(replaygain);

            // Manually set player state, because set_replaygain
            // only updates player state when the value changes.
            player.set_replaygain(replaygain);

            self.imp()
                .playback_control
                .set_repeat_mode(queue.repeat_mode());
            self.set_playlist_shuffled(queue.is_shuffled());

            // Manually update the icon on the initial empty state
            // to avoid generating the UI definition file at build
            // time
            self.imp().status_page.set_icon_name(Some(APPLICATION_ID));

            if utils::has_cached_playlist() {
                self.imp().restore_playlist_button.set_visible(true);
                self.imp()
                    .restore_playlist_button
                    .add_css_class("suggested-action");
                self.imp()
                    .add_folder_button
                    .remove_css_class("suggested-action");
            } else {
                self.imp().restore_playlist_button.set_visible(false);
                self.imp()
                    .restore_playlist_button
                    .remove_css_class("suggested-action");
                self.imp()
                    .add_folder_button
                    .add_css_class("suggested-action");
            }

            let state = player.state();
            if state.playing() || !queue.is_empty() {
                self.switch_mode(WindowMode::MainView);
            } else {
                self.switch_mode(WindowMode::InitialView);
            }
        }
    }

    fn setup_playlist(&self) {
        let imp = self.imp();

        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(clone!(@weak self as win => move |_, item| {
            let row = QueueRow::default();
            let list_item = item.downcast_ref::<gtk::ListItem>().unwrap();
            list_item.set_child(Some(&row));

            row.connect_notify_local(
                Some("selected"),
                clone!(@weak win => move |_, _| {
                    win.update_selected_count();
                }),
            );

            win
                .bind_property("playlist-selection", &row, "selection-mode")
                .flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE)
                .build();

            list_item
                .bind_property("item", &row, "song")
                .flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE)
                .build();

            list_item
                .property_expression("item")
                .chain_property::<Song>("artist")
                .bind(&row, "song-artist", gtk::Widget::NONE);
            list_item
                .property_expression("item")
                .chain_property::<Song>("title")
                .bind(&row, "song-title", gtk::Widget::NONE);
            list_item
                .property_expression("item")
                .chain_property::<Song>("cover")
                .bind(&row, "song-cover", gtk::Widget::NONE);
            list_item
                .property_expression("item")
                .chain_property::<Song>("playing")
                .bind(&row, "playing", gtk::Widget::NONE);
            list_item
                .property_expression("item")
                .chain_property::<Song>("selected")
                .bind(&row, "selected", gtk::Widget::NONE);
        }));
        imp.playlist_view
            .queue_view()
            .set_factory(Some(&factory.upcast::<gtk::ListItemFactory>()));

        if let Some(player) = self.player() {
            let queue = player.queue();

            let filter = FuzzyFilter::new();
            let filter_model =
                gtk::FilterListModel::new(Some(queue.model().clone()), Some(filter.clone()));
            let sorter = FuzzySorter::new();
            let sorter_model = gtk::SortListModel::new(Some(filter_model), Some(sorter.clone()));
            let selection = gtk::NoSelection::new(Some(sorter_model.clone()));
            imp.playlist_view
                .queue_view()
                .set_model(Some(selection.upcast_ref::<gtk::SelectionModel>()));
            imp.playlist_view.queue_view().connect_activate(
                clone!(@weak self as win, @weak selection => move |_, pos| {
                    let song = selection
                        .upcast::<gio::ListModel>()
                        .item(pos)
                        .unwrap()
                        .downcast::<Song>()
                        .unwrap();

                    if let Some(player) = win.player() {
                        let queue = player.queue();

                        let mut real_pos = None;
                        for i in 0..queue.model().n_items() {
                            if let Some(item) = queue.model().item(i) {
                                let s = item.downcast::<Song>().unwrap();
                                if s.equals(&song) {
                                    real_pos = Some(i);
                                    break;
                                }
                            }
                        }

                        if let Some(real_pos) = real_pos {
                            if win.playlist_selection() {
                                queue.select_song_at(real_pos);
                            } else if queue.current_song_index() != Some(real_pos) {
                                player.skip_to(real_pos);
                                player.play();
                            } else if !player.state().playing() {
                                player.play();
                            }
                        }
                    }
                }),
            );

            imp.playlist_filtermodel
                .replace(Some(sorter_model.upcast::<gio::ListModel>()));

            imp.playlist_view
                .playlist_searchentry()
                .bind_property("text", &filter, "search")
                .flags(glib::BindingFlags::SYNC_CREATE)
                .build();
            imp.playlist_view
                .playlist_searchentry()
                .bind_property("text", &sorter, "search")
                .flags(glib::BindingFlags::SYNC_CREATE)
                .build();
            imp.playlist_view
                .playlist_searchentry()
                .connect_search_changed(clone!(@weak self as win => move |_| {
                    if let Some(adjustment) = win.imp().playlist_view.queue_view().vadjustment() {
                        adjustment.set_value(0.0);
                    }
                }));
        }
    }

    fn setup_drop_target(&self) {
        let drop_target = gtk::DropTarget::builder()
            .name("file-drop-target")
            .actions(gdk::DragAction::COPY)
            .formats(&gdk::ContentFormats::for_type(gdk::FileList::static_type()))
            .build();

        drop_target.connect_drop(
            clone!(@weak self as win => @default-return false, move |_, value, _, _| {
                if let Ok(file_list) = value.get::<gdk::FileList>() {
                    if file_list.files().is_empty() {
                        win.add_toast(i18n("Unable to access dropped files"));
                        return false;
                    }

                    let model = gio::ListStore::new::<gio::File>();
                    for f in file_list.files() {
                        model.append(&f);
                    }
                    win.add_files_to_queue(model.upcast_ref::<gio::ListModel>());
                    return true;
                }

                false
            }),
        );

        self.imp().drag_overlay.set_drop_target(&drop_target);
    }

    fn update_play_button(&self) {
        if let Some(player) = self.player() {
            let state = player.state();
            let play_button = self.imp().playback_control.play_button();
            if state.playing() {
                play_button.set_icon_name("media-playback-pause-symbolic");
            } else {
                play_button.set_icon_name("media-playback-start-symbolic");
            }
        }
    }

    fn update_position_labels(&self) {
        self.update_lyrics();
        if let Some(player) = self.player() {
            let state = player.state();
            if state.current_song().is_some() {
                let elapsed = state.position();
                let duration = state.duration();
                let remaining = duration.checked_sub(elapsed).unwrap_or_default();
                self.set_song_time(Some(elapsed), Some(remaining));

                let position = if state.duration() > 0 {
                    state.position() as f64 / state.duration() as f64
                } else {
                    0.0
                };
                self.set_song_position(position);
            } else {
                self.set_song_time(None, None);
                self.set_song_position(0.0);
            }
        }
    }

    fn update_song(&self) {
        self.load_lyrics();
        if let Some(player) = self.player() {
            let state = player.state();
            self.scroll_playlist_to_song();
            self.update_playlist_time();
            self.update_title(state.current_song().as_ref());
            self.update_style(state.current_song().as_ref());
        }
    }

    fn update_cover(&self) {
        if let Some(player) = self.player() {
            let state = player.state();
            let song_cover = self.imp().song_cover.get();
            if let Some(cover) = state.cover() {
                song_cover.album_image().set_cover(Some(&cover));
                song_cover.show_cover_image(true);
            } else {
                song_cover.album_image().set_cover(None);
                song_cover.show_cover_image(false);
            }
        }
    }

    fn update_playlist_time(&self) {
        if let Some(player) = self.player() {
            let queue = player.queue();
            let n_songs = queue.n_songs();
            if let Some(current) = queue.current_song_index() {
                let mut remaining_time = 0;
                for pos in 0..n_songs {
                    let song = queue.song_at(pos).unwrap();
                    if pos >= current {
                        remaining_time += song.duration();
                    }
                }

                let remaining_min = ((remaining_time - (remaining_time % 60)) / 60) as u32;
                let remaining_hrs = (remaining_min - (remaining_min % 60)) / 60;

                let remaining_str = if remaining_hrs > 0 {
                    // Translators: the `{}` must be left unmodified, and
                    // it will be replaced by the number of minutes remaining
                    // in the string "N hours M minutes remaining"
                    let minutes = ni18n_f(
                        "{} minute",
                        "{} minutes",
                        remaining_min % 60,
                        &[&(remaining_min % 60).to_string()],
                    );

                    // Translators: `{hours}` and `{minutes}` must be left
                    // unmodified, and they will be replaced by number of
                    // hours and by the translated number of minutes
                    // remaining, respectively
                    ni18n_k(
                        "{hours} hour {minutes} remaining",
                        "{hours} hours {minutes} remaining",
                        remaining_hrs,
                        &[("hours", &remaining_hrs.to_string()), ("minutes", &minutes)],
                    )
                } else {
                    ni18n_f(
                        // Translators: the '{}' must be left unmodified, and
                        // it will be replaced by the number of minutes remaining
                        // in the playlist
                        "{} minute remaining",
                        "{} minutes remaining",
                        remaining_min,
                        &[&remaining_min.to_string()],
                    )
                };

                self.imp()
                    .playlist_view
                    .queue_length_label()
                    .set_label(&remaining_str);
                self.imp()
                    .playlist_view
                    .queue_length_label()
                    .set_visible(true);
            } else {
                self.imp()
                    .playlist_view
                    .queue_length_label()
                    .set_visible(false);
            }
        }
    }

    fn scroll_playlist_to_song(&self) {
        let queue_view = self.imp().playlist_view.queue_view();
        if let Some(player) = self.player() {
            if let Some(current_idx) = player.queue().current_song_index() {
                debug!("Scrolling playlist to {}", current_idx);
                queue_view
                    .upcast_ref::<gtk::Widget>()
                    .activate_action("list.scroll-to-item", Some(&current_idx.to_variant()))
                    .expect("Failed to activate action");
            }
        }
    }

    fn setup_provider(&self) {
        let imp = self.imp();
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, &imp.provider, 400);
        }
    }

    fn update_style(&self, song: Option<&Song>) {
        let imp = self.imp();

        if !imp.settings.boolean("enable-recoloring")
            || adw::StyleManager::default().is_high_contrast()
        {
            imp.provider.load_from_data("@define-color background_color_0 @window_bg_color; @define-color background_color_1 @window_bg_color; @define-color background_color_2 @window_bg_color;");
            imp.main_stack.remove_css_class("main-window");
            return;
        }

        if let Some(song) = song {
            if let Some(bg_colors) = song.cover_palette() {
                let mut css = String::new();

                let n_colors = bg_colors.len();
                for (i, color) in bg_colors.iter().enumerate().take(n_colors) {
                    let s = format!("@define-color background_color_{} {};", i, color);
                    css.push_str(&s);
                }

                for i in n_colors..5 {
                    css.push_str(&format!(
                        "@define-color background_color_{} @window_bg_color;",
                        i
                    ));
                }

                imp.provider.load_from_data(&css);
                if !imp.main_stack.has_css_class("main-window") {
                    imp.main_stack.add_css_class("main-window");
                }

                self.action_set_enabled("win.enable-recoloring", true);

                return;
            }
        }

        imp.provider.load_from_data("@define-color background_color_0 @window_bg_color; @define-color background_color_1 @window_bg_color; @define-color background_color_2 @window_bg_color;");
        imp.main_stack.remove_css_class("main-window");
        self.action_set_enabled("win.enable-recoloring", false);
    }

    fn update_title(&self, song: Option<&Song>) {
        if let Some(song) = song {
            self.set_title(Some(&format!("{} - {}", song.artist(), song.title())));
        } else {
            self.set_title(Some("Amberol"));
        }
    }

    fn update_selected_count(&self) {
        if let Some(player) = self.player() {
            let queue = player.queue();
            let n_selected = queue.n_selected_songs();

            let selected_str = if n_selected == 0 {
                i18n("No song selected")
            } else {
                ni18n_f(
                    // Translators: The '{}' must be left unmodified, and
                    // it is expanded to the number of songs selected
                    "{} song selected",
                    "{} songs selected",
                    n_selected,
                    &[&n_selected.to_string()],
                )
            };

            self.imp()
                .playlist_view
                .queue_selected_label()
                .set_label(&selected_str);
        }
    }

    pub fn open_files(&self, files: &[gio::File]) {
        if files.is_empty() {
            self.add_toast(i18n("Unable to access files"));
            return;
        }

        let model = gio::ListStore::new::<gio::File>();
        for f in files {
            model.append(f);
        }
        self.add_files_to_queue(model.upcast_ref::<gio::ListModel>());
    }

    pub fn remove_song(&self, song: &Song) {
        if let Some(p) = self.player() {
            p.remove_song(song);
        }
        self.update_selected_count();
        self.update_playlist_time();
    }

    pub fn add_toast(&self, msg: String) {
        let toast = adw::Toast::new(&msg);
        self.imp().toast_overlay.add_toast(toast);
    }

    pub fn add_skip_to_toast(&self, msg: String, button: String, pos: u32) {
        let toast = adw::Toast::new(&msg);
        toast.set_button_label(Some(&button));
        toast.set_action_name(Some("win.skip-to"));
        toast.set_action_target_value(Some(&pos.to_variant()));
        self.imp().toast_overlay.add_toast(toast);
    }

    fn copy_song(&self) {
        if let Some(player) = self.player() {
            let state = player.state();
            if let Some(song) = state.current_song() {
                let s = i18n_k(
                    // Translators: `{title}` and `{artist}` must be left
                    // untranslated; they will expand to the title and
                    // artist of the currently playing song, respectively
                    "Currently playing “{title}” by “{artist}”",
                    &[("title", &song.title()), ("artist", &song.artist())],
                );
                self.clipboard().set_text(&s);
            }
        }
    }

    pub fn switch_mode(&self, mode: WindowMode) {
        let stack = self.imp().main_stack.get();
        match mode {
            WindowMode::InitialView => {
                stack.set_visible_child_name("initial-view");
                self.set_default_widget(Some(&self.imp().add_folder_button.get()));
            }
            WindowMode::MainView => {
                stack.set_visible_child_name("main-view");
                self.set_default_widget(Some(&self.imp().playback_control.play_button()));
            }
        };
    }

    pub fn set_replaygain(&self, replaygain: ReplayGainMode) {
        let imp = self.imp();

        if replaygain != imp.replaygain_mode.replace(replaygain) {
            if let Some(p) = self.player() {
                p.set_replaygain(replaygain);
            }
            self.imp()
                .settings
                .set_enum("replay-gain", replaygain.into())
                .expect("Unable to store setting");

            self.notify("replaygain-mode");
        }
    }

    pub fn replaygain(&self) -> ReplayGainMode {
        self.imp().replaygain_mode.get()
    }

    pub fn set_song_time(&self, elapsed: Option<u64>, remaining: Option<u64>) {
        if let Some(elapsed) = elapsed {
            self.imp()
                .elapsed_label
                .set_text(&utils::format_time(elapsed as i64));
        } else {
            self.imp().elapsed_label.set_text("0:00");
        }

        if let Some(remaining) = remaining {
            self.imp()
                .remaining_label
                .set_text(&utils::format_remaining_time(remaining as i64));
        } else {
            self.imp().remaining_label.set_text("0:00");
        }
    }

    pub fn set_song_position(&self, position: f64) {
        self.imp().waveform_view.set_position(position);
    }
}
