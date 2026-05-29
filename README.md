# forge-subtitle

**Subtitle decomposition into tiles — the SubForge pattern, generalized.**

This is the spiritual successor to SubForge's media pipeline. SRT/VTT files become tiles, tiles get transformed, tiles become SRT/VTT again.

## The Idea

Subtitles are a perfect candidate for the tile pattern:

1. **Decompose** — Parse SRT or VTT into a flat list of `SubtitleTile` structs
2. **Transform** — Shift timing, filter ranges, merge close gaps, split long lines
3. **Recompose** — Serialize tiles back to SRT or VTT

Each tile is an independent unit with an ID, timing, text, and metadata. Transformations are pure functions that take tiles and return tiles. This makes it trivial to compose operations, parallelize work, or feed tiles into downstream systems.

## Quick Start

```rust
use forge_subtitle::{SubtitleDecomposer, SubFormat};

// Parse
let tiles = SubtitleDecomposer::parse_srt(&srt_content);

// Shift all subtitles 2 seconds forward
let shifted = SubtitleDecomposer::shift_timing(&tiles, 2000);

// Filter to a time range
let clip = SubtitleDecomposer::filter_by_time(&shifted, 5000, 60000);

// Merge subtitles within 1.5s of each other
let merged = SubtitleDecomposer::merge_tiles(&clip, 1500);

// Output
let vtt = SubtitleDecomposer::to_vtt(&merged);
```

## API

### `SubtitleTile`

| Field | Type | Description |
|-------|------|-------------|
| `id` | `Uuid` | Unique identifier |
| `index` | `u32` | Original sequence number |
| `start_ms` | `u64` | Start time in milliseconds |
| `end_ms` | `u64` | End time in milliseconds |
| `text` | `String` | Subtitle text (may be multiline) |
| `meta` | `HashMap<String, String>` | Arbitrary metadata |

### `SubtitleDecomposer`

All methods are associated functions (no state).

| Method | Description |
|--------|-------------|
| `detect_format(input)` | Detect SRT, VTT, or Unknown |
| `parse_srt(input)` | Parse SRT into tiles |
| `parse_vtt(input)` | Parse VTT into tiles |
| `to_srt(tiles)` | Serialize tiles to SRT |
| `to_vtt(tiles)` | Serialize tiles to VTT |
| `shift_timing(tiles, offset_ms)` | Shift all times by offset (can be negative) |
| `filter_by_time(tiles, start, end)` | Keep tiles within a time range |
| `merge_tiles(tiles, max_gap_ms)` | Merge tiles closer than max gap |
| `split_long(tile, max_chars)` | Split a tile's text at word boundaries |
| `duration_seconds(tiles)` | Total span from first start to last end |
| `total_text(tiles)` | Concatenate all tile text |

## Dependencies

- `serde` + `serde_json` — serialization
- `uuid` — unique tile IDs

## License

MIT
