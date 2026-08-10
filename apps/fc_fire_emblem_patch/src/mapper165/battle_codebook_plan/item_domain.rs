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

mod eligibility_tables;
mod participant_glyphs;

use eligibility_tables::{
    bank_six_slice, eligible_player_loadouts, equip_candidate_source_indices,
    item_family_class_lists,
};
use participant_glyphs::{BattleItemGlyphSets, plan_battle_item_glyph_sets};

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
const ITEM_REQUIREMENTS_CPU_ADDRESS: u16 = 0xD6B3;
const ITEM_REQUIREMENTS_SHA1: &str = "a1c2dc64c53fe2597acd90795c5ef48e8a2efaf9";
const ITEM_FAMILY_THRESHOLD_CPU_ADDRESS: u16 = 0xA3D1;
const ITEM_FAMILY_THRESHOLD_COUNT: usize = 9;
const ITEM_FAMILY_THRESHOLD_SHA1: &str = "cba6fceb7629df0f1aec73ca9083cc7a8a515760";
const ITEM_FAMILY_CLASS_POINTER_CPU_ADDRESS: u16 = 0xA3DA;
const ITEM_FAMILY_CLASS_POINTER_SHA1: &str = "e226ab95ae307032a9e53e77416dee18b26493b3";
const ITEM_FAMILY_CLASS_LIST_SHA1: &str = "ac08ce27ad7437447ce687b509936d84bee89aa3";
const CLASS_ITEM_PAIR_COUNT: usize = 377;
const CLASS_ITEM_PAIR_SHA1: &str = "726faa7d3509b14da54337d1741c906143c27773";
const PLAYER_LOADOUT_SHA1: &str = "5f2cae5fa5afed57f0274b433a27116e1d129634";
const IDENTITY_RESTRICTED_LOADOUT_COUNT: usize = 184;
const UNRESTRICTED_LOADOUT_COUNT: usize = 193;
const ENEMY_CLASS_ITEM_PAIR_SHA1: &str = "9dbd65cdd40128abae6e818dc100f5b196b093cc";
const CLASS_SOURCE_ENTRY_COUNT: usize = 23;
const UNIT_SOURCE_ENTRY_COUNT: usize = 52;
const EQUIP_NECESSARY_CONDITION_FRAGMENT: [u8; 8] =
    [0xCA, 0xBD, 0xC3, 0xD9, 0x29, 0x01, 0xD0, 0x4A];

pub(super) struct BattleItemDomain {
    pub(super) equip_candidate_item_glyph_sets: Vec<BTreeSet<char>>,
    pub(super) enemy_class_item_pairs: BTreeSet<(u8, u8)>,
    pub(super) player_participant_glyph_sets: Vec<BTreeSet<char>>,
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
    item_requirements: ItemTableBinding,
    item_family_thresholds: ItemTableBinding,
    item_family_class_pointers: ItemTableBinding,
    item_family_class_list_sha1: String,
    class_item_pair_count: usize,
    class_item_pair_sha1: String,
    player_loadout_sha1: String,
    identity_restricted_loadout_count: usize,
    unrestricted_loadout_count: usize,
    enemy_class_item_pair_sha1: String,
    player_participant_candidate_count: usize,
    candidate_set_is_necessary_condition_superset: bool,
    class_family_checks_modeled: bool,
    weapon_level_thresholds_modeled: bool,
    identity_restrictions_modeled: bool,
    identity_restricted_item_classes_conservative: bool,
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

#[derive(Debug, Serialize)]
struct ItemTableBinding {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
    source_sha1: String,
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

    let requirements_offset = fixed_bank_file_offset(ITEM_REQUIREMENTS_CPU_ADDRESS)?;
    let requirements = rom
        .data()
        .get(requirements_offset..requirements_offset + ITEM_ENTRY_COUNT)
        .context("item requirements table is outside the ROM")?;
    let requirements_sha1 = sha1_hex(requirements);
    ensure!(
        requirements_sha1 == ITEM_REQUIREMENTS_SHA1,
        "item requirements changed: expected {ITEM_REQUIREMENTS_SHA1}, found {requirements_sha1}"
    );
    let family_thresholds = bank_six_slice(
        rom,
        ITEM_FAMILY_THRESHOLD_CPU_ADDRESS,
        ITEM_FAMILY_THRESHOLD_COUNT,
        "item family thresholds",
    )?;
    let family_thresholds_sha1 = sha1_hex(family_thresholds);
    ensure!(
        family_thresholds_sha1 == ITEM_FAMILY_THRESHOLD_SHA1,
        "item family thresholds changed: expected {ITEM_FAMILY_THRESHOLD_SHA1}, found {family_thresholds_sha1}"
    );
    let family_pointer_bytes = bank_six_slice(
        rom,
        ITEM_FAMILY_CLASS_POINTER_CPU_ADDRESS,
        ITEM_FAMILY_THRESHOLD_COUNT * 2,
        "item family class pointers",
    )?;
    let family_pointer_sha1 = sha1_hex(family_pointer_bytes);
    ensure!(
        family_pointer_sha1 == ITEM_FAMILY_CLASS_POINTER_SHA1,
        "item family class pointers changed: expected {ITEM_FAMILY_CLASS_POINTER_SHA1}, found {family_pointer_sha1}"
    );
    let family_class_lists = item_family_class_lists(rom, family_pointer_bytes)?;
    let family_list_bytes = family_class_lists
        .iter()
        .flat_map(|classes| classes.iter().copied().chain([0xEF]))
        .collect::<Vec<_>>();
    let family_class_list_sha1 = sha1_hex(&family_list_bytes);
    ensure!(
        family_class_list_sha1 == ITEM_FAMILY_CLASS_LIST_SHA1,
        "item family class lists changed: expected {ITEM_FAMILY_CLASS_LIST_SHA1}, found {family_class_list_sha1}"
    );
    let player_loadouts =
        eligible_player_loadouts(flags, requirements, family_thresholds, &family_class_lists)?;
    ensure!(
        player_loadouts.len() == CLASS_ITEM_PAIR_COUNT,
        "player loadout count changed: expected {CLASS_ITEM_PAIR_COUNT}, found {}",
        player_loadouts.len()
    );
    let player_loadout_bytes = player_loadouts
        .iter()
        .flat_map(|loadout| [loadout.required_identity, loadout.class_id, loadout.item_id])
        .collect::<Vec<_>>();
    let player_loadout_sha1 = sha1_hex(&player_loadout_bytes);
    ensure!(
        player_loadout_sha1 == PLAYER_LOADOUT_SHA1,
        "player loadouts changed: expected {PLAYER_LOADOUT_SHA1}, found {player_loadout_sha1}"
    );
    let identity_restricted_loadout_count = player_loadouts
        .iter()
        .filter(|loadout| loadout.required_identity != 0)
        .count();
    let unrestricted_loadout_count = player_loadouts.len() - identity_restricted_loadout_count;
    ensure!(
        identity_restricted_loadout_count == IDENTITY_RESTRICTED_LOADOUT_COUNT,
        "identity-restricted player loadout count changed"
    );
    ensure!(
        unrestricted_loadout_count == UNRESTRICTED_LOADOUT_COUNT,
        "unrestricted player loadout count changed"
    );

    let class_item_pairs = player_loadouts
        .iter()
        .map(|loadout| (loadout.class_id, loadout.item_id))
        .collect::<BTreeSet<_>>();
    ensure!(
        class_item_pairs.len() == CLASS_ITEM_PAIR_COUNT,
        "class-item pair count changed: expected {CLASS_ITEM_PAIR_COUNT}, found {}",
        class_item_pairs.len()
    );
    let pair_bytes = class_item_pairs
        .iter()
        .flat_map(|(class_id, item_id)| [*class_id, *item_id])
        .collect::<Vec<_>>();
    let class_item_pair_sha1 = sha1_hex(&pair_bytes);
    ensure!(
        class_item_pair_sha1 == CLASS_ITEM_PAIR_SHA1,
        "class-item pairs changed: expected {CLASS_ITEM_PAIR_SHA1}, found {class_item_pair_sha1}"
    );
    let enemy_class_item_pairs = player_loadouts
        .iter()
        .filter(|loadout| loadout.required_identity == 0)
        .map(|loadout| (loadout.class_id, loadout.item_id))
        .collect::<BTreeSet<_>>();
    let enemy_pair_bytes = enemy_class_item_pairs
        .iter()
        .flat_map(|(class_id, item_id)| [*class_id, *item_id])
        .collect::<Vec<_>>();
    let enemy_class_item_pair_sha1 = sha1_hex(&enemy_pair_bytes);
    ensure!(
        enemy_class_item_pairs.len() == UNRESTRICTED_LOADOUT_COUNT,
        "enemy class-item pair count changed"
    );
    ensure!(
        enemy_class_item_pair_sha1 == ENEMY_CLASS_ITEM_PAIR_SHA1,
        "enemy class-item pairs changed: expected {ENEMY_CLASS_ITEM_PAIR_SHA1}, found {enemy_class_item_pair_sha1}"
    );

    let candidate_source_indices = candidate_source_indices
        .into_iter()
        .collect::<BTreeSet<_>>();
    let BattleItemGlyphSets {
        item_glyph_sets: glyph_sets,
        player_participant_glyph_sets,
    } = plan_battle_item_glyph_sets(fixed, &candidate_source_indices, &player_loadouts)?;
    let player_participant_candidate_count = player_participant_glyph_sets.len();

    Ok(BattleItemDomain {
        equip_candidate_item_glyph_sets: glyph_sets,
        enemy_class_item_pairs,
        player_participant_glyph_sets,
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
            item_requirements: ItemTableBinding {
                role: "item_weapon_level_or_identity_requirement",
                prg_bank: 0x0F,
                cpu_address: ITEM_REQUIREMENTS_CPU_ADDRESS,
                byte_count: ITEM_ENTRY_COUNT,
                source_sha1: requirements_sha1,
            },
            item_family_thresholds: ItemTableBinding {
                role: "item_family_upper_bounds",
                prg_bank: ITEM_ELIGIBILITY_PRG_BANK,
                cpu_address: ITEM_FAMILY_THRESHOLD_CPU_ADDRESS,
                byte_count: ITEM_FAMILY_THRESHOLD_COUNT,
                source_sha1: family_thresholds_sha1,
            },
            item_family_class_pointers: ItemTableBinding {
                role: "item_family_allowed_class_pointers",
                prg_bank: ITEM_ELIGIBILITY_PRG_BANK,
                cpu_address: ITEM_FAMILY_CLASS_POINTER_CPU_ADDRESS,
                byte_count: ITEM_FAMILY_THRESHOLD_COUNT * 2,
                source_sha1: family_pointer_sha1,
            },
            item_family_class_list_sha1: family_class_list_sha1,
            class_item_pair_count: CLASS_ITEM_PAIR_COUNT,
            class_item_pair_sha1,
            player_loadout_sha1,
            identity_restricted_loadout_count,
            unrestricted_loadout_count,
            enemy_class_item_pair_sha1,
            player_participant_candidate_count,
            candidate_set_is_necessary_condition_superset: true,
            class_family_checks_modeled: true,
            weapon_level_thresholds_modeled: false,
            identity_restrictions_modeled: true,
            identity_restricted_item_classes_conservative: true,
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
        item_requirements: ItemTableBinding {
            role: "requirements",
            prg_bank: 15,
            cpu_address: ITEM_REQUIREMENTS_CPU_ADDRESS,
            byte_count: ITEM_ENTRY_COUNT,
            source_sha1: "requirements".to_owned(),
        },
        item_family_thresholds: ItemTableBinding {
            role: "thresholds",
            prg_bank: 6,
            cpu_address: ITEM_FAMILY_THRESHOLD_CPU_ADDRESS,
            byte_count: ITEM_FAMILY_THRESHOLD_COUNT,
            source_sha1: "thresholds".to_owned(),
        },
        item_family_class_pointers: ItemTableBinding {
            role: "class pointers",
            prg_bank: 6,
            cpu_address: ITEM_FAMILY_CLASS_POINTER_CPU_ADDRESS,
            byte_count: ITEM_FAMILY_THRESHOLD_COUNT * 2,
            source_sha1: "pointers".to_owned(),
        },
        item_family_class_list_sha1: "lists".to_owned(),
        class_item_pair_count: CLASS_ITEM_PAIR_COUNT,
        class_item_pair_sha1: "pairs".to_owned(),
        player_loadout_sha1: "loadouts".to_owned(),
        identity_restricted_loadout_count: IDENTITY_RESTRICTED_LOADOUT_COUNT,
        unrestricted_loadout_count: UNRESTRICTED_LOADOUT_COUNT,
        enemy_class_item_pair_sha1: "enemy pairs".to_owned(),
        player_participant_candidate_count: UNRESTRICTED_LOADOUT_COUNT * UNIT_SOURCE_ENTRY_COUNT
            + IDENTITY_RESTRICTED_LOADOUT_COUNT,
        candidate_set_is_necessary_condition_superset: true,
        class_family_checks_modeled: true,
        weapon_level_thresholds_modeled: false,
        identity_restrictions_modeled: true,
        identity_restricted_item_classes_conservative: true,
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
        assert!(binding.class_family_checks_modeled);
        assert!(!binding.weapon_level_thresholds_modeled);
        assert!(!binding.actual_equipped_item_reachability_proven);
    }
}
