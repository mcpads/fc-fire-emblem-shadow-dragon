use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{rom::Rom, tracked::TrackedImage};

use super::{
    installation_layout::main_dialogue_runtime_material_file_offset,
    runtime_code::DialogueRuntimeCodePlan,
    runtime_code::chr_selector::SELECTOR_CHAIN_SITE,
    runtime_code::dispatcher_gate::{COLD_ENTRY, DISPATCHER_ENTRY},
    runtime_nmi_contract::CONSUMER_HOOK,
};
use crate::dialogue_inventory::switchable_cpu_to_file_offset;

/// 대사 뱅크다.
const MAIN_DIALOGUE_BANK: u8 = 0x0A;
const FIXED_BANK_SIZE: usize = 16 * 1024;

pub(super) struct IntegratedWriteSetInputs<'a> {
    pub(super) candidate: &'a Rom,
    pub(super) dialogue_runtime_material: &'a [u8],
    pub(super) dialogue_runtime_code: &'a DialogueRuntimeCodePlan,
    pub(super) required_domains: &'a [&'static str],
    pub(super) expected_dialogue_storage_write_count: usize,
}

#[derive(Serialize)]
pub(super) struct IntegratedWriteSetPlan {
    required_domain_count: usize,
    domains: Vec<DomainWriteContribution>,
    contributing_domain_count: usize,
    fully_planned_domain_count: usize,
    expected_write_count: usize,
    dialogue_runtime_hook_count: usize,
    dialogue_runtime_fixed_routine_count: usize,
    changed_byte_count: usize,
    every_change_tracked: bool,
    one_shared_image: bool,
    all_domains_contribute_expected_writes: bool,
    output_materialized_in_memory_only: bool,
    rom_emitted: bool,
}

#[derive(Serialize)]
struct DomainWriteContribution {
    id: &'static str,
    translation_input_loaded: bool,
    glyph_lifetime_bound: bool,
    storage_and_address_writes_contributed: bool,
    runtime_material_writes_contributed: bool,
    font_supply_writes_contributed: bool,
    all_consumer_writes_contributed: bool,
    expected_write_count: usize,
    complete_in_integrated_plan: bool,
}

pub(super) fn plan_integrated_write_set(
    inputs: IntegratedWriteSetInputs<'_>,
) -> Result<IntegratedWriteSetPlan> {
    let mut image = TrackedImage::new(inputs.candidate.data().to_vec());
    ensure!(
        image.writes().len() == inputs.expected_dialogue_storage_write_count,
        "integrated write set and complete dialogue write set disagree"
    );
    let runtime_material_offset = main_dialogue_runtime_material_file_offset()?;
    let runtime_material_end = runtime_material_offset
        .checked_add(inputs.dialogue_runtime_material.len())
        .ok_or_else(|| anyhow::anyhow!("dialogue runtime material range overflow"))?;
    let expected_runtime_material = inputs
        .candidate
        .data()
        .get(runtime_material_offset..runtime_material_end)
        .ok_or_else(|| anyhow::anyhow!("dialogue runtime material is outside candidate"))?;
    ensure!(
        expected_runtime_material.iter().all(|byte| *byte == 0xFF),
        "dialogue runtime material destination is not exact FF"
    );
    image.write_expected(
        "main dialogue runtime material",
        runtime_material_offset,
        expected_runtime_material,
        inputs.dialogue_runtime_material,
    )?;
    // 고정 뱅크 동굴의 조각들이다. 자리가 아직 `FF`여야 원본을 덮지 않는다.
    for routine in &inputs.dialogue_runtime_code.fixed_routines {
        let offset = fixed_file_offset(inputs.candidate, routine.address)?;
        let existing = inputs
            .candidate
            .data()
            .get(offset..offset + routine.bytes.len())
            .ok_or_else(|| anyhow::anyhow!("{} is outside the candidate", routine.role))?;
        ensure!(
            existing.iter().all(|byte| *byte == 0xFF),
            "{} would overwrite bytes that are not reserved",
            routine.role
        );
        image.write_expected(routine.role, offset, existing, &routine.bytes)?;
    }

    // 훅 셋이다. 각각 밀어낼 원본 호출을 정확히 알고 있어야 한다.
    let hooks: [(&str, usize, [u8; 3]); 4] = [
        (
            "dialogue CHR selector hook",
            fixed_file_offset(inputs.candidate, SELECTOR_CHAIN_SITE)?,
            inputs.dialogue_runtime_code.selector_hook,
        ),
        (
            "dialogue consumer hook",
            fixed_file_offset(inputs.candidate, CONSUMER_HOOK)?,
            inputs.dialogue_runtime_code.consumer_hook,
        ),
        (
            "dialogue dispatcher hook",
            switchable_cpu_to_file_offset(MAIN_DIALOGUE_BANK, DISPATCHER_ENTRY)?,
            inputs.dialogue_runtime_code.dispatcher_hook,
        ),
        (
            "dialogue cold initializer hook",
            switchable_cpu_to_file_offset(MAIN_DIALOGUE_BANK, COLD_ENTRY)?,
            inputs.dialogue_runtime_code.cold_hook,
        ),
    ];
    for (role, offset, bytes) in hooks {
        let existing = inputs
            .candidate
            .data()
            .get(offset..offset + bytes.len())
            .ok_or_else(|| anyhow::anyhow!("{role} is outside the candidate"))?;
        ensure!(
            existing != bytes,
            "{role} is already installed; the candidate is not a clean base"
        );
        image.write_expected(role, offset, existing, &bytes)?;
    }

    image.verify_all_changes_tracked(inputs.candidate.data())?;
    let expected_write_count = image.writes().len();
    let output = image.into_data();
    let changed_byte_count = inputs
        .candidate
        .data()
        .iter()
        .zip(&output)
        .filter(|(before, after)| before != after)
        .count();

    let domains = domain_contributions(
        inputs.required_domains,
        inputs.expected_dialogue_storage_write_count + 1,
    )?;
    let contributing_domain_count = domains
        .iter()
        .filter(|domain| domain.expected_write_count != 0)
        .count();
    let fully_planned_domain_count = domains
        .iter()
        .filter(|domain| domain.complete_in_integrated_plan)
        .count();
    ensure!(
        contributing_domain_count == 1 && fully_planned_domain_count == 0,
        "integrated write gate advanced without every domain layer"
    );

    Ok(IntegratedWriteSetPlan {
        required_domain_count: inputs.required_domains.len(),
        domains,
        contributing_domain_count,
        fully_planned_domain_count,
        expected_write_count,
        dialogue_runtime_hook_count: 4,
        dialogue_runtime_fixed_routine_count: inputs.dialogue_runtime_code.fixed_routines.len(),
        changed_byte_count,
        every_change_tracked: true,
        one_shared_image: true,
        all_domains_contribute_expected_writes: false,
        output_materialized_in_memory_only: true,
        rom_emitted: false,
    })
}

fn fixed_file_offset(rom: &Rom, address: u16) -> Result<usize> {
    ensure!(address >= 0xC000, "fixed-bank address is below C000");
    let base = rom
        .prg()
        .len()
        .checked_sub(FIXED_BANK_SIZE)
        .ok_or_else(|| anyhow::anyhow!("PRG is smaller than one fixed bank"))?;
    Ok(crate::rom::HEADER_SIZE + base + usize::from(address) - 0xC000)
}

fn domain_contributions(
    required_domains: &[&'static str],
    expected_dialogue_write_count: usize,
) -> Result<Vec<DomainWriteContribution>> {
    ensure!(
        required_domains.len() == 13
            && required_domains.contains(&"main_dialogue")
            && required_domains
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == required_domains.len(),
        "integrated write set requires thirteen unique domains including main dialogue"
    );
    Ok(required_domains
        .iter()
        .map(|id| {
            let dialogue = *id == "main_dialogue";
            DomainWriteContribution {
                id,
                translation_input_loaded: true,
                glyph_lifetime_bound: true,
                storage_and_address_writes_contributed: dialogue,
                runtime_material_writes_contributed: dialogue,
                font_supply_writes_contributed: false,
                all_consumer_writes_contributed: false,
                expected_write_count: if dialogue {
                    expected_dialogue_write_count
                } else {
                    0
                },
                complete_in_integrated_plan: false,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_only_dialogue_contribution_does_not_count_as_a_complete_domain() {
        let domains = domain_contributions(
            &[
                "chapter_save_offer_label",
                "chapter_titles",
                "choice_labels",
                "class_names",
                "ending_record_labels",
                "enemy_names",
                "item_action_labels",
                "item_names",
                "location_names",
                "main_dialogue",
                "map_menu_labels",
                "unit_names",
                "unit_ui_labels",
            ],
            538,
        )
        .unwrap();

        assert_eq!(
            domains
                .iter()
                .filter(|domain| domain.expected_write_count != 0)
                .count(),
            1
        );
        assert!(
            domains
                .iter()
                .all(|domain| !domain.complete_in_integrated_plan)
        );
    }
}
