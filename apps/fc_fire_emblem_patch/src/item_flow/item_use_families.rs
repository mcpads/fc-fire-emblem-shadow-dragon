use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::{CodeLocation, location};
use crate::rom::Rom;

use super::source_contract::{
    ITEM_ACTION_FLAGS_TABLE_ADDRESS, ITEM_COUNT, ITEM_DEFAULT_USES_TABLE_ADDRESS,
    ITEM_USE_ACTION_FLAG, source_slice,
};

mod class_change;
mod earth_orb;

const FIXED_PRG_BANK: u8 = 0x0F;
const EXPECTED_USABLE_ITEM_IDS: [u8; 26] = [
    0x0B, 0x10, 0x15, 0x40, 0x41, 0x42, 0x43, 0x44, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D,
    0x4E, 0x4F, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57,
];

#[derive(Debug, Serialize)]
pub(super) struct ItemUseCatalog {
    source_item_count: usize,
    use_action_flag_mask: u8,
    use_action_flag_mask_hex: String,
    pub(super) usable_item_count: usize,
    usable_items: Vec<UsableItemBinding>,
    effect_families: Vec<ItemUseEffectFamily>,
    class_change_contract: class_change::ClassChangeContract,
    earth_orb_contract: earth_orb::EarthOrbContract,
    dialogue_result_indices: Vec<u8>,
    dialogue_result_indices_hex: Vec<String>,
    static_conclusion: &'static str,
    runtime_conclusions: [&'static str; 2],
}

#[derive(Debug, Serialize)]
struct UsableItemBinding {
    item_id: u8,
    item_id_hex: String,
    action_flags: u8,
    action_flags_hex: String,
    default_uses: u8,
    default_uses_hex: String,
    effect_family: &'static str,
}

#[derive(Debug, Serialize)]
struct ItemUseEffectFamily {
    role: &'static str,
    item_ids: &'static [u8],
    item_ids_hex: Vec<String>,
    handler: CodeLocation,
    success_dialogue_indices: &'static [u8],
    success_dialogue_indices_hex: Vec<String>,
    failure_dialogue_indices: &'static [u8],
    failure_dialogue_indices_hex: Vec<String>,
    downstream_surface: &'static str,
    runtime_coverage: &'static str,
}

struct FamilySpec {
    role: &'static str,
    item_ids: &'static [u8],
    handler_address: u16,
    success_dialogue_indices: &'static [u8],
    failure_dialogue_indices: &'static [u8],
    downstream_surface: &'static str,
    runtime_coverage: &'static str,
}

const EFFECT_FAMILIES: &[FamilySpec] = &[
    FamilySpec {
        role: "full_hp_restore",
        item_ids: &[0x0B, 0x10],
        handler_address: 0x95DB,
        success_dialogue_indices: &[0x50],
        failure_dialogue_indices: &[],
        downstream_surface: "common item-result dialogue",
        runtime_coverage: "source_bound_only",
    },
    FamilySpec {
        role: "temporary_magic_resistance",
        item_ids: &[0x15, 0x41],
        handler_address: 0x95F0,
        success_dialogue_indices: &[0x2F],
        failure_dialogue_indices: &[0x30],
        downstream_surface: "common item-result dialogue",
        runtime_coverage: "source_bound_only",
    },
    FamilySpec {
        role: "self_heal",
        item_ids: &[0x40],
        handler_address: 0x9653,
        success_dialogue_indices: &[0x1D],
        failure_dialogue_indices: &[0x30],
        downstream_surface: "common item-result dialogue",
        runtime_coverage: "success, no-effect, and exhausted-use variants observed for item 0x40",
    },
    FamilySpec {
        role: "map_key",
        item_ids: &[0x42, 0x43, 0x44],
        handler_address: 0x9690,
        success_dialogue_indices: &[0x31, 0x32],
        failure_dialogue_indices: &[0x30],
        downstream_surface: "common item-result dialogue after a source-bound target search",
        runtime_coverage: "source_bound_only",
    },
    FamilySpec {
        role: "stat_boost",
        item_ids: &[0x46, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F],
        handler_address: 0x978C,
        success_dialogue_indices: &[0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27],
        failure_dialogue_indices: &[0x30],
        downstream_surface: "common item-result dialogue; result 0x27 continues to record 0x28",
        runtime_coverage: "source_bound_only",
    },
    FamilySpec {
        role: "class_change",
        item_ids: &[0x50, 0x51, 0x52, 0x53, 0x54],
        handler_address: 0x97DA,
        success_dialogue_indices: &[],
        failure_dialogue_indices: &[0x30],
        downstream_surface: "successful use bypasses common result selection and enters result substates 0x04 through 0x06, including the shared battle presentation",
        runtime_coverage: "successful use observed through the initial use sentence, shared battle presentation, its nested acknowledgement boundary, automatic cleanup, and map return",
    },
    FamilySpec {
        role: "earth_orb",
        item_ids: &[0x55],
        handler_address: 0x98AC,
        success_dialogue_indices: &[0x33],
        failure_dialogue_indices: &[],
        downstream_surface: "synchronous 32-step map-displacement and multi-target record effect inside result substate 0x02 before result 0x33",
        runtime_coverage: "all 32 effect steps, stable use text and CHR, final result 0x33, input wait, and map return observed",
    },
    FamilySpec {
        role: "explicit_no_effect",
        item_ids: &[0x56, 0x57],
        handler_address: 0x95D3,
        success_dialogue_indices: &[],
        failure_dialogue_indices: &[0x30],
        downstream_surface: "common item-result dialogue",
        runtime_coverage: "source_bound_only",
    },
];

pub(super) fn inspect(rom: &Rom) -> Result<ItemUseCatalog> {
    let class_change_contract = class_change::inspect(rom)?;
    let earth_orb_contract = earth_orb::inspect(rom)?;
    let action_flags = source_slice(
        rom,
        FIXED_PRG_BANK,
        ITEM_ACTION_FLAGS_TABLE_ADDRESS,
        ITEM_COUNT,
    )?;
    let default_uses = source_slice(
        rom,
        FIXED_PRG_BANK,
        ITEM_DEFAULT_USES_TABLE_ADDRESS,
        ITEM_COUNT,
    )?;
    let usable_item_ids = action_flags
        .iter()
        .enumerate()
        .filter(|(_, flags)| **flags & ITEM_USE_ACTION_FLAG != 0)
        .map(|(index, _)| u8::try_from(index + 1).context("usable item ID exceeds u8"))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        usable_item_ids == EXPECTED_USABLE_ITEM_IDS,
        "use-action item population changed"
    );

    let mut usable_items = Vec::with_capacity(usable_item_ids.len());
    for item_id in usable_item_ids {
        let index = usize::from(item_id - 1);
        let family = family_for_item(item_id)?;
        usable_items.push(UsableItemBinding {
            item_id,
            item_id_hex: format!("0x{item_id:02X}"),
            action_flags: action_flags[index],
            action_flags_hex: format!("0x{:02X}", action_flags[index]),
            default_uses: default_uses[index],
            default_uses_hex: format!("0x{:02X}", default_uses[index]),
            effect_family: family.role,
        });
    }

    let effect_families = EFFECT_FAMILIES
        .iter()
        .map(|family| ItemUseEffectFamily {
            role: family.role,
            item_ids: family.item_ids,
            item_ids_hex: hex_values(family.item_ids),
            handler: location(0x06, family.handler_address),
            success_dialogue_indices: family.success_dialogue_indices,
            success_dialogue_indices_hex: hex_values(family.success_dialogue_indices),
            failure_dialogue_indices: family.failure_dialogue_indices,
            failure_dialogue_indices_hex: hex_values(family.failure_dialogue_indices),
            downstream_surface: family.downstream_surface,
            runtime_coverage: family.runtime_coverage,
        })
        .collect::<Vec<_>>();
    let dialogue_result_indices = EFFECT_FAMILIES
        .iter()
        .flat_map(|family| {
            family
                .success_dialogue_indices
                .iter()
                .chain(family.failure_dialogue_indices)
        })
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    Ok(ItemUseCatalog {
        source_item_count: ITEM_COUNT,
        use_action_flag_mask: ITEM_USE_ACTION_FLAG,
        use_action_flag_mask_hex: format!("0x{ITEM_USE_ACTION_FLAG:02X}"),
        usable_item_count: usable_items.len(),
        usable_items,
        effect_families,
        class_change_contract,
        earth_orb_contract,
        dialogue_result_indices_hex: hex_values(&dialogue_result_indices),
        dialogue_result_indices,
        static_conclusion: "all use-action items and every directly selected result dialogue are source-bound; successful class change uses three extra result substates and the shared battle presentation, while the earth orb runs synchronously inside result substate 0x02",
        runtime_conclusions: [
            "successful class change: the initial use sentence leaves the map-text lifetime for the shared battle lifetime; the completed nested battle dialogue requires one A acknowledgement before automatic cleanup and map return",
            "earth orb: the initial use sentence and CHR remain live throughout all 32 automatic displacement steps, then result 0x33 shares that lifetime and waits for A in common result substate 0x03",
        ],
    })
}

pub(super) fn common_result_dialogue_sequences() -> Vec<Vec<u8>> {
    const INITIAL_USE_DIALOGUE_INDEX: u8 = 0x1A;
    const STAT_CAP_CONTINUATION_INDEX: u8 = 0x28;

    let mut sequences = EFFECT_FAMILIES
        .iter()
        .flat_map(|family| {
            family
                .success_dialogue_indices
                .iter()
                .chain(family.failure_dialogue_indices)
        })
        .map(|result| {
            let mut sequence = vec![INITIAL_USE_DIALOGUE_INDEX, *result];
            if *result == 0x27 {
                sequence.push(STAT_CAP_CONTINUATION_INDEX);
            }
            sequence
        })
        .collect::<std::collections::BTreeSet<_>>();
    sequences.insert(vec![INITIAL_USE_DIALOGUE_INDEX]);
    sequences.into_iter().collect()
}

fn family_for_item(item_id: u8) -> Result<&'static FamilySpec> {
    let matches = EFFECT_FAMILIES
        .iter()
        .filter(|family| family.item_ids.contains(&item_id))
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "usable item 0x{item_id:02X} belongs to {} effect families",
        matches.len()
    );
    Ok(matches[0])
}

fn hex_values(values: &[u8]) -> Vec<String> {
    values
        .iter()
        .map(|value| format!("0x{value:02X}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_expected_usable_item_has_exactly_one_family() {
        let classified = EFFECT_FAMILIES
            .iter()
            .flat_map(|family| family.item_ids.iter().copied())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(classified, EXPECTED_USABLE_ITEM_IDS.into_iter().collect());
        for item_id in EXPECTED_USABLE_ITEM_IDS {
            assert!(family_for_item(item_id).is_ok());
        }
    }

    #[test]
    fn only_two_families_leave_the_common_result_surface() {
        let roles = EFFECT_FAMILIES
            .iter()
            .filter(|family| !family.downstream_surface.starts_with("common item-result"))
            .map(|family| family.role)
            .collect::<Vec<_>>();
        assert_eq!(roles, ["class_change", "earth_orb"]);
    }

    #[test]
    fn dialogue_results_cover_stat_transition_and_earth_orb_result() {
        let results = EFFECT_FAMILIES
            .iter()
            .flat_map(|family| {
                family
                    .success_dialogue_indices
                    .iter()
                    .chain(family.failure_dialogue_indices)
            })
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        assert!(results.contains(&0x27));
        assert!(results.contains(&0x33));
        assert!(results.contains(&0x50));
    }

    #[test]
    fn common_result_sequences_keep_mutually_exclusive_results_separate() {
        let sequences = common_result_dialogue_sequences();
        assert_eq!(sequences.len(), 18);
        assert!(sequences.contains(&vec![0x1A]));
        assert!(sequences.contains(&vec![0x1A, 0x27, 0x28]));
        assert!(sequences.contains(&vec![0x1A, 0x33]));
        assert!(
            !sequences
                .iter()
                .any(|sequence| { sequence.contains(&0x1D) && sequence.contains(&0x30) })
        );
    }
}
