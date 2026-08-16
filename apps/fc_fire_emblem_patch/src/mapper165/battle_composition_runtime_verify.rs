use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE},
    rom::Rom,
    runtime_storage_layout::{BATTLE_REMAP_PAIR_TABLE_START, BATTLE_REMAP_STATE_ADDRESS},
    sha1_hex,
};

use super::{
    OUTPUT_MAPPER,
    battle_codebook_plan::{
        BattleRuntimeRecipeInput, compose_runtime_font_page, inspect_runtime_recipe_input,
    },
    battle_text_cache_probe::{
        GLYPH_ATLAS_PRG_OFFSET, PHYSICAL_CODE_TABLE_PRG_OFFSET, PROTECTED_ABSTRACT_COLOR_COUNT,
        PROTECTED_ABSTRACT_COLORS_PRG_OFFSET, RECIPE_BLOB_PRG_OFFSET, SOURCE_PAGE_PRG_OFFSET,
    },
};

const COMPOSE_RETURN_ADDRESS: u16 = 0xFB26;
const INTERNAL_BATTLE_FIELD_START: usize = 0x0304;
const INTERNAL_BATTLE_FIELD_END_EXCLUSIVE: usize = 0x0324;
const OBSERVED_DIALOGUE_SELECTOR_ADDRESS: usize = 0x7936;
const SELECTOR_62_REQUIRED_NONZERO_ADDRESSES: [usize; 3] = [0x0334, 0x0479, 0x0335];
const SELECTOR_62_REQUIRED_ZERO_ADDRESS: usize = 0x05DF;
const SELECTOR_62_VALUE: u8 = 0x3E;
const REMAP_STATE_ADDRESS: usize = BATTLE_REMAP_STATE_ADDRESS as usize;
const CACHE_UPLOADED_MARKER: u8 = 0x80;
const REMAP_PAIR_COUNT_MASK: u8 = 0x1E;
const REMAP_PAIR_TABLE_ADDRESS: usize = BATTLE_REMAP_PAIR_TABLE_START as usize;
const RECIPE_HEADER_BYTE_COUNT: usize = 32;
const RECIPE_MAGIC: &[u8; 4] = b"FBRC";

#[derive(Debug, Deserialize)]
struct DebugEventFile {
    events: Vec<DebugEvent>,
}

#[derive(Debug, Deserialize)]
struct DebugEvent {
    frame: u64,
    kind: String,
    pc: u16,
    snapshot: Vec<MemorySnapshot>,
}

#[derive(Debug, Deserialize)]
struct MemorySnapshot {
    address: usize,
    hex: String,
    memory_type: String,
}

#[derive(Debug, Serialize)]
struct RuntimeInputReport {
    participant_record_identities: [u8; 2],
    class_record_identities: [u8; 2],
    item_source_indices: [u8; 2],
    terrain_source_indices: [u8; 2],
    observed_dialogue_selector: u8,
    projected_dialogue_selector: u8,
    selector_62_predicate_matched: bool,
}

#[derive(Debug, Serialize)]
struct DifferingTileReport {
    physical_code: u8,
    abstract_color: Option<u8>,
    expected_atlas_index: Option<u16>,
    actual_atlas_index: Option<u16>,
    actual_matches_source_tile: bool,
    expected_hex: String,
    actual_hex: String,
}

#[derive(Debug, Serialize)]
struct BattleCompositionRuntimeVerificationReport {
    schema: u8,
    rom_sha1: String,
    compose_return_cpu_address_hex: String,
    compose_return_frame: u64,
    runtime_input: RuntimeInputReport,
    selected_recipe_offsets_hex: Vec<String>,
    selected_unique_overlay_count: usize,
    selected_glyph_reference_count: usize,
    remap_state_hex: String,
    cache_uploaded_marker_present: bool,
    dynamic_assignment_sha1: String,
    expected_remap_pairs_hex: Vec<String>,
    actual_remap_pairs_hex: Vec<String>,
    exact_remap_match: bool,
    expected_chr_ram_sha1: String,
    actual_chr_ram_sha1: String,
    differing_byte_count: usize,
    differing_tile_count: usize,
    differing_tiles: Vec<DifferingTileReport>,
    first_differing_ppu_address_hex: Option<String>,
    exact_composition_match: bool,
    runtime_verified: bool,
    release_eligible: bool,
    next_gate: &'static str,
}

pub(crate) struct BattleCompositionRuntimeVerificationSummary {
    pub(crate) report_sha1: String,
    pub(crate) expected_chr_ram_sha1: String,
    pub(crate) actual_chr_ram_sha1: String,
    pub(crate) differing_byte_count: usize,
    pub(crate) differing_tile_count: usize,
}

pub(crate) fn verify_battle_composition_runtime(
    rom_path: &Path,
    event_path: &Path,
    report_path: &Path,
) -> Result<BattleCompositionRuntimeVerificationSummary> {
    let rom = Rom::from_path(rom_path)?;
    ensure!(
        rom.mapper() == OUTPUT_MAPPER,
        "battle composition runtime verifier requires mapper 165"
    );
    let event_bytes = fs::read(event_path)
        .with_context(|| format!("read battle composition event {}", event_path.display()))?;
    let event_file: DebugEventFile = serde_json::from_slice(&event_bytes)
        .with_context(|| format!("parse battle composition event {}", event_path.display()))?;
    let event = event_file
        .events
        .iter()
        .find(|event| event.kind == "exec" && event.pc == COMPOSE_RETURN_ADDRESS)
        .context("debug event has no battle composition return hit at 0xFB26")?;
    let internal = snapshot_bytes(event, "nesInternalRam")?;
    let actual_page = snapshot_bytes(event, "nesChrRam")?;
    ensure!(
        actual_page.bytes.len() == FONT_PAGE_SIZE,
        "composition return snapshot does not contain 4 KiB of CHR RAM"
    );
    let observed_dialogue_selector =
        event_snapshot_byte(event, "nesMemory", OBSERVED_DIALOGUE_SELECTOR_ADDRESS)?;
    let (projected_dialogue_selector, selector_62_predicate_matched) =
        project_dialogue_selector(&internal, observed_dialogue_selector)?;
    let input = BattleRuntimeRecipeInput {
        participant_record_identities: [
            snapshot_byte(&internal, INTERNAL_BATTLE_FIELD_START)?,
            snapshot_byte(&internal, INTERNAL_BATTLE_FIELD_START + 1)?,
        ],
        class_record_identities: [
            snapshot_byte(&internal, INTERNAL_BATTLE_FIELD_START + 2)?,
            snapshot_byte(&internal, INTERNAL_BATTLE_FIELD_START + 3)?,
        ],
        item_source_indices: [
            snapshot_byte(&internal, INTERNAL_BATTLE_FIELD_END_EXCLUSIVE - 4)?,
            snapshot_byte(&internal, INTERNAL_BATTLE_FIELD_END_EXCLUSIVE - 3)?,
        ],
        terrain_source_indices: [
            snapshot_byte(&internal, INTERNAL_BATTLE_FIELD_END_EXCLUSIVE - 2)?,
            snapshot_byte(&internal, INTERNAL_BATTLE_FIELD_END_EXCLUSIVE - 1)?,
        ],
        dialogue_selector: projected_dialogue_selector,
    };
    let remap_state = snapshot_byte(&internal, REMAP_STATE_ADDRESS)?;
    let cache_uploaded_marker_present = remap_state & CACHE_UPLOADED_MARKER != 0;

    let prg = rom.prg();
    let recipe_header = prg
        .get(RECIPE_BLOB_PRG_OFFSET..RECIPE_BLOB_PRG_OFFSET + RECIPE_HEADER_BYTE_COUNT)
        .context("battle recipe header is outside PRG")?;
    ensure!(
        &recipe_header[..RECIPE_MAGIC.len()] == RECIPE_MAGIC,
        "battle recipe magic changed"
    );
    let abstract_color_count = usize::from(recipe_header[5]);
    let atlas_tile_count = usize::from(u16::from_le_bytes([recipe_header[6], recipe_header[7]]));
    let recipe_byte_count = usize::from(u16::from_le_bytes([recipe_header[8], recipe_header[9]]));
    let glyph_atlas = prg
        .get(GLYPH_ATLAS_PRG_OFFSET..GLYPH_ATLAS_PRG_OFFSET + atlas_tile_count * FONT_TILE_SIZE)
        .context("battle glyph atlas is outside PRG")?;
    let canonical_color_codes = prg
        .get(PHYSICAL_CODE_TABLE_PRG_OFFSET..PHYSICAL_CODE_TABLE_PRG_OFFSET + abstract_color_count)
        .context("battle canonical code table is outside PRG")?;
    let protected_abstract_colors = prg
        .get(
            PROTECTED_ABSTRACT_COLORS_PRG_OFFSET
                ..PROTECTED_ABSTRACT_COLORS_PRG_OFFSET + PROTECTED_ABSTRACT_COLOR_COUNT,
        )
        .context("battle protected abstract-color list is outside PRG")?;
    let source_page = prg
        .get(SOURCE_PAGE_PRG_OFFSET..SOURCE_PAGE_PRG_OFFSET + FONT_PAGE_SIZE)
        .context("battle source font page is outside PRG")?;
    let recipe_blob = prg
        .get(RECIPE_BLOB_PRG_OFFSET..RECIPE_BLOB_PRG_OFFSET + recipe_byte_count)
        .context("battle recipe blob is outside PRG")?;
    let expected_composition = compose_runtime_font_page(
        source_page,
        glyph_atlas,
        canonical_color_codes,
        protected_abstract_colors,
        recipe_blob,
        input,
    )?;
    let expected_page = &expected_composition.page;
    let actual_remap_pair_count = usize::from(remap_state & REMAP_PAIR_COUNT_MASK) / 2;
    let actual_remap_pairs = (0..actual_remap_pair_count)
        .map(|index| {
            Ok((
                snapshot_byte(&internal, REMAP_PAIR_TABLE_ADDRESS + index * 2)?,
                snapshot_byte(&internal, REMAP_PAIR_TABLE_ADDRESS + index * 2 + 1)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let exact_remap_match = actual_remap_pairs == expected_composition.remap_pairs;
    let selection = inspect_runtime_recipe_input(recipe_blob, input)?;
    let differing_byte_count = expected_page
        .iter()
        .zip(actual_page.bytes.iter())
        .filter(|(expected, actual)| expected != actual)
        .count();
    let differing_tile_count = expected_page
        .chunks_exact(FONT_TILE_SIZE)
        .zip(actual_page.bytes.chunks_exact(FONT_TILE_SIZE))
        .filter(|(expected, actual)| expected != actual)
        .count();
    let differing_tiles = expected_page
        .chunks_exact(FONT_TILE_SIZE)
        .zip(actual_page.bytes.chunks_exact(FONT_TILE_SIZE))
        .zip(source_page.chunks_exact(FONT_TILE_SIZE))
        .enumerate()
        .filter(|(_, ((expected, actual), _))| expected != actual)
        .map(
            |(physical_code, ((expected, actual), source))| DifferingTileReport {
                physical_code: u8::try_from(physical_code).expect("font page tile code fits u8"),
                abstract_color: expected_composition
                    .color_codes
                    .iter()
                    .find(|(_, code)| usize::from(**code) == physical_code)
                    .map(|(color, _)| *color),
                expected_atlas_index: atlas_tile_index(glyph_atlas, expected),
                actual_atlas_index: atlas_tile_index(glyph_atlas, actual),
                actual_matches_source_tile: actual == source,
                expected_hex: encode_hex(expected),
                actual_hex: encode_hex(actual),
            },
        )
        .collect();
    let first_differing_ppu_address_hex = expected_page
        .iter()
        .zip(actual_page.bytes.iter())
        .position(|(expected, actual)| expected != actual)
        .map(|offset| format!("0x{:04X}", 0x1000 + offset));
    let exact_composition_match = differing_byte_count == 0;
    let report = BattleCompositionRuntimeVerificationReport {
        schema: 5,
        rom_sha1: sha1_hex(rom.data()),
        compose_return_cpu_address_hex: format!("0x{COMPOSE_RETURN_ADDRESS:04X}"),
        compose_return_frame: event.frame,
        runtime_input: RuntimeInputReport {
            participant_record_identities: input.participant_record_identities,
            class_record_identities: input.class_record_identities,
            item_source_indices: input.item_source_indices,
            terrain_source_indices: input.terrain_source_indices,
            observed_dialogue_selector,
            projected_dialogue_selector,
            selector_62_predicate_matched,
        },
        selected_recipe_offsets_hex: selection
            .recipe_offsets
            .iter()
            .map(|offset| format!("0x{offset:04X}"))
            .collect(),
        selected_unique_overlay_count: selection.unique_overlay_count,
        selected_glyph_reference_count: selection.glyph_reference_count,
        remap_state_hex: format!("0x{remap_state:02X}"),
        cache_uploaded_marker_present,
        dynamic_assignment_sha1: expected_composition.assignment_sha1,
        expected_remap_pairs_hex: encode_pairs(&expected_composition.remap_pairs),
        actual_remap_pairs_hex: encode_pairs(&actual_remap_pairs),
        exact_remap_match,
        expected_chr_ram_sha1: sha1_hex(expected_page),
        actual_chr_ram_sha1: sha1_hex(&actual_page.bytes),
        differing_byte_count,
        differing_tile_count,
        differing_tiles,
        first_differing_ppu_address_hex,
        exact_composition_match,
        runtime_verified: false,
        release_eligible: false,
        next_gate: "verify temporal battle rendering across varied inputs and automatic exit restoration",
    };
    let report_bytes = serde_json::to_vec_pretty(&report)?;
    write_file(report_path, &report_bytes)?;
    let summary = BattleCompositionRuntimeVerificationSummary {
        report_sha1: sha1_hex(&report_bytes),
        expected_chr_ram_sha1: report.expected_chr_ram_sha1,
        actual_chr_ram_sha1: report.actual_chr_ram_sha1,
        differing_byte_count,
        differing_tile_count,
    };
    ensure!(
        cache_uploaded_marker_present,
        "composition return snapshot does not have the uploaded-cache marker"
    );
    ensure!(
        exact_remap_match,
        "runtime remap pairs differ from the independently planned dynamic assignment"
    );
    ensure!(
        exact_composition_match,
        "runtime CHR RAM differs from the independently composed page in {differing_byte_count} bytes across {differing_tile_count} tiles"
    );
    Ok(summary)
}

struct DecodedSnapshot {
    start: usize,
    bytes: Vec<u8>,
}

fn snapshot_bytes(event: &DebugEvent, memory_type: &str) -> Result<DecodedSnapshot> {
    let snapshot = event
        .snapshot
        .iter()
        .find(|snapshot| snapshot.memory_type == memory_type)
        .with_context(|| format!("composition return event has no {memory_type} snapshot"))?;
    Ok(DecodedSnapshot {
        start: snapshot.address,
        bytes: decode_hex(&snapshot.hex)
            .with_context(|| format!("decode {memory_type} snapshot"))?,
    })
}

fn snapshot_byte(snapshot: &DecodedSnapshot, address: usize) -> Result<u8> {
    snapshot
        .bytes
        .get(
            address
                .checked_sub(snapshot.start)
                .context("snapshot address precedes captured range")?,
        )
        .copied()
        .with_context(|| format!("snapshot does not contain address 0x{address:04X}"))
}

fn event_snapshot_byte(event: &DebugEvent, memory_type: &str, address: usize) -> Result<u8> {
    for snapshot in event
        .snapshot
        .iter()
        .filter(|snapshot| snapshot.memory_type == memory_type)
    {
        let decoded = DecodedSnapshot {
            start: snapshot.address,
            bytes: decode_hex(&snapshot.hex)
                .with_context(|| format!("decode {memory_type} snapshot"))?,
        };
        if let Ok(byte) = snapshot_byte(&decoded, address) {
            return Ok(byte);
        }
    }
    anyhow::bail!("composition return event has no {memory_type} byte at 0x{address:04X}")
}

fn project_dialogue_selector(
    internal: &DecodedSnapshot,
    observed_selector: u8,
) -> Result<(u8, bool)> {
    let predicate_matched = SELECTOR_62_REQUIRED_NONZERO_ADDRESSES
        .into_iter()
        .map(|address| snapshot_byte(internal, address))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .all(|value| value != 0)
        && snapshot_byte(internal, SELECTOR_62_REQUIRED_ZERO_ADDRESS)? == 0;
    Ok((
        if predicate_matched {
            SELECTOR_62_VALUE
        } else {
            observed_selector
        },
        predicate_matched,
    ))
}

fn decode_hex(encoded: &str) -> Result<Vec<u8>> {
    ensure!(
        encoded.len().is_multiple_of(2),
        "hex snapshot has an odd length"
    );
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).context("hex snapshot is not UTF-8")?;
            u8::from_str_radix(text, 16).context("hex snapshot contains a non-hex byte")
        })
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn encode_pairs(pairs: &[(u8, u8)]) -> Vec<String> {
    pairs
        .iter()
        .map(|(canonical, target)| format!("{canonical:02X}:{target:02X}"))
        .collect()
}

fn atlas_tile_index(glyph_atlas: &[u8], tile: &[u8]) -> Option<u16> {
    glyph_atlas
        .chunks_exact(FONT_TILE_SIZE)
        .position(|candidate| candidate == tile)
        .map(|index| u16::try_from(index).expect("battle glyph atlas index fits u16"))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_addressing_uses_the_declared_capture_origin() {
        let snapshot = DecodedSnapshot {
            start: 0x300,
            bytes: vec![0x11, 0x22, 0x33],
        };

        assert_eq!(snapshot_byte(&snapshot, 0x302).unwrap(), 0x33);
    }

    #[test]
    fn malformed_hex_snapshots_fail_closed() {
        assert!(decode_hex("0").is_err());
        assert!(decode_hex("gg").is_err());
    }

    #[test]
    fn dialogue_selector_projection_matches_the_source_predicate() {
        let mut internal = DecodedSnapshot {
            start: 0,
            bytes: vec![0; 0x0800],
        };
        for address in SELECTOR_62_REQUIRED_NONZERO_ADDRESSES {
            internal.bytes[address] = 1;
        }

        assert_eq!(
            project_dialogue_selector(&internal, 0).unwrap(),
            (SELECTOR_62_VALUE, true)
        );

        internal.bytes[SELECTOR_62_REQUIRED_ZERO_ADDRESS] = 1;
        assert_eq!(project_dialogue_selector(&internal, 7).unwrap(), (7, false));
    }
}
