use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SubFormat {
    SRT,
    VTT,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleTile {
    pub id: Uuid,
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    #[serde(default)]
    pub meta: HashMap<String, String>,
}

// ── Decomposer ───────────────────────────────────────────────────────────────

pub struct SubtitleDecomposer;

impl SubtitleDecomposer {
    // ── Detection ────────────────────────────────────────────────────────

    pub fn detect_format(input: &str) -> SubFormat {
        let trimmed = input.trim();
        if trimmed.starts_with("WEBVTT") {
            SubFormat::VTT
        } else if trimmed.starts_with('1')
            || trimmed.split_whitespace().next().map_or(false, |n| n.parse::<u32>().is_ok())
        {
            // Heuristic: SRT starts with a sequence number (usually 1)
            SubFormat::SRT
        } else {
            SubFormat::Unknown
        }
    }

    // ── Parsing ──────────────────────────────────────────────────────────

    pub fn parse_srt(input: &str) -> Vec<SubtitleTile> {
        let mut tiles = Vec::new();
        for block in input.trim().split("\n\n") {
            let lines: Vec<&str> = block.lines().collect();
            if lines.len() < 3 {
                continue;
            }
            let index = match lines[0].trim().parse::<u32>() {
                Ok(i) => i,
                Err(_) => continue,
            };
            let (start_ms, end_ms) = match parse_srt_timestamp(lines[1].trim()) {
                Some(t) => t,
                None => continue,
            };
            let text = lines[2..].join("\n");
            tiles.push(SubtitleTile {
                id: Uuid::new_v4(),
                index,
                start_ms,
                end_ms,
                text,
                meta: HashMap::new(),
            });
        }
        tiles
    }

    pub fn parse_vtt(input: &str) -> Vec<SubtitleTile> {
        let mut tiles = Vec::new();
        let mut idx: u32 = 0;
        // Strip the WEBVTT header line and any metadata
        let body = input.trim_start();
        let mut in_header = true;
        for block in body.split("\n\n") {
            if in_header {
                // Skip WEBVTT header block
                in_header = false;
                if block.starts_with("WEBVTT") {
                    continue;
                }
            }
            let lines: Vec<&str> = block.lines().collect();
            if lines.is_empty() {
                continue;
            }
            // Find the timestamp line
            let ts_line_idx = lines
                .iter()
                .position(|l| l.contains("-->"))
                .unwrap_or(0);
            // If first line is a cue identifier (no -->), index is that line
            let cue_id = if ts_line_idx > 0 {
                Some(lines[0].trim().to_string())
            } else {
                None
            };
            let ts_line = lines[ts_line_idx].trim();
            let (start_ms, end_ms) = match parse_vtt_timestamp(ts_line) {
                Some(t) => t,
                None => continue,
            };
            let text = if lines.len() > ts_line_idx + 1 {
                lines[ts_line_idx + 1..].join("\n")
            } else {
                String::new()
            };
            idx += 1;
            let mut meta = HashMap::new();
            if let Some(id) = cue_id {
                meta.insert("cue_id".to_string(), id);
            }
            tiles.push(SubtitleTile {
                id: Uuid::new_v4(),
                index: idx,
                start_ms,
                end_ms,
                text,
                meta,
            });
        }
        tiles
    }

    // ── Output ───────────────────────────────────────────────────────────

    pub fn to_srt(tiles: &[SubtitleTile]) -> String {
        tiles
            .iter()
            .enumerate()
            .map(|(i, t)| {
                format!(
                    "{}\n{} --> {}\n{}",
                    i + 1,
                    fmt_srt_ts(t.start_ms),
                    fmt_srt_ts(t.end_ms),
                    t.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn to_vtt(tiles: &[SubtitleTile]) -> String {
        let body = tiles
            .iter()
            .map(|t| {
                format!(
                    "{}\n{} --> {}\n{}",
                    t.index,
                    fmt_vtt_ts(t.start_ms),
                    fmt_vtt_ts(t.end_ms),
                    t.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        format!("WEBVTT\n\n{body}")
    }

    // ── Transformations ──────────────────────────────────────────────────

    pub fn shift_timing(tiles: &[SubtitleTile], offset_ms: i64) -> Vec<SubtitleTile> {
        tiles
            .iter()
            .map(|t| {
                let start = clamp(t.start_ms as i64 + offset_ms);
                let end = clamp(t.end_ms as i64 + offset_ms);
                SubtitleTile {
                    id: t.id,
                    index: t.index,
                    start_ms: start,
                    end_ms: end,
                    text: t.text.clone(),
                    meta: t.meta.clone(),
                }
            })
            .collect()
    }

    pub fn filter_by_time(tiles: &[SubtitleTile], start_ms: u64, end_ms: u64) -> Vec<SubtitleTile> {
        tiles
            .iter()
            .filter(|t| t.start_ms >= start_ms && t.end_ms <= end_ms)
            .cloned()
            .collect()
    }

    pub fn merge_tiles(tiles: &[SubtitleTile], max_gap_ms: u64) -> Vec<SubtitleTile> {
        if tiles.is_empty() {
            return Vec::new();
        }
        let mut sorted: Vec<&SubtitleTile> = tiles.iter().collect();
        sorted.sort_by_key(|t| t.start_ms);

        let mut result = Vec::new();
        let mut current = sorted[0].clone();
        let mut idx = current.index;

        for tile in &sorted[1..] {
            if tile.start_ms <= current.end_ms + max_gap_ms {
                // Merge: extend time, concatenate text
                current.end_ms = current.end_ms.max(tile.end_ms);
                current.text = format!("{} {}", current.text, tile.text);
            } else {
                idx += 1;
                result.push(std::mem::replace(
                    &mut current,
                    SubtitleTile {
                        id: Uuid::new_v4(),
                        index: idx,
                        start_ms: tile.start_ms,
                        end_ms: tile.end_ms,
                        text: tile.text.clone(),
                        meta: tile.meta.clone(),
                    },
                ));
            }
        }
        result.push(current);
        result
    }

    pub fn split_long(tile: &SubtitleTile, max_chars: usize) -> Vec<SubtitleTile> {
        if tile.text.len() <= max_chars {
            return vec![tile.clone()];
        }
        let words: Vec<&str> = tile.text.split_whitespace().collect();
        let mut tiles = Vec::new();
        let mut line = String::new();
        let mut line_start = tile.start_ms;
        let total_duration = (tile.end_ms - tile.start_ms) as f64;
        let total_chars = tile.text.len() as f64;

        for word in &words {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if candidate.len() > max_chars && !line.is_empty() {
                let frac = line.len() as f64 / total_chars;
                let line_end = line_start + (frac * total_duration) as u64;
                tiles.push(SubtitleTile {
                    id: Uuid::new_v4(),
                    index: tile.index,
                    start_ms: line_start,
                    end_ms: line_end,
                    text: line.clone(),
                    meta: tile.meta.clone(),
                });
                line_start = line_end;
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            tiles.push(SubtitleTile {
                id: Uuid::new_v4(),
                index: tile.index,
                start_ms: line_start,
                end_ms: tile.end_ms,
                text: line,
                meta: tile.meta.clone(),
            });
        }
        tiles
    }

    pub fn duration_seconds(tiles: &[SubtitleTile]) -> f64 {
        if tiles.is_empty() {
            return 0.0;
        }
        let min = tiles.iter().map(|t| t.start_ms).min().unwrap();
        let max = tiles.iter().map(|t| t.end_ms).max().unwrap();
        (max - min) as f64 / 1000.0
    }

    pub fn total_text(tiles: &[SubtitleTile]) -> String {
        tiles.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join(" ")
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn clamp(v: i64) -> u64 {
    v.max(0) as u64
}

fn parse_srt_timestamp(line: &str) -> Option<(u64, u64)> {
    // 00:01:23,456 --> 00:02:34,567
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() != 2 {
        return None;
    }
    Some((ts_to_ms(parts[0].trim(), ','), ts_to_ms(parts[1].trim(), ',')))
}

fn parse_vtt_timestamp(line: &str) -> Option<(u64, u64)> {
    // 00:01:23.456 --> 00:02:34.567
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() != 2 {
        return None;
    }
    Some((ts_to_ms(parts[0].trim(), '.'), ts_to_ms(parts[1].trim(), '.')))
}

fn ts_to_ms(ts: &str, frac_sep: char) -> u64 {
    // Handles HH:MM:SS,mmm and MM:SS,mmm
    let main_frac: Vec<&str> = ts.split(frac_sep).collect();
    let ms: u64 = main_frac.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let time_parts: Vec<u64> = main_frac[0]
        .split(':')
        .filter_map(|s| s.parse().ok())
        .collect();
    match time_parts.len() {
        3 => time_parts[0] * 3600000 + time_parts[1] * 60000 + time_parts[2] * 1000 + ms,
        2 => time_parts[0] * 60000 + time_parts[1] * 1000 + ms,
        _ => ms,
    }
}

fn fmt_srt_ts(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1_000;
    let mil = ms % 1_000;
    format!("{h:02}:{m:02}:{s:02},{mil:03}")
}

fn fmt_vtt_ts(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1_000;
    let mil = ms % 1_000;
    format!("{h:02}:{m:02}:{s:02}.{mil:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_srt() -> &'static str {
        "1\n00:00:01,000 --> 00:00:04,000\nHello, world!\n\n2\n00:00:05,000 --> 00:00:08,000\nThis is a test.\n\n3\n00:00:10,000 --> 00:00:13,000\nThird subtitle line."
    }

    fn sample_vtt() -> &'static str {
        "WEBVTT\n\n1\n00:00:01.000 --> 00:00:04.000\nHello, world!\n\n2\n00:00:05.000 --> 00:00:08.000\nThis is a test."
    }

    // ── Parse ────────────────────────────────────────────────────────

    #[test]
    fn parse_srt_basic() {
        let tiles = SubtitleDecomposer::parse_srt(sample_srt());
        assert_eq!(tiles.len(), 3);
        assert_eq!(tiles[0].index, 1);
        assert_eq!(tiles[0].text, "Hello, world!");
        assert_eq!(tiles[0].start_ms, 1000);
        assert_eq!(tiles[0].end_ms, 4000);
        assert_eq!(tiles[2].text, "Third subtitle line.");
    }

    #[test]
    fn parse_srt_multiline_text() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nLine one\nLine two";
        let tiles = SubtitleDecomposer::parse_srt(srt);
        assert_eq!(tiles[0].text, "Line one\nLine two");
    }

    #[test]
    fn parse_vtt_basic() {
        let tiles = SubtitleDecomposer::parse_vtt(sample_vtt());
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].text, "Hello, world!");
        assert_eq!(tiles[0].start_ms, 1000);
        assert_eq!(tiles[1].end_ms, 8000);
    }

    #[test]
    fn parse_vtt_with_cue_ids() {
        let vtt = "WEBVTT\n\ncue-1\n00:00:01.000 --> 00:00:04.000\nHello";
        let tiles = SubtitleDecomposer::parse_vtt(vtt);
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].meta.get("cue_id").unwrap(), "cue-1");
    }

    #[test]
    fn parse_empty() {
        assert!(SubtitleDecomposer::parse_srt("").is_empty());
        assert!(SubtitleDecomposer::parse_vtt("WEBVTT").is_empty());
    }

    // ── Output ───────────────────────────────────────────────────────

    #[test]
    fn to_srt_roundtrip() {
        let tiles = SubtitleDecomposer::parse_srt(sample_srt());
        let out = SubtitleDecomposer::to_srt(&tiles);
        assert!(out.contains("1\n00:00:01,000 --> 00:00:04,000\nHello, world!"));
        assert!(out.contains("3\n00:00:10,000 --> 00:00:13,000\nThird subtitle line."));
    }

    #[test]
    fn to_vtt_roundtrip() {
        let tiles = SubtitleDecomposer::parse_vtt(sample_vtt());
        let out = SubtitleDecomposer::to_vtt(&tiles);
        assert!(out.starts_with("WEBVTT"));
        assert!(out.contains("00:00:01.000 --> 00:00:04.000"));
    }

    // ── Detection ────────────────────────────────────────────────────

    #[test]
    fn detect_srt() {
        assert_eq!(SubtitleDecomposer::detect_format(sample_srt()), SubFormat::SRT);
    }

    #[test]
    fn detect_vtt() {
        assert_eq!(SubtitleDecomposer::detect_format(sample_vtt()), SubFormat::VTT);
    }

    #[test]
    fn detect_unknown() {
        assert_eq!(SubtitleDecomposer::detect_format("random gibberish"), SubFormat::Unknown);
    }

    // ── Shift timing ─────────────────────────────────────────────────

    #[test]
    fn shift_positive() {
        let tiles = SubtitleDecomposer::parse_srt(sample_srt());
        let shifted = SubtitleDecomposer::shift_timing(&tiles, 5000);
        assert_eq!(shifted[0].start_ms, 6000);
        assert_eq!(shifted[0].end_ms, 9000);
    }

    #[test]
    fn shift_negative_clamp() {
        let tiles = SubtitleDecomposer::parse_srt(sample_srt());
        let shifted = SubtitleDecomposer::shift_timing(&tiles, -2000);
        assert_eq!(shifted[0].start_ms, 0); // 1000-2000 clamped to 0
    }

    // ── Filter ───────────────────────────────────────────────────────

    #[test]
    fn filter_by_time() {
        let tiles = SubtitleDecomposer::parse_srt(sample_srt());
        let filtered = SubtitleDecomposer::filter_by_time(&tiles, 0, 8000);
        assert_eq!(filtered.len(), 2);
    }

    // ── Merge ────────────────────────────────────────────────────────

    #[test]
    fn merge_close_tiles() {
        let tiles = SubtitleDecomposer::parse_srt(sample_srt());
        // gap between tile[0] end=4000 and tile[1] start=5000 is 1000ms
        let merged = SubtitleDecomposer::merge_tiles(&tiles, 1500);
        assert!(merged.len() < tiles.len());
        assert!(merged[0].text.contains("Hello"));
        assert!(merged[0].text.contains("test"));
    }

    // ── Split long ───────────────────────────────────────────────────

    #[test]
    fn split_long_tile() {
        let tile = SubtitleTile {
            id: Uuid::new_v4(),
            index: 1,
            start_ms: 0,
            end_ms: 10000,
            text: "This is a very long subtitle that should definitely be split".to_string(),
            meta: HashMap::new(),
        };
        let parts = SubtitleDecomposer::split_long(&tile, 20);
        assert!(parts.len() > 1);
        for p in &parts {
            assert!(p.text.len() <= 25); // word boundary may slightly exceed
        }
    }

    #[test]
    fn split_short_tile_unchanged() {
        let tile = SubtitleTile {
            id: Uuid::new_v4(),
            index: 1,
            start_ms: 0,
            end_ms: 5000,
            text: "Short".to_string(),
            meta: HashMap::new(),
        };
        let parts = SubtitleDecomposer::split_long(&tile, 100);
        assert_eq!(parts.len(), 1);
    }

    // ── Duration / total text ────────────────────────────────────────

    #[test]
    fn duration_seconds() {
        let tiles = SubtitleDecomposer::parse_srt(sample_srt());
        let dur = SubtitleDecomposer::duration_seconds(&tiles);
        assert!((dur - 12.0).abs() < 0.001); // 1000ms to 13000ms = 12s
    }

    #[test]
    fn duration_empty() {
        assert_eq!(SubtitleDecomposer::duration_seconds(&[]), 0.0);
    }

    #[test]
    fn total_text() {
        let tiles = SubtitleDecomposer::parse_srt(sample_srt());
        let text = SubtitleDecomposer::total_text(&tiles);
        assert!(text.contains("Hello, world!"));
        assert!(text.contains("This is a test."));
        assert!(text.contains("Third subtitle line."));
    }

    #[test]
    fn single_entry() {
        let srt = "1\n00:00:01,500 --> 00:00:03,200\nSolo";
        let tiles = SubtitleDecomposer::parse_srt(srt);
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].start_ms, 1500);
        assert_eq!(tiles[0].end_ms, 3200);
        let out = SubtitleDecomposer::to_srt(&tiles);
        assert!(out.contains("00:00:01,500 --> 00:00:03,200"));
    }

    #[test]
    fn overlapping_times() {
        let srt = "1\n00:00:01,000 --> 00:00:05,000\nFirst\n\n2\n00:00:03,000 --> 00:00:07,000\nOverlapping";
        let tiles = SubtitleDecomposer::parse_srt(srt);
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].end_ms, 5000);
        assert_eq!(tiles[1].start_ms, 3000); // overlap
    }
}
