use super::*;

pub(super) fn summarize_page(page_index: usize, page: &[u8]) -> PageReport {
    let tiles: Vec<&[u8]> = page.chunks_exact(TILE_SIZE).collect();
    let mut blank_pattern_codes = Vec::new();
    let mut low_plane_only_count = 0;
    let mut high_plane_only_count = 0;
    let mut dual_plane_count = 0;
    let mut patterns = BTreeSet::new();

    for (code, tile) in tiles.iter().enumerate() {
        patterns.insert((*tile).to_vec());
        match plane_usage(tile) {
            PlaneUsage::Blank => blank_pattern_codes.push(format!("{code:02X}")),
            PlaneUsage::LowOnly => low_plane_only_count += 1,
            PlaneUsage::HighOnly => high_plane_only_count += 1,
            PlaneUsage::Dual => dual_plane_count += 1,
        }
    }

    PageReport {
        page_index,
        chr_offset: page_index * CHR_PAGE_SIZE,
        chr_offset_hex: format!("0x{:05X}", page_index * CHR_PAGE_SIZE),
        sha1: sha1_hex(page),
        nonblank_tile_count: TILES_PER_PAGE - blank_pattern_codes.len(),
        blank_pattern_count: blank_pattern_codes.len(),
        low_plane_only_count,
        high_plane_only_count,
        dual_plane_count,
        distinct_pattern_count: patterns.len(),
        blank_pattern_codes,
    }
}

pub(super) fn describe_font_page(page: &[u8]) -> Vec<SlotReport> {
    let tiles: Vec<&[u8]> = page.chunks_exact(TILE_SIZE).collect();
    tiles
        .iter()
        .enumerate()
        .map(|(code, tile)| {
            let code = code as u8;
            let reference_occurrences: Vec<ReferenceOccurrence> = KNOWN_REFERENCES
                .iter()
                .filter_map(|reference| {
                    let count = reference
                        .expected
                        .iter()
                        .filter(|value| **value == code)
                        .count();
                    (count > 0).then_some(ReferenceOccurrence {
                        reference_id: reference.id,
                        count,
                        scope: reference.scope,
                    })
                })
                .collect();
            let preserved_reference = reference_occurrences
                .iter()
                .any(|occurrence| occurrence.scope == ReferenceScope::PreservedOriginal);
            let is_preserved_glyph = is_declared_preserved_glyph(code);
            let is_control = [ENTRY_SEPARATOR, TABLE_TERMINATOR].contains(&code);
            let is_latch = LATCH_TRIGGER_CODES.contains(&code);

            let mut code_assignment_reasons = Vec::new();
            if is_preserved_glyph {
                code_assignment_reasons
                    .push("declared original digit, Latin, or attached punctuation");
            }
            if preserved_reference {
                code_assignment_reasons.push("confirmed preserved-original table reference");
            }
            if is_control {
                code_assignment_reasons.push("confirmed text control code");
            }
            if is_latch {
                code_assignment_reasons.push("MMC4 tile-fetch latch code");
            }
            let code_assignment = if code_assignment_reasons.is_empty() {
                code_assignment_reasons.push("consumer population is incomplete");
                Decision::Unresolved
            } else {
                Decision::Protected
            };

            let mut tile_reuse_reasons = Vec::new();
            if is_preserved_glyph || preserved_reference {
                tile_reuse_reasons.push("preserved original display depends on this tile");
            }
            if is_latch {
                tile_reuse_reasons.push("MMC4 latch behavior reserves this tile code");
            }
            let tile_reuse = if tile_reuse_reasons.is_empty() {
                if plane_usage(tile) == PlaneUsage::Blank {
                    tile_reuse_reasons.push("blank pattern is not free-space proof");
                } else {
                    tile_reuse_reasons.push("all tile consumers have not been excluded");
                }
                Decision::Unresolved
            } else {
                Decision::Protected
            };

            let matching_codes = tiles
                .iter()
                .enumerate()
                .filter(|(other_code, other_tile)| {
                    *other_code != code as usize && *other_tile == tile
                })
                .map(|(other_code, _)| format!("{other_code:02X}"))
                .collect();
            let chr_offset = code as usize * TILE_SIZE;

            SlotReport {
                code,
                code_hex: format!("{code:02X}"),
                chr_offset,
                chr_offset_hex: format!("0x{chr_offset:05X}"),
                tile_sha1: sha1_hex(tile),
                plane_usage: plane_usage(tile),
                nonzero_pixel_count: nonzero_pixel_count(tile),
                declared_glyph: declared_glyph(code),
                reference_occurrences,
                matching_codes,
                code_assignment,
                code_assignment_reasons,
                tile_reuse,
                tile_reuse_reasons,
            }
        })
        .collect()
}

pub(super) fn is_declared_preserved_glyph(code: u8) -> bool {
    PRESERVED_DISPLAY_CODES.contains(&code)
}

pub(super) fn declared_glyph(code: u8) -> Option<String> {
    match code {
        0x60..=0x69 => Some(char::from(b'0' + code - 0x60).to_string()),
        0x6A..=0x83 => Some(char::from(b'A' + code - 0x6A).to_string()),
        0x8D => Some(":".to_owned()),
        0x9B => Some(".".to_owned()),
        0x0F => Some("゛".to_owned()),
        0x30 => Some("ア".to_owned()),
        0x31 => Some("イ".to_owned()),
        0x32 => Some("ウ".to_owned()),
        0x33 => Some("エ".to_owned()),
        0x3A => Some("サ".to_owned()),
        0x3B => Some("シ".to_owned()),
        0x3F => Some("ー".to_owned()),
        0x40 => Some("タ".to_owned()),
        0x44 => Some("ト".to_owned()),
        0x46 => Some("ニ".to_owned()),
        0x50 => Some("マ".to_owned()),
        0x53 => Some("メ".to_owned()),
        0x5F => Some("ン".to_owned()),
        0x8B => Some("ョ".to_owned()),
        _ => None,
    }
}

pub(super) fn plane_usage(tile: &[u8]) -> PlaneUsage {
    let low = tile[..8].iter().any(|byte| *byte != 0);
    let high = tile[8..].iter().any(|byte| *byte != 0);
    match (low, high) {
        (false, false) => PlaneUsage::Blank,
        (true, false) => PlaneUsage::LowOnly,
        (false, true) => PlaneUsage::HighOnly,
        (true, true) => PlaneUsage::Dual,
    }
}

pub(super) fn nonzero_pixel_count(tile: &[u8]) -> u32 {
    tile[..8]
        .iter()
        .zip(&tile[8..])
        .map(|(low, high)| (low | high).count_ones())
        .sum()
}
