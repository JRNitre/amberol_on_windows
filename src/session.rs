// SPDX-FileCopyrightText: 2026 Amberol contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::audio::AudioPlayer;
use gtk::glib;
use serde_json::{json, Value};

pub fn path() -> std::path::PathBuf {
    glib::user_data_dir().join("amberol/session.json")
}

pub fn save(player: &AudioPlayer) {
    let queue = player.queue();
    let songs = queue.canonical_songs();
    let uris: Vec<_> = songs.iter().map(|song| song.uri()).collect();
    let data = json!({
        "version": 1, "songs": uris,
        "current": player.state().current_song().map(|s| s.uri()),
        "position": player.state().position(), "volume": player.state().volume(),
        "repeat": queue.repeat_mode().to_string(), "shuffle": queue.shuffle_order()
    });
    let path = path();
    let result = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(&data)?)?;
        std::fs::rename(temp, path)
    })();
    if let Err(err) = result {
        log::warn!("Cannot save playback session: {err}");
    }
}

pub fn load() -> Option<Value> {
    let data: Value = serde_json::from_slice(&std::fs::read(path()).ok()?).ok()?;
    (data["version"] == 1 && data["songs"].as_array()?.len() <= 100_000).then_some(data)
}
