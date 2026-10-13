// SPDX-FileCopyrightText: 2026 Amberol contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk::{gio, glib, prelude::*};
use std::{
    io,
    path::{Path, PathBuf},
};

pub fn directory() -> PathBuf {
    glib::user_data_dir().join("amberol").join("playlists")
}

pub fn write_m3u(path: &Path, uris: &[String]) -> io::Result<()> {
    let mut text = String::from("#EXTM3U\n");
    for uri in uris {
        // File URIs preserve Unicode, newlines and filenames beginning with '#'.
        if !uri.contains(['\r', '\n']) {
            text.push_str(uri);
            text.push('\n');
        }
    }
    let temp = path.with_extension("m3u8.tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(temp, path)
}

pub fn read_m3u(path: &Path) -> io::Result<Vec<gio::File>> {
    let text = std::fs::read_to_string(path)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    Ok(text
        .trim_start_matches('\u{feff}')
        .lines()
        .filter_map(|row| {
            let row = row.trim();
            if row.is_empty() || row.starts_with('#') {
                return None;
            }
            let file = if row.starts_with("file:") {
                gio::File::for_uri(row)
            } else if row.contains("://") {
                return None;
            } else {
                gio::File::for_path(parent.join(row))
            };
            file.path().filter(|p| p.is_file()).map(|_| file)
        })
        .collect())
}

pub fn named_path(name: &str) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 120
        || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        || name.chars().any(char::is_control)
        || name.ends_with(['.', ' '])
    {
        return None;
    }
    // A prefix also avoids Windows reserved device names (CON, AUX, etc.).
    Some(directory().join(format!("list-{name}.m3u8")))
}

pub fn playlists() -> Vec<(String, PathBuf)> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(directory()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("m3u8") {
                if let Some(name) = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_prefix("list-"))
                {
                    result.push((name.to_owned(), path));
                }
            }
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prevents_path_escape() {
        assert!(named_path("../escape").is_none());
        assert!(named_path("CON").unwrap().ends_with("list-CON.m3u8"));
        assert!(named_path("夜晚").is_some());
        assert!(named_path("name.").is_none());
    }
    #[test]
    fn m3u_unicode_relative_paths_and_bom() {
        let dir = std::env::temp_dir().join(format!("amberol-m3u-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let song = dir.join("音乐 #1.wav");
        std::fs::write(&song, b"test").unwrap();
        let playlist = dir.join("test.m3u8");
        std::fs::write(
            &playlist,
            "\u{feff}#EXTM3U\n音乐 #1.wav\nmissing.wav\nhttps://example.com/song\n",
        )
        .unwrap();
        let files = read_m3u(&playlist).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path().unwrap(), song);
        write_m3u(&playlist, &[files[0].uri().to_string()]).unwrap();
        assert_eq!(read_m3u(&playlist).unwrap()[0].path().unwrap(), song);
        std::fs::remove_file(playlist).unwrap();
        std::fs::remove_file(song).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
