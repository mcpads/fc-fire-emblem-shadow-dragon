use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::EncodedMainDialogueDisplayStorage, rom::Rom, sha1_hex, tracked::TrackedImage,
};

mod transition_reader;
mod write_set;

use transition_reader::{
    NMI_PRG_BANK_RESTORE_SELECTOR, SELECTOR_CAVE_END_EXCLUSIVE, SELECTOR_CAVE_START,
    SOURCE_POINTER_RESOLVER, TRANSITION_BANK_MARKER, TRANSITION_BANK_RESTORE,
    TRANSITION_BANK_SELECTOR, TRANSITION_POINTER_RESOLVER, assemble_transition_reader_routines,
};
use write_set::{CompleteDialogueWriteSetPlan, validate_complete_dialogue_write_set};

const EXPECTED_MAPPER: u16 = 165;
const EXPANDED_PRG_SIZE: usize = 512 * 1024;
pub(super) const PRG_BANK_SIZE: usize = 16 * 1024;
const SOURCE_DIALOGUE_BANK: u8 = 0x0A;
const TRANSITION_SOURCE_BANKS: [u8; 5] = [0x04, 0x07, 0x08, 0x0B, 0x0C];
pub(super) const TRANSITION_MIRROR_BANKS: [u8; 5] = [0x11, 0x12, 0x13, 0x14, 0x15];
pub(super) const BATTLE_MATERIAL_BANK: u8 = 0x10;
pub(super) const ACTIVE_FIXED_BANK: u8 = 0x1F;
const DIALOGUE_BYTE_BANK_SELECT_CALL: u16 = 0xE6A1;
const DIALOGUE_BYTE_BANK_RESTORE_CALL: u16 = 0xE6AB;
const SOURCE_PRG_SELECTOR: u16 = 0xFA20;
const NMI_AUDIO_BANK_DISPATCH: u16 = 0xC1FB;
const NMI_AUDIO_BANK_RESTORE_CALL: u16 = 0xC205;
const NMI_AUDIO_BANK_DISPATCH_CODE: [u8; 14] = [
    0xA9, 0x0E, 0x20, 0x20, 0xFA, 0x20, 0x00, 0x80, 0xA5, 0x29, 0x20, 0x20, 0xFA, 0x60,
];
const TRANSITION_POINTER_RESOLVER_CALLS: [u16; 2] = [0x85F8, 0x865F];

pub(super) fn transition_reader_reserved_range() -> Result<std::ops::Range<u16>> {
    let routines = assemble_transition_reader_routines()?;
    let end = NMI_PRG_BANK_RESTORE_SELECTOR
        .checked_add(
            u16::try_from(routines.nmi_bank_restore_selector.len())
                .context("NMI PRG bank restore selector length does not fit u16")?,
        )
        .context("NMI PRG bank restore selector range overflow")?;
    Ok(TRANSITION_POINTER_RESOLVER..end)
}

impl RelocatedDialogueBankPlan {
    pub(super) fn expected_write_count(&self) -> usize {
        self.complete_dialogue_write_set.expected_write_count
    }
}

pub(super) fn append_relocated_dialogue_writes(
    image: &mut TrackedImage,
    candidate: &Rom,
    storage: &EncodedMainDialogueDisplayStorage,
) -> Result<()> {
    let routines = assemble_transition_reader_routines()?;
    write_set::append_complete_dialogue_writes(image, candidate, storage, &routines)
}

#[derive(Serialize)]
pub(super) struct RelocatedDialogueBankPlan {
    pub(super) strategy_selected: bool,
    strategy: &'static str,
    current_candidate_mapper: u16,
    current_candidate_prg_size: usize,
    battle_material_bank: u8,
    active_fixed_bank: u8,
    transition_bank_marker: u8,
    transition_bank_marker_hex: String,
    mappings: Vec<TransitionMirrorBankMapping>,
    direct_source_region_count: usize,
    direct_storage_byte_count: usize,
    canonical_pointer_write_count: usize,
    normalized_record_count: usize,
    transition_payload_byte_count: usize,
    all_selected_banks_are_exact_ff: bool,
    dialogue_byte_bank_select_call_cpu_address_hex: String,
    dialogue_byte_bank_restore_call_cpu_address_hex: String,
    source_prg_selector_cpu_address_hex: String,
    source_selector_masks_to_low_nibble: bool,
    source_selector_entry_preserved: bool,
    nmi_audio_bank_dispatch_sha1: String,
    nmi_audio_restore_uses_active_bank_shadow: bool,
    nmi_audio_bank_restore_call_cpu_address_hex: String,
    nmi_audio_bank_restore_call_hooked: bool,
    transition_pointer_resolver_call_cpu_addresses_hex: Vec<String>,
    transition_pointer_resolver_cpu_address_hex: String,
    transition_pointer_resolver_byte_count: usize,
    transition_pointer_resolver_sha1: String,
    transition_bank_selector_cpu_address_hex: String,
    transition_bank_selector_byte_count: usize,
    transition_bank_selector_sha1: String,
    transition_bank_restore_cpu_address_hex: String,
    transition_bank_restore_byte_count: usize,
    transition_bank_restore_sha1: String,
    nmi_prg_bank_restore_selector_cpu_address_hex: String,
    nmi_prg_bank_restore_selector_byte_count: usize,
    nmi_prg_bank_restore_selector_sha1: String,
    selector_cave_cpu_start_hex: String,
    selector_cave_cpu_end_exclusive_hex: String,
    selector_cave_byte_count: usize,
    selector_cave_sha1: String,
    selector_cave_is_exact_ff: bool,
    canonical_pointer_binding_planned: bool,
    transition_operands_preserved: bool,
    transition_mode_hooks_planned: bool,
    nmi_restorable_reader_selection_assembled: bool,
    complete_dialogue_write_set: CompleteDialogueWriteSetPlan,
    writes_installed: bool,
}

#[derive(Serialize)]
struct TransitionMirrorBankMapping {
    source_prg_bank: u8,
    source_prg_bank_hex: String,
    transition_prg_bank: u8,
    transition_prg_bank_hex: String,
    first_mmc3_page: u8,
    second_mmc3_page: u8,
    record_count: usize,
    payload_byte_count: usize,
    source_preserved_byte_count: usize,
    source_bank_sha1: String,
    material_byte_count: usize,
    material_sha1: String,
    non_payload_bytes_match_source: bool,
    nmi_directory_source_sha1: String,
    nmi_directory_mirror_sha1: String,
    nmi_directory_matches_source: bool,
    candidate_bank_sha1: String,
    candidate_bank_is_exact_ff: bool,
}

pub(super) fn plan_relocated_dialogue_banks(
    source: &Rom,
    candidate: &Rom,
    storage: &EncodedMainDialogueDisplayStorage,
) -> Result<RelocatedDialogueBankPlan> {
    ensure!(
        candidate.mapper() == EXPECTED_MAPPER && candidate.prg().len() == EXPANDED_PRG_SIZE,
        "transition mirror banks require the current 512 KiB mapper 165 candidate"
    );
    ensure!(
        TRANSITION_MIRROR_BANKS
            .iter()
            .all(|bank| *bank != BATTLE_MATERIAL_BANK && *bank != ACTIVE_FIXED_BANK),
        "transition mirror banks overlap reserved expanded PRG banks"
    );
    let mirrors = storage
        .transition_mirrors
        .iter()
        .map(|mirror| (mirror.source_prg_bank, mirror))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        mirrors.keys().copied().collect::<Vec<_>>() == TRANSITION_SOURCE_BANKS,
        "transition mirror source-bank population changed"
    );
    let mappings = TRANSITION_SOURCE_BANKS
        .into_iter()
        .zip(TRANSITION_MIRROR_BANKS)
        .map(|(source_prg_bank, transition_prg_bank)| {
            let mirror = mirrors[&source_prg_bank];
            ensure!(
                mirror.material.len() == PRG_BANK_SIZE,
                "source dialogue bank {source_prg_bank:02X} transition mirror is not 16 KiB"
            );
            let source_bank = prg_bank(source, source_prg_bank)?;
            let payload_byte_count = mirror
                .payload_ranges
                .iter()
                .map(|range| range.len())
                .sum::<usize>();
            ensure!(
                payload_byte_count == mirror.payload_byte_count,
                "transition mirror payload ranges changed"
            );
            let non_payload_bytes_match_source = source_bank
                .iter()
                .zip(&mirror.material)
                .enumerate()
                .all(|(offset, (source_byte, mirror_byte))| {
                    mirror
                        .payload_ranges
                        .iter()
                        .any(|range| range.contains(&offset))
                        || source_byte == mirror_byte
                });
            ensure!(
                non_payload_bytes_match_source,
                "transition mirror {source_prg_bank:02X} changes bytes outside its dialogue payloads"
            );
            let nmi_directory_offset = usize::from(0xBFC0_u16 - 0x8000);
            let nmi_directory_source = &source_bank[nmi_directory_offset..nmi_directory_offset + 2];
            let nmi_directory_mirror =
                &mirror.material[nmi_directory_offset..nmi_directory_offset + 2];
            let nmi_directory_matches_source = nmi_directory_source == nmi_directory_mirror;
            ensure!(
                nmi_directory_matches_source,
                "transition mirror {source_prg_bank:02X} does not preserve its bank-local NMI directory"
            );
            let candidate_bank = prg_bank(candidate, transition_prg_bank)?;
            let exact_ff = candidate_bank.iter().all(|byte| *byte == 0xFF);
            ensure!(
                exact_ff,
                "selected transition PRG bank {transition_prg_bank:02X} is not empty"
            );
            let first_mmc3_page = transition_prg_bank
                .checked_mul(2)
                .context("transition PRG bank MMC3 page overflow")?;
            Ok(TransitionMirrorBankMapping {
                source_prg_bank,
                source_prg_bank_hex: format!("{source_prg_bank:02X}"),
                transition_prg_bank,
                transition_prg_bank_hex: format!("{transition_prg_bank:02X}"),
                first_mmc3_page,
                second_mmc3_page: first_mmc3_page + 1,
                record_count: mirror.record_count,
                payload_byte_count,
                source_preserved_byte_count: PRG_BANK_SIZE - payload_byte_count,
                source_bank_sha1: sha1_hex(source_bank),
                material_byte_count: mirror.material.len(),
                material_sha1: sha1_hex(&mirror.material),
                non_payload_bytes_match_source,
                nmi_directory_source_sha1: sha1_hex(nmi_directory_source),
                nmi_directory_mirror_sha1: sha1_hex(nmi_directory_mirror),
                nmi_directory_matches_source,
                candidate_bank_sha1: sha1_hex(candidate_bank),
                candidate_bank_is_exact_ff: exact_ff,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let expected_source_selector_call = [
        0x20,
        SOURCE_PRG_SELECTOR as u8,
        (SOURCE_PRG_SELECTOR >> 8) as u8,
    ];
    ensure!(
        fixed_cpu_bytes(candidate, DIALOGUE_BYTE_BANK_SELECT_CALL, 3)?
            == expected_source_selector_call,
        "dialogue byte bank-select call no longer targets the source mapper 165 selector"
    );
    ensure!(
        fixed_cpu_bytes(candidate, DIALOGUE_BYTE_BANK_RESTORE_CALL, 3)?
            == expected_source_selector_call,
        "dialogue byte bank-restore call no longer targets the source mapper 165 selector"
    );
    let source_selector = fixed_cpu_bytes(candidate, SOURCE_PRG_SELECTOR, 6)?;
    ensure!(
        source_selector == [0x08, 0x48, 0x29, 0x0F, 0x0A, 0x48],
        "source mapper 165 PRG selector no longer masks to the low nibble"
    );
    let nmi_audio_dispatch = fixed_cpu_bytes(
        candidate,
        NMI_AUDIO_BANK_DISPATCH,
        NMI_AUDIO_BANK_DISPATCH_CODE.len(),
    )?;
    ensure!(
        nmi_audio_dispatch == NMI_AUDIO_BANK_DISPATCH_CODE,
        "NMI audio dispatch no longer restores through the active PRG bank shadow"
    );
    ensure!(
        fixed_cpu_bytes(candidate, NMI_AUDIO_BANK_RESTORE_CALL, 3)?
            == expected_source_selector_call,
        "NMI audio bank-restore call no longer targets the source mapper 165 selector"
    );
    let source_resolver_call = [
        0x20,
        SOURCE_POINTER_RESOLVER as u8,
        (SOURCE_POINTER_RESOLVER >> 8) as u8,
    ];
    for address in TRANSITION_POINTER_RESOLVER_CALLS {
        ensure!(
            switchable_cpu_bytes(candidate, SOURCE_DIALOGUE_BANK, address, 3)?
                == source_resolver_call,
            "transition pointer resolver call at {address:04X} changed"
        );
    }
    let selector_cave = fixed_cpu_bytes(
        candidate,
        SELECTOR_CAVE_START,
        usize::from(SELECTOR_CAVE_END_EXCLUSIVE - SELECTOR_CAVE_START),
    )?;
    let selector_cave_is_exact_ff = selector_cave.iter().all(|byte| *byte == 0xFF);
    ensure!(
        selector_cave_is_exact_ff,
        "transition mirror selector cave is no longer exact FF"
    );
    let routines = assemble_transition_reader_routines()?;
    ensure!(
        usize::from(TRANSITION_POINTER_RESOLVER - SELECTOR_CAVE_START)
            + routines.pointer_resolver.len()
            <= usize::from(TRANSITION_BANK_SELECTOR - SELECTOR_CAVE_START)
            && usize::from(TRANSITION_BANK_SELECTOR - SELECTOR_CAVE_START)
                + routines.bank_selector.len()
                <= usize::from(TRANSITION_BANK_RESTORE - SELECTOR_CAVE_START)
            && usize::from(TRANSITION_BANK_RESTORE - SELECTOR_CAVE_START)
                + routines.bank_restore.len()
                <= usize::from(NMI_PRG_BANK_RESTORE_SELECTOR - SELECTOR_CAVE_START)
            && usize::from(NMI_PRG_BANK_RESTORE_SELECTOR - SELECTOR_CAVE_START)
                + routines.nmi_bank_restore_selector.len()
                <= selector_cave.len(),
        "transition mirror routines do not fit their checked fixed-bank cave"
    );
    let complete_dialogue_write_set =
        validate_complete_dialogue_write_set(candidate, storage, &routines)?;

    Ok(RelocatedDialogueBankPlan {
        strategy_selected: true,
        strategy: "store every direct path in its source-owned region, clone each complete source bank into an execution-equivalent transition mirror, replace only transition dialogue payloads at the same CPU addresses, select marked transition banks only through the dialogue reader, preserve the central PRG selector, and restore a physical mirror only through the NMI audio return when it matches the live transition identity",
        current_candidate_mapper: candidate.mapper(),
        current_candidate_prg_size: candidate.prg().len(),
        battle_material_bank: BATTLE_MATERIAL_BANK,
        active_fixed_bank: ACTIVE_FIXED_BANK,
        transition_bank_marker: TRANSITION_BANK_MARKER,
        transition_bank_marker_hex: format!("{TRANSITION_BANK_MARKER:02X}"),
        all_selected_banks_are_exact_ff: mappings
            .iter()
            .all(|mapping| mapping.candidate_bank_is_exact_ff),
        mappings,
        direct_source_region_count: storage.direct_regions.len(),
        direct_storage_byte_count: storage.direct_used_storage_byte_count,
        canonical_pointer_write_count: storage.pointer_writes.len(),
        normalized_record_count: storage.normalized_record_count,
        transition_payload_byte_count: storage.transition_payload_byte_count,
        dialogue_byte_bank_select_call_cpu_address_hex: format!(
            "{DIALOGUE_BYTE_BANK_SELECT_CALL:04X}"
        ),
        dialogue_byte_bank_restore_call_cpu_address_hex: format!(
            "{DIALOGUE_BYTE_BANK_RESTORE_CALL:04X}"
        ),
        source_prg_selector_cpu_address_hex: format!("{SOURCE_PRG_SELECTOR:04X}"),
        source_selector_masks_to_low_nibble: true,
        source_selector_entry_preserved: true,
        nmi_audio_bank_dispatch_sha1: sha1_hex(nmi_audio_dispatch),
        nmi_audio_restore_uses_active_bank_shadow: true,
        nmi_audio_bank_restore_call_cpu_address_hex: format!("{NMI_AUDIO_BANK_RESTORE_CALL:04X}"),
        nmi_audio_bank_restore_call_hooked: true,
        transition_pointer_resolver_call_cpu_addresses_hex: TRANSITION_POINTER_RESOLVER_CALLS
            .map(|address| format!("{address:04X}"))
            .to_vec(),
        transition_pointer_resolver_cpu_address_hex: format!("{TRANSITION_POINTER_RESOLVER:04X}"),
        transition_pointer_resolver_byte_count: routines.pointer_resolver.len(),
        transition_pointer_resolver_sha1: sha1_hex(&routines.pointer_resolver),
        transition_bank_selector_cpu_address_hex: format!("{TRANSITION_BANK_SELECTOR:04X}"),
        transition_bank_selector_byte_count: routines.bank_selector.len(),
        transition_bank_selector_sha1: sha1_hex(&routines.bank_selector),
        transition_bank_restore_cpu_address_hex: format!("{TRANSITION_BANK_RESTORE:04X}"),
        transition_bank_restore_byte_count: routines.bank_restore.len(),
        transition_bank_restore_sha1: sha1_hex(&routines.bank_restore),
        nmi_prg_bank_restore_selector_cpu_address_hex: format!(
            "{NMI_PRG_BANK_RESTORE_SELECTOR:04X}"
        ),
        nmi_prg_bank_restore_selector_byte_count: routines.nmi_bank_restore_selector.len(),
        nmi_prg_bank_restore_selector_sha1: sha1_hex(&routines.nmi_bank_restore_selector),
        selector_cave_cpu_start_hex: format!("{SELECTOR_CAVE_START:04X}"),
        selector_cave_cpu_end_exclusive_hex: format!("{SELECTOR_CAVE_END_EXCLUSIVE:04X}"),
        selector_cave_byte_count: selector_cave.len(),
        selector_cave_sha1: sha1_hex(selector_cave),
        selector_cave_is_exact_ff,
        canonical_pointer_binding_planned: true,
        transition_operands_preserved: true,
        transition_mode_hooks_planned: true,
        nmi_restorable_reader_selection_assembled: true,
        complete_dialogue_write_set,
        writes_installed: false,
    })
}

fn prg_bank(rom: &Rom, bank: u8) -> Result<&[u8]> {
    let start = usize::from(bank)
        .checked_mul(PRG_BANK_SIZE)
        .context("expanded PRG bank start overflow")?;
    rom.prg()
        .get(start..start + PRG_BANK_SIZE)
        .context("expanded PRG bank is outside the current candidate")
}

fn fixed_cpu_bytes(rom: &Rom, address: u16, len: usize) -> Result<&[u8]> {
    ensure!(
        address >= 0xC000,
        "expanded fixed-bank address is outside $C000-$FFFF"
    );
    let start = rom
        .prg()
        .len()
        .checked_sub(PRG_BANK_SIZE)
        .and_then(|offset| offset.checked_add(usize::from(address - 0xC000)))
        .context("expanded fixed-bank offset overflow")?;
    rom.prg()
        .get(start..start + len)
        .context("expanded fixed-bank range is outside the current candidate")
}

fn switchable_cpu_bytes(rom: &Rom, bank: u8, address: u16, len: usize) -> Result<&[u8]> {
    ensure!(
        (0x8000..0xC000).contains(&address),
        "expanded switchable-bank address is outside $8000-$BFFF"
    );
    let start = usize::from(bank)
        .checked_mul(PRG_BANK_SIZE)
        .and_then(|offset| offset.checked_add(usize::from(address - 0x8000)))
        .context("expanded switchable-bank offset overflow")?;
    rom.prg()
        .get(start..start + len)
        .context("expanded switchable-bank range is outside the current candidate")
}
