use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::EncodedMainDialogueBundle, font_slots::FONT_PAGE_SIZE, rom::Rom, sha1_hex,
    tracked::TrackedImage,
};

use super::{
    chapter_intro_residency::EncodedChapterTitle,
    chapter_save_projection::ChapterSaveProjectionPlan,
    cold_request_presentation::ColdRequestPresentationPage,
    consumer_catalog::ConsumerCatalogPlan,
    consumer_codebook::ConsumerCodebookPlan,
    consumer_installation::ConsumerInstallationPlan,
    cross_domain_material::CrossDomainMaterialPlan,
    ending_record_projection::EndingRecordProjectionPlan,
    fixed_ui_projection::FixedUiProjectionPlan,
    runtime_code::{DialogueRuntimeCodePlan, DialogueRuntimeHookRole, DialogueRuntimeHookSite},
    screen_font_residency::FontPageSelectorForwarderPlan,
};
use crate::dialogue_inventory::switchable_cpu_to_file_offset;

const FIXED_BANK_SIZE: usize = 16 * 1024;
const MMC3_PAGE_BYTE_COUNT: usize = 8 * 1024;
const RUNTIME_CODE_WINDOW_START: u16 = 0xA000;
const CHR_APPEND_FILL_BYTE: u8 = 0xFF;
const CHR_APPEND_ROLE: &str = "append integrated candidate CHR capacity";
const CHR_HEADER_ROLE: &str = "expand integrated candidate CHR";

pub(super) struct IntegratedWriteSetInputs<'a> {
    pub(super) candidate: &'a Rom,
    pub(super) encoded_dialogue: &'a EncodedMainDialogueBundle,
    pub(super) dialogue_runtime_material: &'a [u8],
    pub(super) dialogue_runtime_code: &'a DialogueRuntimeCodePlan,
    pub(super) encoded_chapter_titles: &'a [EncodedChapterTitle],
    pub(super) cold_request_presentation: &'a ColdRequestPresentationPage,
    pub(super) consumer_codebook: &'a ConsumerCodebookPlan,
    pub(super) consumer_catalog: &'a ConsumerCatalogPlan,
    pub(super) cross_domain_material: &'a CrossDomainMaterialPlan,
    pub(super) fixed_ui_projection: &'a FixedUiProjectionPlan,
    pub(super) chapter_save_projection: &'a ChapterSaveProjectionPlan,
    pub(super) ending_record_projection: &'a EndingRecordProjectionPlan,
    pub(super) font_page_selector_forwarders: &'a FontPageSelectorForwarderPlan,
    pub(super) consumer_installation: &'a ConsumerInstallationPlan,
    pub(super) required_domains: &'a [&'static str],
    pub(super) all_required_dialogue_runtime_hook_roles_assembled: bool,
    pub(super) output_will_be_emitted: bool,
}

#[derive(Serialize)]
pub(super) struct IntegratedWriteSetPlan {
    declared_domain_count: usize,
    domains: Vec<DomainWriteContribution>,
    declared_domain_with_expected_writes_count: usize,
    statically_accounted_declared_domain_count: usize,
    original_candidate_sha1: String,
    original_candidate_byte_count: usize,
    expanded_baseline_sha1: String,
    planned_final_image_byte_count: usize,
    original_chr_bank_count: u8,
    final_chr_bank_count: u8,
    planned_appended_chr_byte_count: usize,
    actual_appended_chr_byte_count: usize,
    required_mutation_identity_count: usize,
    actual_mutation_identity_count: usize,
    expected_write_count: usize,
    required_mutation_identity_sha1: String,
    actual_mutation_identity_sha1: String,
    required_runtime_routine_identity_count: usize,
    required_runtime_hook_identity_count: usize,
    required_runtime_state_initializer_identity_count: usize,
    actual_runtime_state_initializer_identity_count: usize,
    runtime_state_initializer_preserves_consumer_font_page: bool,
    runtime_state_initializer_installed: bool,
    dialogue_runtime_hook_count: usize,
    dialogue_runtime_hook_roles: Vec<DialogueRuntimeHookRole>,
    dialogue_runtime_fixed_routine_count: usize,
    dialogue_runtime_code_routine_count: usize,
    dialogue_storage_region_count: usize,
    dialogue_pointer_write_count: usize,
    chapter_title_storage_write_count: usize,
    cold_request_presentation_write_count: usize,
    chr_expansion_header_write_count: usize,
    appended_chr_page_count: usize,
    static_consumer_font_page_write_count: usize,
    catalog_consumer_font_page_write_count: usize,
    cross_domain_material_write_count: usize,
    fixed_ui_projection_write_count: usize,
    chapter_save_projection_write_count: usize,
    ending_record_projection_write_count: usize,
    font_page_selector_forwarder_write_count: usize,
    installed_cold_request_presentation_matches_plan: bool,
    installed_static_consumer_font_pages_match_plan: bool,
    installed_catalog_consumer_font_pages_match_plan: bool,
    installed_cross_domain_material_matches_plan: bool,
    installed_ending_record_projection_matches_plan: bool,
    changed_byte_count: usize,
    installed_dialogue_matches_current_encoding: bool,
    installed_chapter_titles_match_resident_encoding: bool,
    every_change_tracked: bool,
    image_growth_complete: bool,
    required_mutation_identity_set_complete: bool,
    required_runtime_routine_identities_installed: bool,
    required_runtime_hook_identities_installed: bool,
    final_replacement_bytes_match_manifest: bool,
    technical_installation_complete: bool,
    one_shared_image: bool,
    all_declared_domains_contribute_expected_writes: bool,
    integrated_image_sha1: String,
    output_materialized_in_memory_only: bool,
    rom_emitted: bool,
}

mod chapter_title_storage;
mod cross_domain_material_installation;
mod dialogue_storage;
mod domain_contributions;
mod fixed_projections;
mod font_pages;
mod runtime_material;
mod technical_installation;

use chapter_title_storage::{
    active_fixed_mirror_file_offset, install_encoded_chapter_titles,
    verify_installed_chapter_titles,
};
use cross_domain_material_installation::{
    install_cross_domain_material, verify_installed_cross_domain_material,
};
use dialogue_storage::{install_encoded_dialogue, verify_installed_dialogue};
use domain_contributions::{
    DomainContributionInputs, DomainWriteContribution, domain_contributions,
};
use fixed_projections::{
    fixed_file_offset, install_chapter_save_projection, install_ending_record_projection,
    install_fixed_ui_projection, install_font_page_selector_forwarders,
    verify_installed_chapter_save_projection, verify_installed_ending_record_projection,
    verify_installed_fixed_ui_projection, verify_installed_font_page_selector_forwarders,
};
use font_pages::{
    cold_request_presentation_file_offset, install_catalog_consumer_font_pages,
    install_cold_request_presentation, install_static_consumer_font_pages,
    static_consumer_page_file_offset, verify_installed_catalog_consumer_font_pages,
    verify_installed_cold_request_presentation, verify_installed_static_consumer_font_pages,
};
use runtime_material::install_dialogue_runtime_material;

use technical_installation::{
    IntegratedImage, MutationDerivation, TechnicalInstallationCheckInputs,
    plan_candidate_image_growth, plan_required_mutation_identities, runtime_hook_file_offset,
    runtime_hook_site_identity, verify_runtime_state_initializer_installation,
    verify_technical_installation,
};
pub(super) fn plan_integrated_write_set(
    inputs: IntegratedWriteSetInputs<'_>,
) -> Result<(Vec<u8>, IntegratedWriteSetPlan)> {
    let image_growth = plan_candidate_image_growth(&inputs)?;
    let expanded_base = image_growth.apply(inputs.candidate.data())?;
    let required_mutations =
        plan_required_mutation_identities(&inputs, &image_growth, &expanded_base)?;
    let mut image = IntegratedImage::new(expanded_base.clone(), image_growth.append_identity());
    let appended_chr_page_count = image_growth.appended_chr_page_count;
    let chr_expansion_header_write_count = usize::from(appended_chr_page_count != 0);
    if appended_chr_page_count != 0 {
        image.write_expected(
            CHR_HEADER_ROLE,
            5,
            &[inputs.candidate.data()[5]],
            &[image_growth.final_chr_bank_count],
        )?;
    }
    let dialogue_storage_write_count =
        inputs.encoded_dialogue.regions.len() + inputs.encoded_dialogue.pointer_writes.len();
    let chapter_title_storage_write_count = inputs.encoded_chapter_titles.len() * 2;
    let dialogue_runtime_fixed_routine_count = inputs.dialogue_runtime_code.fixed_routines.len()
        + inputs.dialogue_runtime_code.reclaimed_fixed_routines.len();
    install_encoded_dialogue(&mut image, inputs.candidate, inputs.encoded_dialogue)?;
    install_encoded_chapter_titles(&mut image, inputs.candidate, inputs.encoded_chapter_titles)?;
    ensure!(
        image.writes().len()
            == chr_expansion_header_write_count
                + dialogue_storage_write_count
                + chapter_title_storage_write_count,
        "integrated write set and dialogue/title storage write sets disagree"
    );
    install_cold_request_presentation(
        &mut image,
        inputs.candidate,
        &expanded_base,
        inputs.cold_request_presentation,
    )?;
    install_static_consumer_font_pages(
        &mut image,
        inputs.candidate,
        &expanded_base,
        inputs.consumer_codebook,
    )?;
    install_catalog_consumer_font_pages(
        &mut image,
        inputs.candidate,
        &expanded_base,
        inputs.consumer_catalog,
    )?;
    install_dialogue_runtime_material(
        &mut image,
        inputs.candidate,
        inputs.dialogue_runtime_material,
        inputs.dialogue_runtime_code,
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
        image.write_runtime_routine(
            routine.role,
            offset,
            existing,
            &routine.bytes,
            routine.address,
        )?;
    }
    // 표본 전용 코드가 차지한 구간은 `FF` 동굴로 가장하지 않는다. 계획이 고정한
    // 전체 digest가 맞는 경우에만 전역 런타임으로 대체한다.
    for reclaimed in &inputs.dialogue_runtime_code.reclaimed_fixed_routines {
        let routine = &reclaimed.routine;
        let capacity = usize::from(
            reclaimed
                .source_end_exclusive
                .checked_sub(routine.address)
                .ok_or_else(|| anyhow::anyhow!("{} reclaimed range is reversed", routine.role))?,
        );
        ensure!(
            routine.bytes.len() == capacity,
            "{} must replace its whole reclaimed source range",
            routine.role
        );
        let offset = fixed_file_offset(inputs.candidate, routine.address)?;
        let existing = inputs
            .candidate
            .data()
            .get(offset..offset + capacity)
            .ok_or_else(|| anyhow::anyhow!("{} is outside the candidate", routine.role))?;
        ensure!(
            crate::sha1_hex(existing) == reclaimed.expected_source_sha1.as_str(),
            "{} source digest changed",
            routine.role
        );
        image.write_runtime_routine(
            routine.role,
            offset,
            existing,
            &routine.bytes,
            routine.address,
        )?;
    }

    // 훅 역할과 원본 자리와 쓸 바이트는 코드 계획이 한 단위로 제공한다. 설치자가
    // 별도 배열로 다시 세면 새 훅을 추가할 때 보고서와 실제 쓰기가 갈라진다.
    let mut installed_hook_roles = BTreeSet::new();
    for hook in &inputs.dialogue_runtime_code.hooks {
        ensure!(
            installed_hook_roles.insert(hook.role),
            "dialogue runtime hook role {:?} is emitted more than once",
            hook.role
        );
        let site = runtime_hook_site_identity(&hook.site);
        let offset = runtime_hook_file_offset(inputs.candidate, site)?;
        let existing = inputs
            .candidate
            .data()
            .get(offset..offset + hook.bytes.len())
            .ok_or_else(|| anyhow::anyhow!("{} is outside the candidate", hook.write_role))?;
        ensure!(
            existing != hook.bytes,
            "{} is already installed; the candidate is not a clean base",
            hook.write_role
        );
        image.write_runtime_hook(
            hook.write_role,
            offset,
            existing,
            &hook.bytes,
            hook.role,
            site,
        )?;
    }

    install_fixed_ui_projection(&mut image, inputs.fixed_ui_projection)?;
    install_chapter_save_projection(&mut image, inputs.chapter_save_projection)?;
    install_ending_record_projection(&mut image, inputs.ending_record_projection)?;
    install_font_page_selector_forwarders(
        &mut image,
        inputs.candidate,
        inputs.font_page_selector_forwarders,
    )?;

    let expected_write_count_before_cross_domain = image.writes().len();
    install_cross_domain_material(&mut image, inputs.candidate, inputs.cross_domain_material)?;

    image.verify_all_changes_tracked(&expanded_base)?;
    let expected_write_count = image.writes().len();
    let actual_mutations = image.mutation_identities().to_vec();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse integrated mapper output")?;
    crate::mapper165::selector_safety::verify_final_installed_contract(&output_rom)?;
    super::runtime_code::verify_installed_chr_ram_ownership_gate(&output_rom)?;
    verify_installed_dialogue(&output, inputs.encoded_dialogue)?;
    verify_installed_chapter_titles(&output, inputs.candidate, inputs.encoded_chapter_titles)?;
    verify_installed_cold_request_presentation(
        &output,
        inputs.candidate,
        inputs.cold_request_presentation,
    )?;
    verify_installed_static_consumer_font_pages(
        &output,
        inputs.candidate,
        inputs.consumer_codebook,
    )?;
    verify_installed_catalog_consumer_font_pages(
        &output,
        inputs.candidate,
        inputs.consumer_catalog,
    )?;
    verify_installed_cross_domain_material(&output, inputs.cross_domain_material)?;
    verify_installed_fixed_ui_projection(&output, inputs.fixed_ui_projection)?;
    verify_installed_chapter_save_projection(&output, inputs.chapter_save_projection)?;
    verify_installed_ending_record_projection(&output, inputs.ending_record_projection)?;
    verify_installed_font_page_selector_forwarders(
        &output,
        inputs.candidate,
        inputs.font_page_selector_forwarders,
    )?;
    let runtime_state_initializer = verify_runtime_state_initializer_installation(
        &required_mutations,
        &actual_mutations,
        &output,
    )?;
    let technical_installation = verify_technical_installation(TechnicalInstallationCheckInputs {
        source: inputs.candidate.data(),
        installed: &output,
        required_mutations: &required_mutations,
        actual_mutations: &actual_mutations,
        tracked_write_count: expected_write_count,
        all_required_dialogue_runtime_hook_roles_assembled: inputs
            .all_required_dialogue_runtime_hook_roles_assembled,
        runtime_state_initializer_installed: runtime_state_initializer.installed,
    })?;
    let installed_image = output.clone();
    let changed_byte_count = inputs
        .candidate
        .data()
        .iter()
        .zip(&output)
        .filter(|(before, after)| before != after)
        .count()
        + output.len()
        - inputs.candidate.data().len();

    let domains = domain_contributions(DomainContributionInputs {
        required_domains: inputs.required_domains,
        expected_dialogue_write_count: expected_write_count_before_cross_domain
            - chapter_title_storage_write_count,
        expected_chapter_title_write_count: chapter_title_storage_write_count,
        cross_domain_material: inputs.cross_domain_material,
        fixed_ui_projection: inputs.fixed_ui_projection,
        chapter_save_projection: inputs.chapter_save_projection,
        ending_record_projection: inputs.ending_record_projection,
        font_page_selector_forwarders: inputs.font_page_selector_forwarders,
        consumer_installation: inputs.consumer_installation,
    })?;
    let declared_domain_with_expected_writes_count = domains
        .iter()
        .filter(|domain| domain.expected_write_count != 0)
        .count();
    let statically_accounted_declared_domain_count = domains
        .iter()
        .filter(|domain| domain.complete_for_declared_domain_plan)
        .count();
    ensure!(
        declared_domain_with_expected_writes_count == inputs.required_domains.len()
            && statically_accounted_declared_domain_count
                == inputs
                    .consumer_installation
                    .statically_accounted_declared_domain_count(),
        "integrated write gate advanced without every domain layer"
    );
    let integrated_image_sha1 = crate::sha1_hex(&installed_image);
    let actual_appended_chr_byte_count = installed_image.len() - inputs.candidate.data().len();

    Ok((
        installed_image,
        IntegratedWriteSetPlan {
            declared_domain_count: inputs.required_domains.len(),
            domains,
            declared_domain_with_expected_writes_count,
            statically_accounted_declared_domain_count,
            original_candidate_sha1: sha1_hex(inputs.candidate.data()),
            original_candidate_byte_count: inputs.candidate.data().len(),
            expanded_baseline_sha1: sha1_hex(&expanded_base),
            planned_final_image_byte_count: image_growth.final_byte_count,
            original_chr_bank_count: inputs.candidate.data()[5],
            final_chr_bank_count: image_growth.final_chr_bank_count,
            planned_appended_chr_byte_count: image_growth.appended_chr_byte_count,
            actual_appended_chr_byte_count,
            required_mutation_identity_count: required_mutations.len(),
            actual_mutation_identity_count: actual_mutations.len(),
            expected_write_count,
            required_mutation_identity_sha1: technical_installation.required_mutation_identity_sha1,
            actual_mutation_identity_sha1: technical_installation.actual_mutation_identity_sha1,
            required_runtime_routine_identity_count: required_mutations
                .iter()
                .filter(|identity| {
                    matches!(
                        identity.derivation,
                        MutationDerivation::RuntimeRoutine { .. }
                    )
                })
                .count(),
            required_runtime_hook_identity_count: required_mutations
                .iter()
                .filter(|identity| {
                    matches!(identity.derivation, MutationDerivation::RuntimeHook { .. })
                })
                .count(),
            required_runtime_state_initializer_identity_count: runtime_state_initializer
                .required_identity_count,
            actual_runtime_state_initializer_identity_count: runtime_state_initializer
                .actual_identity_count,
            runtime_state_initializer_preserves_consumer_font_page: runtime_state_initializer
                .preserves_consumer_font_page,
            runtime_state_initializer_installed: runtime_state_initializer.installed,
            dialogue_runtime_hook_count: installed_hook_roles.len(),
            dialogue_runtime_hook_roles: installed_hook_roles.into_iter().collect(),
            dialogue_runtime_fixed_routine_count,
            dialogue_runtime_code_routine_count: inputs.dialogue_runtime_code.code_routines.len(),
            dialogue_storage_region_count: inputs.encoded_dialogue.regions.len(),
            dialogue_pointer_write_count: inputs.encoded_dialogue.pointer_writes.len(),
            chapter_title_storage_write_count,
            cold_request_presentation_write_count: 1,
            chr_expansion_header_write_count,
            appended_chr_page_count,
            static_consumer_font_page_write_count: inputs.consumer_codebook.pages().len(),
            catalog_consumer_font_page_write_count: inputs.consumer_catalog.pages().len(),
            cross_domain_material_write_count: inputs.cross_domain_material.sections().len() + 2,
            fixed_ui_projection_write_count: inputs.fixed_ui_projection.write_count(),
            chapter_save_projection_write_count: inputs.chapter_save_projection.write_count(),
            ending_record_projection_write_count: inputs.ending_record_projection.write_count(),
            font_page_selector_forwarder_write_count: inputs
                .font_page_selector_forwarders
                .write_count(),
            installed_cold_request_presentation_matches_plan: true,
            installed_static_consumer_font_pages_match_plan: true,
            installed_catalog_consumer_font_pages_match_plan: true,
            installed_cross_domain_material_matches_plan: true,
            installed_ending_record_projection_matches_plan: true,
            changed_byte_count,
            installed_dialogue_matches_current_encoding: true,
            installed_chapter_titles_match_resident_encoding: true,
            every_change_tracked: technical_installation.every_change_tracked,
            image_growth_complete: technical_installation.image_growth_complete,
            required_mutation_identity_set_complete: technical_installation
                .required_mutation_identity_set_complete,
            required_runtime_routine_identities_installed: technical_installation
                .required_runtime_routine_identities_installed,
            required_runtime_hook_identities_installed: technical_installation
                .required_runtime_hook_identities_installed,
            final_replacement_bytes_match_manifest: technical_installation
                .final_replacement_bytes_match_manifest,
            technical_installation_complete: technical_installation.technical_installation_complete,
            one_shared_image: true,
            all_declared_domains_contribute_expected_writes: true,
            integrated_image_sha1,
            output_materialized_in_memory_only: !inputs.output_will_be_emitted,
            rom_emitted: inputs.output_will_be_emitted,
        },
    ))
}

impl IntegratedWriteSetPlan {
    pub(super) fn technical_installation_complete(&self) -> bool {
        self.technical_installation_complete
    }
}

#[cfg(test)]
mod tests;
