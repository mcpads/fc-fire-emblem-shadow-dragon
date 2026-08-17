use anyhow::{Result, ensure};
use serde::Serialize;

use super::super::{
    chapter_save_projection::ChapterSaveProjectionPlan,
    consumer_installation::ConsumerInstallationPlan,
    cross_domain_material::CrossDomainMaterialPlan,
    ending_record_projection::EndingRecordProjectionPlan,
    fixed_ui_projection::FixedUiProjectionPlan,
    screen_font_residency::FontPageSelectorForwarderPlan,
};

#[derive(Serialize)]
pub(super) struct DomainWriteContribution {
    id: &'static str,
    translation_input_loaded: bool,
    glyph_lifetime_bound: bool,
    storage_and_address_writes_contributed: bool,
    runtime_material_writes_contributed: bool,
    font_supply_writes_contributed: bool,
    carried_consumer_writes_bound_to_exact_candidate: bool,
    new_global_consumer_writes_contributed: bool,
    all_declared_consumer_writes_contributed: bool,
    pub(super) expected_write_count: usize,
    pub(super) complete_for_declared_domain_plan: bool,
}

pub(super) fn domain_contributions(
    required_domains: &[&'static str],
    expected_dialogue_write_count: usize,
    expected_chapter_title_write_count: usize,
    cross_domain_material: &CrossDomainMaterialPlan,
    fixed_ui_projection: &FixedUiProjectionPlan,
    chapter_save_projection: &ChapterSaveProjectionPlan,
    ending_record_projection: &EndingRecordProjectionPlan,
    font_page_selector_forwarders: &FontPageSelectorForwarderPlan,
    consumer_installation: &ConsumerInstallationPlan,
) -> Result<Vec<DomainWriteContribution>> {
    ensure!(
        required_domains.len() == 14
            && required_domains.contains(&"main_dialogue")
            && required_domains
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == required_domains.len(),
        "integrated write set requires thirteen unique domains including main dialogue"
    );
    let material_sections = cross_domain_material
        .sections()
        .iter()
        .map(|section| section.id)
        .collect::<std::collections::BTreeSet<_>>();
    ensure!(
        material_sections.len() + 1 == required_domains.len()
            && required_domains
                .iter()
                .filter(|id| **id != "main_dialogue")
                .all(|id| material_sections.contains(id)),
        "cross-domain material does not cover every required non-dialogue domain"
    );
    Ok(required_domains
        .iter()
        .map(|id| {
            let dialogue = *id == "main_dialogue";
            let chapter_titles = *id == "chapter_titles";
            let material = material_sections.contains(id);
            let fixed_ui_write_count = fixed_ui_projection.write_count_for_domain(id);
            let chapter_save_write_count = chapter_save_projection.write_count_for_domain(id);
            let ending_record_write_count = ending_record_projection.write_count_for_domain(id);
            let selector_forwarder_write_count =
                font_page_selector_forwarders.write_count_for_domain(id);
            let all_declared_consumers_statically_accounted =
                consumer_installation.domain_has_all_declared_consumers_statically_accounted(id);
            DomainWriteContribution {
                id,
                translation_input_loaded: true,
                glyph_lifetime_bound: true,
                storage_and_address_writes_contributed: dialogue
                    || chapter_titles
                    || fixed_ui_write_count != 0
                    || chapter_save_write_count != 0
                    || ending_record_write_count != 0
                    || selector_forwarder_write_count != 0
                    || all_declared_consumers_statically_accounted,
                runtime_material_writes_contributed: dialogue || material,
                font_supply_writes_contributed: true,
                carried_consumer_writes_bound_to_exact_candidate: consumer_installation
                    .domain_has_carried_consumers(id),
                new_global_consumer_writes_contributed: consumer_installation
                    .domain_has_newly_planned_consumers(id),
                all_declared_consumer_writes_contributed:
                    all_declared_consumers_statically_accounted,
                expected_write_count: usize::from(material)
                    + fixed_ui_write_count
                    + chapter_save_write_count
                    + ending_record_write_count
                    + selector_forwarder_write_count
                    + if dialogue {
                        expected_dialogue_write_count
                    } else if chapter_titles {
                        expected_chapter_title_write_count
                    } else {
                        0
                    },
                complete_for_declared_domain_plan: all_declared_consumers_statically_accounted,
            }
        })
        .collect())
}
