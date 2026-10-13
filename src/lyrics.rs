// SPDX-FileCopyrightText: 2026 Amberol contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub time_ms: i64,
    pub text: String,
}

pub fn parse(text: &str) -> Vec<Line> {
    let timestamp = regex::Regex::new(r"\[(\d+):(\d{2})(?:[.:](\d{1,3}))?\]").unwrap();
    let offset = regex::Regex::new(r"(?i)\[offset:([+-]?\d+)\]")
        .unwrap()
        .captures(text)
        .and_then(|c| c[1].parse::<i64>().ok())
        .unwrap_or(0);
    let mut lines = Vec::new();
    for row in text.lines() {
        let lyric = timestamp.replace_all(row, "").trim().to_owned();
        for cap in timestamp.captures_iter(row) {
            let minutes = cap[1].parse::<i64>().unwrap_or(0);
            let seconds = cap[2].parse::<i64>().unwrap_or(0);
            if seconds >= 60 {
                continue;
            }
            let fraction = cap
                .get(3)
                .map(|m| {
                    m.as_str().parse::<i64>().unwrap_or(0) * 10_i64.pow(3 - m.as_str().len() as u32)
                })
                .unwrap_or(0);
            lines.push(Line {
                time_ms: (minutes * 60 + seconds) * 1000 + fraction + offset,
                text: lyric.clone(),
            });
        }
    }
    lines.sort_by_key(|line| line.time_ms);
    lines
}

pub fn current(lines: &[Line], position_ms: i64, offset_ms: i64) -> Option<usize> {
    let position = position_ms.saturating_add(offset_ms);
    lines.iter().rposition(|line| line.time_ms <= position)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiple_tags_fractions_and_offset() {
        let lines = parse("[offset:-500]\n[00:02.50][01:02.005]你好\n[00:01]开头\n[00:90]无效");
        assert_eq!(
            lines.iter().map(|l| l.time_ms).collect::<Vec<_>>(),
            vec![500, 2000, 61505]
        );
        assert_eq!(lines[1].text, "你好");
        assert_eq!(current(&lines, 1900, 100), Some(1));
        assert_eq!(current(&lines, 0, 0), None);
    }
}
