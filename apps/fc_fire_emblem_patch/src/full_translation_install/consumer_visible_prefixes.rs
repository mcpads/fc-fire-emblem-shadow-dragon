use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::MainDialogueBundlePlan, dialogue_inventory::inspect_main_dialogue_entry_modes,
};

#[derive(Serialize)]
pub(in crate::full_translation_install) struct ConsumerVisiblePrefixPlan {
    canonical_record_count: usize,
    transition_edge_count: usize,
    transition_target_record_count: usize,
    multiple_incoming_transition_target_count: usize,
    direct_prefix_byte_counts: Vec<usize>,
    transition_prefix_byte_counts: Vec<usize>,
    transition_to_direct_body_deltas: Vec<isize>,
    positive_delta_target_count: usize,
    negative_delta_target_count: usize,
    single_global_transition_pointer_adjustment_possible: bool,
    translated_transition_target_body_byte_count: usize,
    distinct_body_split_upper_bound_byte_count: usize,
    distinct_body_split_upper_bound_fits_global_dialogue_storage: bool,
    transition_target_region_count: usize,
    distinct_body_split_upper_bound_overflow_region_count: usize,
    maximum_distinct_body_split_upper_bound_region_deficit_byte_count: usize,
    distinct_body_split_upper_bound_fits_every_source_region: bool,
    distinct_body_split_region_budgets: Vec<ConsumerSplitRegionBudget>,
    distinct_body_split_upper_bound_fits_every_source_bank: bool,
    distinct_body_split_bank_budgets: Vec<ConsumerSplitBankBudget>,
    compact_normalized_prefix_payload_byte_count: usize,
    fixed_row_normalized_prefix_payload_byte_count: usize,
    dense_record_to_prefix_row_byte_count: usize,
    selected_normalized_prefix_material_byte_count: usize,
    atlas_scan_remap_and_prefix_material_byte_count: usize,
    material_prg_8k_page_count_before_prefix_normalization: usize,
    material_prg_8k_page_count_after_prefix_normalization: usize,
    prefix_normalization_adds_prg_8k_pages: bool,
    leading_candidate: &'static str,
    candidate_strategies: [&'static str; 5],
    selected_strategy: Option<&'static str>,
    direct_entry_producers_bound: bool,
    consumer_specific_visible_prefixes_bound: bool,
}

#[derive(Serialize)]
struct ConsumerSplitRegionBudget {
    source_prg_bank: u8,
    source_prg_bank_hex: String,
    capacity_byte_count: usize,
    used_byte_count: usize,
    split_upper_bound_byte_count: usize,
    deficit_byte_count: usize,
}

#[derive(Serialize)]
struct ConsumerSplitBankBudget {
    source_prg_bank: u8,
    source_prg_bank_hex: String,
    capacity_byte_count: usize,
    used_byte_count: usize,
    split_upper_bound_byte_count: usize,
    deficit_byte_count: usize,
}

pub(in crate::full_translation_install) fn plan_consumer_visible_prefixes(
    source: &[u8],
    dialogue: &MainDialogueBundlePlan,
    source_owned_storage_byte_count: usize,
    planned_storage_byte_count: usize,
    atlas_scan_and_dynamic_remap_byte_count: usize,
) -> Result<ConsumerVisiblePrefixPlan> {
    let inspection = inspect_main_dialogue_entry_modes(source)?;
    let logical_byte_counts = dialogue.logical_record_byte_counts();
    let target_ids = inspection
        .transition_targets
        .iter()
        .map(|target| target.record_id.as_str())
        .collect::<BTreeSet<_>>();
    ensure!(
        target_ids
            .iter()
            .all(|record_id| logical_byte_counts.contains_key(record_id)),
        "main dialogue entry-mode target is absent from the all-record bundle"
    );

    let direct_prefix_byte_counts = inspection
        .transition_targets
        .iter()
        .map(|target| target.direct_prefix_byte_count)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let transition_prefix_byte_counts = inspection
        .transition_targets
        .iter()
        .map(|target| target.transition_prefix_byte_count)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let transition_to_direct_body_deltas = inspection
        .transition_targets
        .iter()
        .map(|target| target.transition_to_direct_body_delta)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let translated_transition_target_body_byte_count = target_ids
        .iter()
        .map(|record_id| {
            logical_byte_counts
                .get(record_id)
                .copied()
                .with_context(|| format!("missing logical byte count for {record_id}"))
        })
        .sum::<Result<usize>>()?;
    let distinct_body_split_upper_bound_byte_count = translated_transition_target_body_byte_count;
    let distinct_body_total = planned_storage_byte_count
        .checked_add(distinct_body_split_upper_bound_byte_count)
        .context("distinct consumer-body layout size overflow")?;
    let split_region_budgets = dialogue
        .region_storage_budgets()
        .into_iter()
        .map(|region| {
            let split_upper_bound = region
                .logical_record_byte_counts
                .iter()
                .filter(|(record_id, _)| target_ids.contains(record_id.as_str()))
                .map(|(_, byte_count)| *byte_count)
                .sum::<usize>();
            ConsumerSplitRegionBudget {
                source_prg_bank: region.source_prg_bank,
                source_prg_bank_hex: format!("{:02X}", region.source_prg_bank),
                capacity_byte_count: region.capacity_byte_count,
                used_byte_count: region.used_byte_count,
                split_upper_bound_byte_count: split_upper_bound,
                deficit_byte_count: region
                    .used_byte_count
                    .saturating_add(split_upper_bound)
                    .saturating_sub(region.capacity_byte_count),
            }
        })
        .collect::<Vec<_>>();
    let region_deficits = split_region_budgets
        .iter()
        .map(|region| region.deficit_byte_count)
        .collect::<Vec<_>>();
    let mut bank_totals = std::collections::BTreeMap::<u8, (usize, usize, usize)>::new();
    for region in &split_region_budgets {
        let totals = bank_totals.entry(region.source_prg_bank).or_default();
        totals.0 += region.capacity_byte_count;
        totals.1 += region.used_byte_count;
        totals.2 += region.split_upper_bound_byte_count;
    }
    let distinct_body_split_bank_budgets = bank_totals
        .into_iter()
        .map(
            |(source_prg_bank, (capacity, used, split))| ConsumerSplitBankBudget {
                source_prg_bank,
                source_prg_bank_hex: format!("{source_prg_bank:02X}"),
                capacity_byte_count: capacity,
                used_byte_count: used,
                split_upper_bound_byte_count: split,
                deficit_byte_count: used.saturating_add(split).saturating_sub(capacity),
            },
        )
        .collect::<Vec<_>>();
    let distinct_body_split_upper_bound_fits_every_source_bank = distinct_body_split_bank_budgets
        .iter()
        .all(|bank| bank.deficit_byte_count == 0);
    let positive_delta_target_count = inspection
        .transition_targets
        .iter()
        .filter(|target| target.transition_to_direct_body_delta > 0)
        .count();
    let negative_delta_target_count = inspection
        .transition_targets
        .iter()
        .filter(|target| target.transition_to_direct_body_delta < 0)
        .count();
    ensure!(
        positive_delta_target_count + negative_delta_target_count
            == inspection.transition_targets.len(),
        "main dialogue prefix normalization has a zero-delta target"
    );
    ensure!(
        inspection.transition_targets.len() < usize::from(u8::MAX),
        "main dialogue prefix rows do not fit a one-byte index with FF sentinel"
    );
    let compact_normalized_prefix_payload_byte_count =
        positive_delta_target_count * 4 + negative_delta_target_count * 6;
    let fixed_row_normalized_prefix_payload_byte_count = inspection.transition_targets.len() * 6;
    let dense_record_to_prefix_row_byte_count = inspection.canonical_record_count;
    let selected_normalized_prefix_material_byte_count =
        fixed_row_normalized_prefix_payload_byte_count + dense_record_to_prefix_row_byte_count;
    let atlas_scan_remap_and_prefix_material_byte_count = atlas_scan_and_dynamic_remap_byte_count
        .checked_add(selected_normalized_prefix_material_byte_count)
        .context("dialogue material size overflow after prefix normalization")?;
    let material_prg_8k_page_count_before_prefix_normalization =
        atlas_scan_and_dynamic_remap_byte_count.div_ceil(8 * 1024);
    let material_prg_8k_page_count_after_prefix_normalization =
        atlas_scan_remap_and_prefix_material_byte_count.div_ceil(8 * 1024);

    Ok(ConsumerVisiblePrefixPlan {
        canonical_record_count: inspection.canonical_record_count,
        transition_edge_count: inspection.transition_edge_count,
        transition_target_record_count: inspection.transition_targets.len(),
        multiple_incoming_transition_target_count: inspection
            .transition_targets
            .iter()
            .filter(|target| target.incoming_transition_edge_count > 1)
            .count(),
        direct_prefix_byte_counts,
        transition_prefix_byte_counts,
        transition_to_direct_body_deltas: transition_to_direct_body_deltas.clone(),
        positive_delta_target_count,
        negative_delta_target_count,
        single_global_transition_pointer_adjustment_possible: transition_to_direct_body_deltas
            .len()
            == 1,
        translated_transition_target_body_byte_count,
        distinct_body_split_upper_bound_byte_count,
        distinct_body_split_upper_bound_fits_global_dialogue_storage: distinct_body_total
            <= source_owned_storage_byte_count,
        transition_target_region_count: split_region_budgets
            .iter()
            .filter(|region| region.split_upper_bound_byte_count > 0)
            .count(),
        distinct_body_split_upper_bound_overflow_region_count: region_deficits
            .iter()
            .filter(|deficit| **deficit > 0)
            .count(),
        maximum_distinct_body_split_upper_bound_region_deficit_byte_count: region_deficits
            .iter()
            .copied()
            .max()
            .unwrap_or(0),
        distinct_body_split_upper_bound_fits_every_source_region: region_deficits
            .iter()
            .all(|deficit| *deficit == 0),
        distinct_body_split_region_budgets: split_region_budgets,
        distinct_body_split_upper_bound_fits_every_source_bank,
        distinct_body_split_bank_budgets,
        compact_normalized_prefix_payload_byte_count,
        fixed_row_normalized_prefix_payload_byte_count,
        dense_record_to_prefix_row_byte_count,
        selected_normalized_prefix_material_byte_count,
        atlas_scan_remap_and_prefix_material_byte_count,
        material_prg_8k_page_count_before_prefix_normalization,
        material_prg_8k_page_count_after_prefix_normalization,
        prefix_normalization_adds_prg_8k_pages:
            material_prg_8k_page_count_after_prefix_normalization
                > material_prg_8k_page_count_before_prefix_normalization,
        leading_candidate: "one canonical translated body plus a dense record-to-prefix-row index and fixed six-byte rows; direct and transition shims replay the original header or E8 effect before entering that body",
        candidate_strategies: [
            "bind direct producers, then keep only the entry modes that are actually reachable",
            "normalize relocated records and use a mode-aware transition shim that preserves E8 side effects",
            "split only proven dual-mode records into direct and transition bodies",
            "redirect transition controls to consumer-specific pointer aliases when unused table slots are proven",
            "recover split cost with dialogue token compression only if regional storage still overflows",
        ],
        selected_strategy: None,
        direct_entry_producers_bound: false,
        consumer_specific_visible_prefixes_bound: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_delta_is_required_before_selecting_one_global_adjustment() {
        let deltas = [4, 4, 4].into_iter().collect::<BTreeSet<_>>();
        assert_eq!(deltas.len(), 1);

        let mixed = [4, 10].into_iter().collect::<BTreeSet<_>>();
        assert_ne!(mixed.len(), 1);
    }
}
