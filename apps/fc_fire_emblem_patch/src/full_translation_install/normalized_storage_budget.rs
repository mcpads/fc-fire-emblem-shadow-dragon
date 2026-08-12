use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::dialogue_assets::{MainDialogueBundlePlan, MainDialogueDisplayPlan};

#[derive(Serialize)]
pub(super) struct NormalizedStorageBudgetPlan {
    normalized_record_count: usize,
    direct_path_storage_byte_count: usize,
    transition_path_storage_byte_count: usize,
    direct_leading_target_byte_count: usize,
    transition_leading_target_byte_count: usize,
    duplicated_common_body_byte_count: usize,
    additional_storage_upper_bound_byte_count: usize,
    planned_storage_upper_bound_byte_count: usize,
    source_owned_storage_byte_count: usize,
    fits_global_source_owned_storage: bool,
    differing_leading_page_alignment_record_count: usize,
    same_leading_page_alignment_record_count: usize,
    overflow_region_count: usize,
    maximum_region_deficit_byte_count: usize,
    fits_every_source_region: bool,
    region_budgets: Vec<NormalizedStorageRegionBudget>,
    overflow_bank_count: usize,
    maximum_bank_deficit_byte_count: usize,
    pub(super) fits_every_source_bank: bool,
    bank_budgets: Vec<NormalizedStorageBankBudget>,
    storage_strategy_status: &'static str,
}

#[derive(Serialize)]
struct NormalizedStorageRegionBudget {
    source_prg_bank: u8,
    source_prg_bank_hex: String,
    capacity_byte_count: usize,
    baseline_used_byte_count: usize,
    normalized_record_count: usize,
    additional_storage_upper_bound_byte_count: usize,
    planned_storage_upper_bound_byte_count: usize,
    deficit_byte_count: usize,
}

#[derive(Serialize)]
struct NormalizedStorageBankBudget {
    source_prg_bank: u8,
    source_prg_bank_hex: String,
    capacity_byte_count: usize,
    baseline_used_byte_count: usize,
    normalized_record_count: usize,
    additional_storage_upper_bound_byte_count: usize,
    planned_storage_upper_bound_byte_count: usize,
    deficit_byte_count: usize,
}

pub(super) fn plan_normalized_storage_budget(
    dialogue: &MainDialogueBundlePlan,
    display: &MainDialogueDisplayPlan,
) -> Result<NormalizedStorageBudgetPlan> {
    let normalized = display
        .normalized_record_storage
        .iter()
        .map(|record| (record.record_id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        normalized.len() == display.dual_entry_record_count,
        "normalized storage budget contains duplicate record IDs"
    );

    let mut covered_normalized_record_count = 0;
    let region_budgets = dialogue
        .region_storage_budgets()
        .into_iter()
        .map(|region| {
            let mut normalized_record_count = 0;
            let mut additional_storage_upper_bound_byte_count = 0;
            for (record_id, baseline_byte_count) in &region.logical_record_byte_counts {
                let Some(record) = normalized.get(record_id.as_str()) else {
                    continue;
                };
                normalized_record_count += 1;
                additional_storage_upper_bound_byte_count += record
                    .transition_storage_byte_count
                    .checked_add(
                        record
                            .direct_storage_byte_count
                            .saturating_sub(*baseline_byte_count),
                    )
                    .context("normalized record storage upper bound overflow")?;
            }
            covered_normalized_record_count += normalized_record_count;
            let planned_storage_upper_bound_byte_count = region
                .used_byte_count
                .checked_add(additional_storage_upper_bound_byte_count)
                .context("normalized region storage upper bound overflow")?;
            Ok(NormalizedStorageRegionBudget {
                source_prg_bank: region.source_prg_bank,
                source_prg_bank_hex: format!("{:02X}", region.source_prg_bank),
                capacity_byte_count: region.capacity_byte_count,
                baseline_used_byte_count: region.used_byte_count,
                normalized_record_count,
                additional_storage_upper_bound_byte_count,
                planned_storage_upper_bound_byte_count,
                deficit_byte_count: planned_storage_upper_bound_byte_count
                    .saturating_sub(region.capacity_byte_count),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        covered_normalized_record_count == normalized.len(),
        "normalized storage budget lost dual-entry records"
    );

    let mut banks = BTreeMap::<u8, (usize, usize, usize, usize)>::new();
    for region in &region_budgets {
        let bank = banks.entry(region.source_prg_bank).or_default();
        bank.0 += region.capacity_byte_count;
        bank.1 += region.baseline_used_byte_count;
        bank.2 += region.normalized_record_count;
        bank.3 += region.additional_storage_upper_bound_byte_count;
    }
    let bank_budgets = banks
        .into_iter()
        .map(
            |(source_prg_bank, (capacity, baseline, record_count, additional))| {
                let planned = baseline + additional;
                NormalizedStorageBankBudget {
                    source_prg_bank,
                    source_prg_bank_hex: format!("{source_prg_bank:02X}"),
                    capacity_byte_count: capacity,
                    baseline_used_byte_count: baseline,
                    normalized_record_count: record_count,
                    additional_storage_upper_bound_byte_count: additional,
                    planned_storage_upper_bound_byte_count: planned,
                    deficit_byte_count: planned.saturating_sub(capacity),
                }
            },
        )
        .collect::<Vec<_>>();

    let source_owned_storage_byte_count = region_budgets
        .iter()
        .map(|region| region.capacity_byte_count)
        .sum();
    let baseline_planned_storage_byte_count = region_budgets
        .iter()
        .map(|region| region.baseline_used_byte_count)
        .sum::<usize>();
    let additional_storage_upper_bound_byte_count = region_budgets
        .iter()
        .map(|region| region.additional_storage_upper_bound_byte_count)
        .sum::<usize>();
    let planned_storage_upper_bound_byte_count = baseline_planned_storage_byte_count
        .checked_add(additional_storage_upper_bound_byte_count)
        .context("global normalized storage upper bound overflow")?;
    let overflow_region_count = region_budgets
        .iter()
        .filter(|region| region.deficit_byte_count > 0)
        .count();
    let overflow_bank_count = bank_budgets
        .iter()
        .filter(|bank| bank.deficit_byte_count > 0)
        .count();
    let fits_every_source_bank = overflow_bank_count == 0;

    Ok(NormalizedStorageBudgetPlan {
        normalized_record_count: normalized.len(),
        direct_path_storage_byte_count: normalized
            .values()
            .map(|record| record.direct_storage_byte_count)
            .sum(),
        transition_path_storage_byte_count: normalized
            .values()
            .map(|record| record.transition_storage_byte_count)
            .sum(),
        direct_leading_target_byte_count: normalized
            .values()
            .map(|record| record.direct_leading_target_byte_count)
            .sum(),
        transition_leading_target_byte_count: normalized
            .values()
            .map(|record| record.transition_leading_target_byte_count)
            .sum(),
        duplicated_common_body_byte_count: normalized
            .values()
            .map(|record| record.common_body_target_byte_count)
            .sum(),
        additional_storage_upper_bound_byte_count,
        planned_storage_upper_bound_byte_count,
        source_owned_storage_byte_count,
        fits_global_source_owned_storage: planned_storage_upper_bound_byte_count
            <= source_owned_storage_byte_count,
        differing_leading_page_alignment_record_count: normalized
            .values()
            .filter(|record| {
                record.direct_leading_line_count % 4 != record.transition_leading_line_count % 4
            })
            .count(),
        same_leading_page_alignment_record_count: normalized
            .values()
            .filter(|record| {
                record.direct_leading_line_count % 4 == record.transition_leading_line_count % 4
            })
            .count(),
        overflow_region_count,
        maximum_region_deficit_byte_count: region_budgets
            .iter()
            .map(|region| region.deficit_byte_count)
            .max()
            .unwrap_or(0),
        fits_every_source_region: overflow_region_count == 0,
        region_budgets,
        overflow_bank_count,
        maximum_bank_deficit_byte_count: bank_budgets
            .iter()
            .map(|bank| bank.deficit_byte_count)
            .max()
            .unwrap_or(0),
        fits_every_source_bank,
        bank_budgets,
        storage_strategy_status: if fits_every_source_bank {
            "duplicated direct and transition display paths fit without compression"
        } else {
            "duplicated display paths fit globally but overflow at least one source PRG bank; choose one global recovery mechanism before pointer binding"
        },
    })
}
