use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    battle_runtime_state::BATTLE_RUNTIME_STATE,
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
    battle_composition_runtime::{
        BattleCompositionRuntimeLayout, CUMULATIVE_RUNTIME_LAYOUT, InstalledDialogueCacheRefresh,
        PROBE_RUNTIME_LAYOUT, composition_dispatch_for_layout,
        match_installed_final_dialogue_cache_refresh,
    },
    battle_text_material::{
        GLYPH_ATLAS_PRG_OFFSET, PHYSICAL_CODE_TABLE_PRG_OFFSET, PROTECTED_ABSTRACT_COLOR_COUNT,
        PROTECTED_ABSTRACT_COLORS_PRG_OFFSET, RECIPE_BLOB_PRG_OFFSET, SOURCE_PAGE_PRG_OFFSET,
    },
};

const FIXED_CPU_WINDOW_START: u16 = 0xC000;
const FIXED_CPU_WINDOW_BYTE_COUNT: usize = 0x4000;
const JSR_BYTE_COUNT: u16 = 3;
const SUPPORTED_RUNTIME_LAYOUTS: [BattleCompositionRuntimeLayout; 2] =
    [PROBE_RUNTIME_LAYOUT, CUMULATIVE_RUNTIME_LAYOUT];
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
    staged_participant_identities: [u8; 2],
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
    composition_path: &'static str,
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
    let installed = bind_installed_battle_composition_paths(&rom)?;
    let runtime = BATTLE_RUNTIME_STATE;
    let selector = runtime.dialogue_selector_projection;
    let selected = select_composition_event(&event_file, installed)?;
    let event = selected.event;
    let internal = snapshot_bytes(event, "nesInternalRam")?;
    let actual_page = snapshot_bytes(event, "nesChrRam")?;
    ensure!(
        actual_page.bytes.len() == FONT_PAGE_SIZE,
        "composition return snapshot does not contain 4 KiB of CHR RAM"
    );
    let observed_dialogue_selector = event_snapshot_byte(
        event,
        "nesMemory",
        usize::from(selector.observed_selector_address),
    )?;
    let (projected_dialogue_selector, selector_62_predicate_matched) = selector
        .project(observed_dialogue_selector, |address| {
            snapshot_byte(&internal, usize::from(address))
        })?;
    let input = BattleRuntimeRecipeInput {
        staged_participant_identities: snapshot_pair(
            &internal,
            runtime.staged_participant_identity_addresses,
        )?,
        class_record_identities: snapshot_pair(&internal, runtime.staged_class_identity_addresses)?,
        item_source_indices: snapshot_pair(&internal, runtime.staged_item_source_index_addresses)?,
        terrain_source_indices: snapshot_pair(
            &internal,
            runtime.staged_terrain_source_index_addresses,
        )?,
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
        schema: 6,
        rom_sha1: sha1_hex(rom.data()),
        composition_path: selected.path.name(),
        compose_return_cpu_address_hex: format!("0x{:04X}", selected.return_address),
        compose_return_frame: event.frame,
        runtime_input: RuntimeInputReport {
            staged_participant_identities: input.staged_participant_identities,
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

#[derive(Debug, Clone, Copy)]
struct InstalledCompositionPaths {
    dispatch_return: u16,
    dialogue_cache_refresh: Option<InstalledDialogueCacheRefresh>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompositionPath {
    BattleLifetimeDispatch,
    DialogueCacheRefresh,
}

impl CompositionPath {
    fn name(self) -> &'static str {
        match self {
            Self::BattleLifetimeDispatch => "battle_lifetime_dispatch",
            Self::DialogueCacheRefresh => "dialogue_cache_refresh",
        }
    }
}

#[derive(Debug)]
struct SelectedCompositionEvent<'a> {
    event: &'a DebugEvent,
    path: CompositionPath,
    return_address: u16,
}

fn bind_installed_battle_composition_paths(rom: &Rom) -> Result<InstalledCompositionPaths> {
    let matches = SUPPORTED_RUNTIME_LAYOUTS
        .into_iter()
        .map(|layout| {
            Ok(match_battle_composition_return(rom.prg(), layout)?
                .map(|dispatch_return| (layout, dispatch_return)))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "ROM binds {} supported battle composition dispatches; expected exactly one",
        matches.len()
    );
    let (layout, dispatch_return) = matches[0];
    Ok(InstalledCompositionPaths {
        dispatch_return,
        dialogue_cache_refresh: match_installed_final_dialogue_cache_refresh(rom, layout)?,
    })
}

fn select_composition_event(
    event_file: &DebugEventFile,
    installed: InstalledCompositionPaths,
) -> Result<SelectedCompositionEvent<'_>> {
    let dispatch_matches = event_file
        .events
        .iter()
        .filter(|event| event.kind == "exec" && event.pc == installed.dispatch_return)
        .collect::<Vec<_>>();
    ensure!(
        dispatch_matches.len() <= 1,
        "debug event has {} battle-lifetime composition return hits at 0x{:04X}; expected at most one",
        dispatch_matches.len(),
        installed.dispatch_return,
    );

    let refresh_match = installed
        .dialogue_cache_refresh
        .map(|refresh| select_dialogue_cache_refresh_event(event_file, refresh))
        .transpose()?
        .flatten();
    let candidate_count =
        usize::from(!dispatch_matches.is_empty()) + usize::from(refresh_match.is_some());
    ensure!(
        candidate_count == 1,
        "debug event contains {candidate_count} complete battle composition paths; expected exactly one"
    );
    if let Some(event) = dispatch_matches.first() {
        return Ok(SelectedCompositionEvent {
            event,
            path: CompositionPath::BattleLifetimeDispatch,
            return_address: installed.dispatch_return,
        });
    }
    let (event, return_address) = refresh_match.context("dialogue cache refresh disappeared")?;
    Ok(SelectedCompositionEvent {
        event,
        path: CompositionPath::DialogueCacheRefresh,
        return_address,
    })
}

fn select_dialogue_cache_refresh_event(
    event_file: &DebugEventFile,
    refresh: InstalledDialogueCacheRefresh,
) -> Result<Option<(&DebugEvent, u16)>> {
    let positions_for = |address| {
        event_file
            .events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| {
                (event.kind == "exec" && event.pc == address).then_some((index, event))
            })
            .collect::<Vec<_>>()
    };
    let refresh_entries = positions_for(refresh.refresh_path_entry);
    let compose_entries = positions_for(refresh.compose_entry);
    let compose_returns = positions_for(refresh.compose_return);
    if refresh_entries.is_empty() && compose_entries.is_empty() && compose_returns.is_empty() {
        return Ok(None);
    }
    ensure!(
        refresh_entries.len() == 1 && compose_entries.len() == 1 && compose_returns.len() == 1,
        "debug event has incomplete dialogue-cache composition sequence: refresh entries {}, compose entries {}, returns {}",
        refresh_entries.len(),
        compose_entries.len(),
        compose_returns.len(),
    );
    ensure!(
        refresh_entries[0].0 < compose_entries[0].0 && compose_entries[0].0 < compose_returns[0].0,
        "dialogue-cache composition events are out of order"
    );
    Ok(Some((compose_returns[0].1, refresh.compose_return)))
}

fn match_battle_composition_return(
    prg: &[u8],
    layout: BattleCompositionRuntimeLayout,
) -> Result<Option<u16>> {
    ensure!(
        prg.len() >= FIXED_CPU_WINDOW_BYTE_COUNT,
        "ROM PRG is smaller than the fixed CPU window"
    );
    let expected = composition_dispatch_for_layout(layout)?;
    let composer_call_offset = find_composer_call_offset(&expected, layout.compose_page)?;
    let fixed_window_start = prg.len() - FIXED_CPU_WINDOW_BYTE_COUNT;
    let dispatch_offset = fixed_window_start
        + usize::from(
            layout
                .dispatch
                .checked_sub(FIXED_CPU_WINDOW_START)
                .context("battle composition dispatch is outside the fixed CPU window")?,
        );
    let actual = prg
        .get(dispatch_offset..dispatch_offset + expected.len())
        .context("battle composition dispatch is outside PRG")?;
    let call_operand = composer_call_offset + 1..composer_call_offset + 3;
    let dispatch_matches = actual
        .iter()
        .zip(expected.iter())
        .enumerate()
        .all(|(index, (actual, expected))| call_operand.contains(&index) || actual == expected);
    if !dispatch_matches {
        return Ok(None);
    }
    let rebound_target = u16::from_le_bytes([
        actual[composer_call_offset + 1],
        actual[composer_call_offset + 2],
    ]);
    if rebound_target < FIXED_CPU_WINDOW_START {
        return Ok(None);
    }
    Ok(Some(
        layout
            .dispatch
            .checked_add(
                u16::try_from(composer_call_offset).context("composer call offset overflow")?,
            )
            .and_then(|address| address.checked_add(JSR_BYTE_COUNT))
            .context("battle composition return address overflow")?,
    ))
}

fn find_composer_call_offset(dispatch: &[u8], composer: u16) -> Result<usize> {
    let [low, high] = composer.to_le_bytes();
    let matches = dispatch
        .windows(3)
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == [0x20, low, high]).then_some(offset))
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "battle composition dispatch contains {} composer calls; expected exactly one",
        matches.len()
    );
    Ok(matches[0])
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

fn snapshot_pair(snapshot: &DecodedSnapshot, addresses: [u16; 2]) -> Result<[u8; 2]> {
    Ok([
        snapshot_byte(snapshot, usize::from(addresses[0]))?,
        snapshot_byte(snapshot, usize::from(addresses[1]))?,
    ])
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

    fn debug_event(frame: u64, pc: u16) -> DebugEvent {
        DebugEvent {
            frame,
            kind: "exec".to_owned(),
            pc,
            snapshot: Vec::new(),
        }
    }

    fn installed_paths(
        dialogue_cache_refresh: Option<InstalledDialogueCacheRefresh>,
    ) -> InstalledCompositionPaths {
        InstalledCompositionPaths {
            dispatch_return: 0xFC4C,
            dialogue_cache_refresh,
        }
    }

    fn prg_with_dispatch(layout: BattleCompositionRuntimeLayout, rebound_composer: u16) -> Vec<u8> {
        let mut prg = vec![0xFF; 512 * 1024];
        let mut dispatch = composition_dispatch_for_layout(layout).unwrap();
        let call_offset = find_composer_call_offset(&dispatch, layout.compose_page).unwrap();
        let [low, high] = rebound_composer.to_le_bytes();
        dispatch[call_offset + 1] = low;
        dispatch[call_offset + 2] = high;
        let fixed_window_start = prg.len() - FIXED_CPU_WINDOW_BYTE_COUNT;
        let offset = fixed_window_start + usize::from(layout.dispatch - FIXED_CPU_WINDOW_START);
        prg[offset..offset + dispatch.len()].copy_from_slice(&dispatch);
        prg
    }

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
        let selector = BATTLE_RUNTIME_STATE.dialogue_selector_projection;
        let mut internal = DecodedSnapshot {
            start: 0,
            bytes: vec![0; 0x0800],
        };
        for address in selector.required_nonzero_addresses {
            internal.bytes[usize::from(address)] = 1;
        }

        assert_eq!(
            selector
                .project(0, |address| snapshot_byte(&internal, usize::from(address)))
                .unwrap(),
            (selector.forced_selector, true)
        );

        internal.bytes[usize::from(selector.required_zero_addresses[0])] = 1;
        assert_eq!(
            selector
                .project(7, |address| snapshot_byte(&internal, usize::from(address)))
                .unwrap(),
            (7, false)
        );
    }

    #[test]
    fn installed_probe_dispatch_binds_its_composition_return() {
        let prg = prg_with_dispatch(PROBE_RUNTIME_LAYOUT, PROBE_RUNTIME_LAYOUT.compose_page);
        let dispatch = composition_dispatch_for_layout(PROBE_RUNTIME_LAYOUT).unwrap();
        let call_offset =
            find_composer_call_offset(&dispatch, PROBE_RUNTIME_LAYOUT.compose_page).unwrap();
        let expected_return =
            PROBE_RUNTIME_LAYOUT.dispatch + u16::try_from(call_offset).unwrap() + JSR_BYTE_COUNT;

        assert_eq!(
            match_battle_composition_return(&prg, PROBE_RUNTIME_LAYOUT).unwrap(),
            Some(expected_return)
        );
        assert_eq!(
            match_battle_composition_return(&prg, CUMULATIVE_RUNTIME_LAYOUT).unwrap(),
            None
        );
    }

    #[test]
    fn integrated_dispatch_binds_relocated_composer_by_its_call_role() {
        let prg = prg_with_dispatch(CUMULATIVE_RUNTIME_LAYOUT, 0xF622);

        assert_eq!(
            match_battle_composition_return(&prg, CUMULATIVE_RUNTIME_LAYOUT).unwrap(),
            Some(0xFC4C)
        );
    }

    #[test]
    fn dispatch_binding_rejects_mutations_outside_the_composer_operand() {
        let mut prg = prg_with_dispatch(CUMULATIVE_RUNTIME_LAYOUT, 0xF622);
        let fixed_window_start = prg.len() - FIXED_CPU_WINDOW_BYTE_COUNT;
        let offset = fixed_window_start
            + usize::from(CUMULATIVE_RUNTIME_LAYOUT.dispatch - FIXED_CPU_WINDOW_START);
        prg[offset] ^= 1;

        assert_eq!(
            match_battle_composition_return(&prg, CUMULATIVE_RUNTIME_LAYOUT).unwrap(),
            None
        );
    }

    #[test]
    fn runtime_verification_selects_one_lifetime_dispatch_return() {
        let event_file = DebugEventFile {
            events: vec![debug_event(27, 0xFC4C)],
        };

        let selected = select_composition_event(&event_file, installed_paths(None)).unwrap();

        assert_eq!(selected.event.frame, 27);
        assert_eq!(selected.path, CompositionPath::BattleLifetimeDispatch);
    }

    #[test]
    fn runtime_verification_selects_a_complete_dialogue_refresh_sequence() {
        let refresh = InstalledDialogueCacheRefresh {
            refresh_path_entry: 0xBF4F,
            compose_entry: 0xFC99,
            compose_return: 0xBF57,
        };
        let event_file = DebugEventFile {
            events: vec![
                debug_event(27, refresh.refresh_path_entry),
                debug_event(27, refresh.compose_entry),
                debug_event(28, refresh.compose_return),
            ],
        };

        let selected =
            select_composition_event(&event_file, installed_paths(Some(refresh))).unwrap();

        assert_eq!(selected.event.frame, 28);
        assert_eq!(selected.path, CompositionPath::DialogueCacheRefresh);
    }

    #[test]
    fn runtime_verification_rejects_a_refresh_return_without_its_compose_entry() {
        let refresh = InstalledDialogueCacheRefresh {
            refresh_path_entry: 0xBF4F,
            compose_entry: 0xFC99,
            compose_return: 0xBF57,
        };
        let event_file = DebugEventFile {
            events: vec![
                debug_event(27, refresh.refresh_path_entry),
                debug_event(28, refresh.compose_return),
            ],
        };

        assert!(
            select_composition_event(&event_file, installed_paths(Some(refresh)))
                .unwrap_err()
                .to_string()
                .contains("incomplete dialogue-cache composition sequence")
        );
    }

    #[test]
    fn runtime_verification_rejects_ambiguous_lifetime_dispatch_returns() {
        let event_file = DebugEventFile {
            events: vec![debug_event(27, 0xFC4C), debug_event(32, 0xFC4C)],
        };

        assert!(
            select_composition_event(&event_file, installed_paths(None))
                .unwrap_err()
                .to_string()
                .contains("expected at most one")
        );
    }
}
