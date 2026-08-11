use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::plan_main_dialogue_bundle,
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, active_hangul_codes},
    item_flow::{plan_item_action_labels, validate_item_lifetime_source},
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::{FixedTextLogicalByte, FixedTextPlannedEntry, plan_fixed_text},
    unit_names::plan_unit_names,
};

use super::super::report::TranslationLifetimeDemandReport;

const MAXIMUM_VISIBLE_ITEM_COUNT: usize = 4;

pub(super) struct InputBindings<'a> {
    pub(super) source_path: &'a Path,
    pub(super) main_dialogue_workspace_path: &'a Path,
    pub(super) fixed_text_workspace_path: &'a Path,
    pub(super) unit_name_workspace_path: &'a Path,
    pub(super) item_action_label_workspace_path: &'a Path,
    pub(super) main_dialogue_workspace_sha1: &'a str,
    pub(super) fixed_text_workspace_sha1: &'a str,
    pub(super) unit_name_workspace_sha1: &'a str,
    pub(super) item_action_label_workspace_sha1: &'a str,
}

struct Measurements {
    maximum_item_name_glyph_count: usize,
    maximum_unit_name_glyph_count: usize,
    action_label_union_glyph_count: usize,
    inventory_preserved_active_code_count: usize,
    equip_dialogue_glyph_count: usize,
    equip_preserved_active_code_count: usize,
    transfer_dialogue_glyph_count: usize,
    transfer_preserved_active_code_count: usize,
    discard_dialogue_glyph_count: usize,
    discard_preserved_active_code_count: usize,
}

#[derive(Serialize)]
struct EvidenceDigest<'a> {
    schema: u8,
    source_sha1: &'static str,
    main_dialogue_workspace_sha1: &'a str,
    fixed_text_workspace_sha1: &'a str,
    unit_name_workspace_sha1: &'a str,
    item_action_label_workspace_sha1: &'a str,
    maximum_item_name_glyph_count: usize,
    maximum_unit_name_glyph_count: usize,
    action_label_union_glyph_count: usize,
    measured_screen_roles: [&'static str; 5],
    excluded_screen_role: &'static str,
    exclusion_reason: &'static str,
}

pub(super) fn inspect(bindings: InputBindings<'_>) -> Result<Vec<TranslationLifetimeDemandReport>> {
    let rom = Rom::from_path(bindings.source_path)?;
    rom.verify_supported_japanese()?;
    validate_item_lifetime_source(&rom)?;

    let fixed_text = plan_fixed_text(&rom, bindings.fixed_text_workspace_path)?;
    let unit_names = plan_unit_names(&rom, bindings.unit_name_workspace_path)?;
    let action_labels = plan_item_action_labels(&rom, bindings.item_action_label_workspace_path)?;
    ensure!(
        fixed_text.workspace_sha1 == bindings.fixed_text_workspace_sha1
            && unit_names.workspace_sha1 == bindings.unit_name_workspace_sha1
            && action_labels.workspace_sha1 == bindings.item_action_label_workspace_sha1,
        "item-flow lifetime translation inputs do not match global coverage"
    );

    let item_entries = fixed_text
        .entries
        .iter()
        .filter(|entry| entry.table_id == "item-names")
        .collect::<Vec<_>>();
    ensure!(
        item_entries.len() == 91,
        "item-flow lifetime requires all 91 item names"
    );
    let maximum_item_name_glyph_count = maximum_entry_glyph_count(&item_entries, "item names")?;
    let unit_entries = unit_names.entries.iter().collect::<Vec<_>>();
    let maximum_unit_name_glyph_count = maximum_entry_glyph_count(&unit_entries, "unit names")?;
    let action_label_union_glyph_count = action_labels.unique_target_glyphs().len();
    ensure!(
        action_labels.entry_count == 4 && action_label_union_glyph_count > 0,
        "item-flow action-label population changed"
    );

    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let item_preserved_codes = preserved_active_codes(&item_entries, &active_codes);
    let unit_preserved_codes = preserved_active_codes(&unit_entries, &active_codes);
    let dynamic_preserved_codes = item_preserved_codes
        .union(&unit_preserved_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let equip = dialogue_record(&rom, &bindings, 0x19, &active_codes)?;
    let transfer = dialogue_record(&rom, &bindings, 0x1B, &active_codes)?;
    let discard = dialogue_record(&rom, &bindings, 0x1C, &active_codes)?;

    let measurements = Measurements {
        maximum_item_name_glyph_count,
        maximum_unit_name_glyph_count,
        action_label_union_glyph_count,
        inventory_preserved_active_code_count: item_preserved_codes.len(),
        equip_dialogue_glyph_count: equip.target_glyph_count,
        equip_preserved_active_code_count: union_count(
            &equip.preserved_active_codes,
            &dynamic_preserved_codes,
        ),
        transfer_dialogue_glyph_count: transfer.target_glyph_count,
        transfer_preserved_active_code_count: union_count(
            &transfer.preserved_active_codes,
            &dynamic_preserved_codes,
        ),
        discard_dialogue_glyph_count: discard.target_glyph_count,
        discard_preserved_active_code_count: union_count(
            &discard.preserved_active_codes,
            &dynamic_preserved_codes,
        ),
    };
    let evidence = EvidenceDigest {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        main_dialogue_workspace_sha1: bindings.main_dialogue_workspace_sha1,
        fixed_text_workspace_sha1: bindings.fixed_text_workspace_sha1,
        unit_name_workspace_sha1: bindings.unit_name_workspace_sha1,
        item_action_label_workspace_sha1: bindings.item_action_label_workspace_sha1,
        maximum_item_name_glyph_count,
        maximum_unit_name_glyph_count,
        action_label_union_glyph_count,
        measured_screen_roles: [
            "item_inventory_list",
            "item_action_menu",
            "item_equip_result",
            "item_transfer_result",
            "item_discard_result",
        ],
        excluded_screen_role: "item_use_result",
        exclusion_reason: "successful class-change and earth-orb intermediate surfaces are not runtime-bound",
    };
    let evidence_sha1 = sha1_hex(
        &serde_json::to_vec(&evidence).context("serialize item-flow lifetime evidence digest")?,
    );
    build_demands(measurements, &evidence_sha1)
}

struct DialogueRecordMeasurement {
    target_glyph_count: usize,
    preserved_active_codes: BTreeSet<u8>,
}

fn dialogue_record(
    rom: &Rom,
    bindings: &InputBindings<'_>,
    index: u8,
    active_codes: &BTreeSet<u8>,
) -> Result<DialogueRecordMeasurement> {
    let record_id = format!("shop-and-item-dialogue:{index:03}");
    let plan = plan_main_dialogue_bundle(
        rom,
        bindings.main_dialogue_workspace_path,
        &[record_id.as_str()],
    )?;
    ensure!(
        plan.workspace_sha1 == bindings.main_dialogue_workspace_sha1,
        "item-flow dialogue lifetime does not match global coverage"
    );
    Ok(DialogueRecordMeasurement {
        target_glyph_count: plan.unique_glyphs().len(),
        preserved_active_codes: plan
            .preserved_source_codes
            .intersection(active_codes)
            .copied()
            .collect(),
    })
}

fn maximum_entry_glyph_count(entries: &[&FixedTextPlannedEntry], role: &str) -> Result<usize> {
    entries
        .iter()
        .map(|entry| entry.unique_glyphs().len())
        .max()
        .with_context(|| format!("item-flow lifetime has no {role}"))
}

fn preserved_active_codes(
    entries: &[&FixedTextPlannedEntry],
    active_codes: &BTreeSet<u8>,
) -> BTreeSet<u8> {
    entries
        .iter()
        .flat_map(|entry| &entry.logical_bytes)
        .filter_map(|byte| match byte {
            FixedTextLogicalByte::Encoded(value) if active_codes.contains(value) => Some(*value),
            FixedTextLogicalByte::Encoded(_) | FixedTextLogicalByte::TargetGlyph(_) => None,
        })
        .collect()
}

fn union_count(left: &BTreeSet<u8>, right: &BTreeSet<u8>) -> usize {
    left.union(right).count()
}

fn build_demands(
    measurements: Measurements,
    evidence_sha1: &str,
) -> Result<Vec<TranslationLifetimeDemandReport>> {
    let item_rows = measurements
        .maximum_item_name_glyph_count
        .checked_mul(MAXIMUM_VISIBLE_ITEM_COUNT)
        .context("item inventory glyph upper bound overflow")?;
    let action_menu = item_rows
        .checked_add(measurements.action_label_union_glyph_count)
        .context("item action-menu glyph upper bound overflow")?;
    let one_unit_and_item = measurements
        .maximum_unit_name_glyph_count
        .checked_add(measurements.maximum_item_name_glyph_count)
        .context("item result dynamic glyph reservation overflow")?;
    let two_units_and_item = measurements
        .maximum_unit_name_glyph_count
        .checked_mul(2)
        .and_then(|units| units.checked_add(measurements.maximum_item_name_glyph_count))
        .context("item transfer dynamic glyph reservation overflow")?;

    Ok(vec![
        demand(
            "item_inventory_list",
            "four independently maximal translated item names; original durability digits and NO ITEM stay in reserved slots",
            item_rows,
            measurements.inventory_preserved_active_code_count,
            0,
            evidence_sha1,
        )?,
        demand(
            "item_action_menu",
            "retained four-item upper bound plus the complete four-action label union",
            action_menu,
            measurements.inventory_preserved_active_code_count,
            0,
            evidence_sha1,
        )?,
        demand(
            "item_equip_result",
            "exact translated result record plus independent maxima for one unit and one item name",
            measurements.equip_dialogue_glyph_count,
            measurements.equip_preserved_active_code_count,
            one_unit_and_item,
            evidence_sha1,
        )?,
        demand(
            "item_transfer_result",
            "exact translated result record plus independent maxima for source unit, target unit, and item name",
            measurements.transfer_dialogue_glyph_count,
            measurements.transfer_preserved_active_code_count,
            two_units_and_item,
            evidence_sha1,
        )?,
        demand(
            "item_discard_result",
            "exact translated result record plus independent maxima for one unit and one item name",
            measurements.discard_dialogue_glyph_count,
            measurements.discard_preserved_active_code_count,
            one_unit_and_item,
            evidence_sha1,
        )?,
    ])
}

fn demand(
    screen_role: &'static str,
    measurement_basis: &'static str,
    target_glyph_count: usize,
    preserved_active_source_code_count: usize,
    additional_target_glyph_reservation_count: usize,
    evidence_sha1: &str,
) -> Result<TranslationLifetimeDemandReport> {
    let total_slot_demand = target_glyph_count
        .checked_add(preserved_active_source_code_count)
        .and_then(|count| count.checked_add(additional_target_glyph_reservation_count))
        .context("item-flow lifetime slot demand overflow")?;
    Ok(TranslationLifetimeDemandReport {
        screen_role,
        measurement_basis,
        target_glyph_count,
        preserved_active_source_code_count,
        additional_target_glyph_reservation_count,
        total_slot_demand,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        fits_active_page: total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        evidence_report_sha1: evidence_sha1.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_item_screens_use_complete_upper_bounds() {
        let demands = build_demands(
            Measurements {
                maximum_item_name_glyph_count: 6,
                maximum_unit_name_glyph_count: 4,
                action_label_union_glyph_count: 9,
                inventory_preserved_active_code_count: 0,
                equip_dialogue_glyph_count: 8,
                equip_preserved_active_code_count: 2,
                transfer_dialogue_glyph_count: 7,
                transfer_preserved_active_code_count: 3,
                discard_dialogue_glyph_count: 6,
                discard_preserved_active_code_count: 1,
            },
            "evidence",
        )
        .unwrap();

        assert_eq!(demands.len(), 5);
        assert_eq!(demands[0].total_slot_demand, 24);
        assert_eq!(demands[1].total_slot_demand, 33);
        assert_eq!(demands[2].total_slot_demand, 20);
        assert_eq!(demands[3].total_slot_demand, 24);
        assert_eq!(demands[4].total_slot_demand, 17);
        assert!(
            demands
                .iter()
                .all(|demand| demand.screen_role != "item_use_result")
        );
    }
}
