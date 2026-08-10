use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    mmc5_prg::fixed_bank_file_offset,
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    text_inventory::FixedTextPlan,
    typed_source::decode_rp2a03_sequence,
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const ITEM_ENTRY_COUNT: usize = 0x5B;
const ITEM_ELIGIBILITY_PRG_BANK: u8 = 0x06;
const ITEM_ELIGIBILITY_CPU_ADDRESS: u16 = 0xA35E;
const ITEM_ELIGIBILITY_BYTE_COUNT: usize = 0x73;
const ITEM_ELIGIBILITY_SHA1: &str = "9557d82d7b1984b51602540018b8666c07c07aec";
const ITEM_ACTION_FLAGS_CPU_ADDRESS: u16 = 0xD9C3;
const ITEM_ACTION_FLAGS_SHA1: &str = "17c5bdab2181218617fdc1d7f1f6866ce437eea5";
const CANDIDATE_SOURCE_INDEX_SHA1: &str = "7ffca0f4fbd9825518e8cdd10791188510936958";
const EQUIP_NECESSARY_CONDITION_FRAGMENT: [u8; 8] =
    [0xCA, 0xBD, 0xC3, 0xD9, 0x29, 0x01, 0xD0, 0x4A];

pub(super) struct BattleItemDomain {
    pub(super) glyph_sets: Vec<BTreeSet<char>>,
    pub(super) binding: BattleItemDomainBinding,
}

#[derive(Debug, Serialize)]
pub(super) struct BattleItemDomainBinding {
    total_item_entry_count: usize,
    candidate_item_entry_count: usize,
    excluded_item_entry_count: usize,
    item_id_to_source_index: &'static str,
    equip_necessary_condition: &'static str,
    candidate_source_index_sha1: String,
    eligibility_routine: ItemEligibilityRoutineBinding,
    item_action_flags: ItemActionFlagsBinding,
    candidate_set_is_necessary_condition_superset: bool,
    weapon_level_and_class_checks_modeled: bool,
    actual_equipped_item_reachability_proven: bool,
}

#[derive(Debug, Serialize)]
struct ItemEligibilityRoutineBinding {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
    source_sha1: String,
    typed_instruction_count: usize,
}

#[derive(Debug, Serialize)]
struct ItemActionFlagsBinding {
    role: &'static str,
    cpu_address: u16,
    byte_count: usize,
    source_sha1: String,
    equip_rejection_mask: u8,
}

pub(super) fn bind_battle_item_domain(
    rom: &Rom,
    fixed: &FixedTextPlan,
) -> Result<BattleItemDomain> {
    let eligibility_source = eligibility_source(rom)?;
    ensure!(
        eligibility_source
            .windows(EQUIP_NECESSARY_CONDITION_FRAGMENT.len())
            .any(|window| window == EQUIP_NECESSARY_CONDITION_FRAGMENT),
        "item eligibility no longer rejects action-flag bit 0"
    );
    let eligibility_sha1 = sha1_hex(eligibility_source);
    ensure!(
        eligibility_sha1 == ITEM_ELIGIBILITY_SHA1,
        "item eligibility source changed: expected {ITEM_ELIGIBILITY_SHA1}, found {eligibility_sha1}"
    );
    let eligibility_instructions = decode_rp2a03_sequence(
        eligibility_source,
        ITEM_ELIGIBILITY_CPU_ADDRESS,
        "evaluate_unit_item_eligibility",
    )?;

    let flags_offset = fixed_bank_file_offset(ITEM_ACTION_FLAGS_CPU_ADDRESS)?;
    let flags = rom
        .data()
        .get(flags_offset..flags_offset + ITEM_ENTRY_COUNT)
        .context("item action-flags table is outside the ROM")?;
    let flags_sha1 = sha1_hex(flags);
    ensure!(
        flags_sha1 == ITEM_ACTION_FLAGS_SHA1,
        "item action flags changed: expected {ITEM_ACTION_FLAGS_SHA1}, found {flags_sha1}"
    );
    let candidate_source_indices = equip_candidate_source_indices(flags);
    ensure!(
        candidate_source_indices.len() == 64,
        "item equip necessary-condition candidate count changed"
    );
    let candidate_index_bytes = candidate_source_indices
        .iter()
        .map(|index| u8::try_from(*index).context("item source index exceeds report encoding"))
        .collect::<Result<Vec<_>>>()?;
    let candidate_source_index_sha1 = sha1_hex(&candidate_index_bytes);
    ensure!(
        candidate_source_index_sha1 == CANDIDATE_SOURCE_INDEX_SHA1,
        "item equip candidate indices changed: expected {CANDIDATE_SOURCE_INDEX_SHA1}, found {candidate_source_index_sha1}"
    );

    let item_entries = fixed
        .entries
        .iter()
        .filter(|entry| entry.table_id == "item-names")
        .collect::<Vec<_>>();
    ensure!(
        item_entries.len() == ITEM_ENTRY_COUNT,
        "fixed-text item entry count does not match the action-flags table"
    );
    for (expected_source_index, entry) in item_entries.iter().enumerate() {
        ensure!(
            entry.source_index == expected_source_index,
            "fixed-text item source indices are not contiguous at {expected_source_index}"
        );
    }
    let candidate_source_indices = candidate_source_indices
        .into_iter()
        .collect::<BTreeSet<_>>();
    let glyph_sets = item_entries
        .into_iter()
        .filter(|entry| candidate_source_indices.contains(&entry.source_index))
        .map(|entry| entry.unique_glyphs())
        .collect::<Vec<_>>();
    ensure!(
        glyph_sets.len() == candidate_source_indices.len(),
        "battle item domain lost a candidate translation entry"
    );

    Ok(BattleItemDomain {
        glyph_sets,
        binding: BattleItemDomainBinding {
            total_item_entry_count: ITEM_ENTRY_COUNT,
            candidate_item_entry_count: candidate_source_indices.len(),
            excluded_item_entry_count: ITEM_ENTRY_COUNT - candidate_source_indices.len(),
            item_id_to_source_index: "item_id - 1",
            equip_necessary_condition: "item ID is nonzero and item action flags bit 0x01 is clear",
            candidate_source_index_sha1,
            eligibility_routine: ItemEligibilityRoutineBinding {
                role: "evaluate_unit_item_eligibility",
                prg_bank: ITEM_ELIGIBILITY_PRG_BANK,
                cpu_address: ITEM_ELIGIBILITY_CPU_ADDRESS,
                byte_count: ITEM_ELIGIBILITY_BYTE_COUNT,
                source_sha1: eligibility_sha1,
                typed_instruction_count: eligibility_instructions.len(),
            },
            item_action_flags: ItemActionFlagsBinding {
                role: "item_action_flags",
                cpu_address: ITEM_ACTION_FLAGS_CPU_ADDRESS,
                byte_count: ITEM_ENTRY_COUNT,
                source_sha1: flags_sha1,
                equip_rejection_mask: 0x01,
            },
            candidate_set_is_necessary_condition_superset: true,
            weapon_level_and_class_checks_modeled: false,
            actual_equipped_item_reachability_proven: false,
        },
    })
}

fn eligibility_source(rom: &Rom) -> Result<&[u8]> {
    let file_offset = HEADER_SIZE
        + usize::from(ITEM_ELIGIBILITY_PRG_BANK) * PRG_BANK_SIZE
        + usize::from(ITEM_ELIGIBILITY_CPU_ADDRESS - SWITCHABLE_CPU_START);
    rom.data()
        .get(file_offset..file_offset + ITEM_ELIGIBILITY_BYTE_COUNT)
        .context("item eligibility routine is outside the ROM")
}

fn equip_candidate_source_indices(flags: &[u8]) -> Vec<usize> {
    flags
        .iter()
        .enumerate()
        .filter_map(|(source_index, flags)| (flags & 0x01 == 0).then_some(source_index))
        .collect()
}

#[cfg(test)]
pub(super) fn test_binding() -> BattleItemDomainBinding {
    BattleItemDomainBinding {
        total_item_entry_count: ITEM_ENTRY_COUNT,
        candidate_item_entry_count: 64,
        excluded_item_entry_count: 27,
        item_id_to_source_index: "item_id - 1",
        equip_necessary_condition: "flags bit 0 clear",
        candidate_source_index_sha1: "indices".to_owned(),
        eligibility_routine: ItemEligibilityRoutineBinding {
            role: "eligibility",
            prg_bank: 6,
            cpu_address: ITEM_ELIGIBILITY_CPU_ADDRESS,
            byte_count: ITEM_ELIGIBILITY_BYTE_COUNT,
            source_sha1: "routine".to_owned(),
            typed_instruction_count: 1,
        },
        item_action_flags: ItemActionFlagsBinding {
            role: "flags",
            cpu_address: ITEM_ACTION_FLAGS_CPU_ADDRESS,
            byte_count: ITEM_ENTRY_COUNT,
            source_sha1: "flags".to_owned(),
            equip_rejection_mask: 1,
        },
        candidate_set_is_necessary_condition_superset: true,
        weapon_level_and_class_checks_modeled: false,
        actual_equipped_item_reachability_proven: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_flag_bit_zero_is_only_a_necessary_equip_filter() {
        let candidates = equip_candidate_source_indices(&[0x00, 0x01, 0x40, 0x41]);

        assert_eq!(candidates, vec![0, 2]);
        let binding = test_binding();
        assert!(binding.candidate_set_is_necessary_condition_superset);
        assert!(!binding.weapon_level_and_class_checks_modeled);
        assert!(!binding.actual_equipped_item_reachability_proven);
    }
}
