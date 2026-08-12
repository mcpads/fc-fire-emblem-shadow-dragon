use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{rom::Rom, sha1_hex};

use super::NormalizedStorageBudgetPlan;

const EXPECTED_MAPPER: u16 = 165;
const EXPANDED_PRG_SIZE: usize = 512 * 1024;
const PRG_BANK_SIZE: usize = 16 * 1024;
const SOURCE_BANKS: [u8; 2] = [0x07, 0x08];
const TRANSLATION_BANKS: [u8; 2] = [0x11, 0x12];
const BATTLE_MATERIAL_BANK: u8 = 0x10;
const ACTIVE_FIXED_BANK: u8 = 0x1F;
const DIALOGUE_BYTE_BANK_SELECT_CALL: u16 = 0xE6A1;
const DIALOGUE_BYTE_BANK_RESTORE_CALL: u16 = 0xE6AB;
const SOURCE_PRG_SELECTOR: u16 = 0xFA20;
const SELECTOR_CAVE_START: u16 = 0xF558;
const SELECTOR_CAVE_END_EXCLUSIVE: u16 = 0xF600;

#[derive(Serialize)]
pub(super) struct RelocatedDialogueBankPlan {
    pub(super) strategy_selected: bool,
    strategy: &'static str,
    current_candidate_mapper: u16,
    current_candidate_prg_size: usize,
    battle_material_bank: u8,
    active_fixed_bank: u8,
    mappings: Vec<RelocatedDialogueBankMapping>,
    all_selected_banks_are_exact_ff: bool,
    all_selected_banks_fit: bool,
    dialogue_byte_bank_select_call_cpu_address_hex: String,
    dialogue_byte_bank_restore_call_cpu_address_hex: String,
    source_prg_selector_cpu_address_hex: String,
    source_selector_masks_to_low_nibble: bool,
    reader_only_selector_required: bool,
    indexed_pointer_table_selection_remains_on_source_banks: bool,
    selector_cave_cpu_start_hex: String,
    selector_cave_cpu_end_exclusive_hex: String,
    selector_cave_byte_count: usize,
    selector_cave_sha1: String,
    selector_cave_is_exact_ff: bool,
    canonical_pointer_binding_planned: bool,
    transition_operand_binding_planned: bool,
    selector_assembled: bool,
}

#[derive(Serialize)]
struct RelocatedDialogueBankMapping {
    source_prg_bank: u8,
    source_prg_bank_hex: String,
    translation_prg_bank: u8,
    translation_prg_bank_hex: String,
    first_mmc3_page: u8,
    second_mmc3_page: u8,
    planned_storage_upper_bound_byte_count: usize,
    capacity_byte_count: usize,
    remaining_byte_count: usize,
    candidate_bank_sha1: String,
    candidate_bank_is_exact_ff: bool,
}

pub(super) fn plan_relocated_dialogue_banks(
    candidate: &Rom,
    storage: &NormalizedStorageBudgetPlan,
) -> Result<RelocatedDialogueBankPlan> {
    ensure!(
        candidate.mapper() == EXPECTED_MAPPER && candidate.prg().len() == EXPANDED_PRG_SIZE,
        "relocated dialogue banks require the current 512 KiB mapper 165 candidate"
    );
    ensure!(
        TRANSLATION_BANKS
            .iter()
            .all(|bank| *bank != BATTLE_MATERIAL_BANK && *bank != ACTIVE_FIXED_BANK),
        "relocated dialogue banks overlap reserved expanded PRG banks"
    );

    let mappings = SOURCE_BANKS
        .into_iter()
        .zip(TRANSLATION_BANKS)
        .map(|(source_prg_bank, translation_prg_bank)| {
            let planned = storage.planned_byte_count_for_bank(source_prg_bank)?;
            let bank = prg_bank(candidate, translation_prg_bank)?;
            let exact_ff = bank.iter().all(|byte| *byte == 0xFF);
            ensure!(
                exact_ff,
                "selected translation PRG bank {translation_prg_bank:02X} is not empty"
            );
            ensure!(
                planned <= bank.len(),
                "source dialogue bank {source_prg_bank:02X} needs {planned} bytes but selected translation bank {translation_prg_bank:02X} has only {}",
                bank.len()
            );
            let first_mmc3_page = translation_prg_bank
                .checked_mul(2)
                .context("translation PRG bank MMC3 page overflow")?;
            Ok(RelocatedDialogueBankMapping {
                source_prg_bank,
                source_prg_bank_hex: format!("{source_prg_bank:02X}"),
                translation_prg_bank,
                translation_prg_bank_hex: format!("{translation_prg_bank:02X}"),
                first_mmc3_page,
                second_mmc3_page: first_mmc3_page + 1,
                planned_storage_upper_bound_byte_count: planned,
                capacity_byte_count: bank.len(),
                remaining_byte_count: bank.len() - planned,
                candidate_bank_sha1: sha1_hex(bank),
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
    let selector_cave = fixed_cpu_bytes(
        candidate,
        SELECTOR_CAVE_START,
        usize::from(SELECTOR_CAVE_END_EXCLUSIVE - SELECTOR_CAVE_START),
    )?;
    let selector_cave_is_exact_ff = selector_cave.iter().all(|byte| *byte == 0xFF);
    ensure!(
        selector_cave_is_exact_ff,
        "relocated dialogue selector cave is no longer exact FF"
    );

    Ok(RelocatedDialogueBankPlan {
        strategy_selected: true,
        strategy: "relocate complete normalized dialogue payloads for overflowing source banks into dedicated expanded 16 KiB banks and remap only the fixed-bank dialogue byte reader",
        current_candidate_mapper: candidate.mapper(),
        current_candidate_prg_size: candidate.prg().len(),
        battle_material_bank: BATTLE_MATERIAL_BANK,
        active_fixed_bank: ACTIVE_FIXED_BANK,
        all_selected_banks_are_exact_ff: mappings
            .iter()
            .all(|mapping| mapping.candidate_bank_is_exact_ff),
        all_selected_banks_fit: mappings.iter().all(|mapping| {
            mapping.planned_storage_upper_bound_byte_count <= mapping.capacity_byte_count
        }),
        mappings,
        dialogue_byte_bank_select_call_cpu_address_hex: format!(
            "{DIALOGUE_BYTE_BANK_SELECT_CALL:04X}"
        ),
        dialogue_byte_bank_restore_call_cpu_address_hex: format!(
            "{DIALOGUE_BYTE_BANK_RESTORE_CALL:04X}"
        ),
        source_prg_selector_cpu_address_hex: format!("{SOURCE_PRG_SELECTOR:04X}"),
        source_selector_masks_to_low_nibble: true,
        reader_only_selector_required: true,
        indexed_pointer_table_selection_remains_on_source_banks: true,
        selector_cave_cpu_start_hex: format!("{SELECTOR_CAVE_START:04X}"),
        selector_cave_cpu_end_exclusive_hex: format!("{SELECTOR_CAVE_END_EXCLUSIVE:04X}"),
        selector_cave_byte_count: selector_cave.len(),
        selector_cave_sha1: sha1_hex(selector_cave),
        selector_cave_is_exact_ff,
        canonical_pointer_binding_planned: false,
        transition_operand_binding_planned: false,
        selector_assembled: false,
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
