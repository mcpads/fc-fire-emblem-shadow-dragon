use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::EncodedMainDialogueBundle,
    font_slots::FONT_PAGE_SIZE,
    rom::{HEADER_SIZE, PRG_SIZE, Rom},
    sha1_hex,
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
    installation_layout::main_dialogue_runtime_material_file_offset,
    runtime_code::{DialogueRuntimeCodePlan, DialogueRuntimeHookRole, DialogueRuntimeHookSite},
};
use crate::dialogue_inventory::switchable_cpu_to_file_offset;

const FIXED_BANK_SIZE: usize = 16 * 1024;
const MMC3_PAGE_BYTE_COUNT: usize = 8 * 1024;
const RUNTIME_CODE_WINDOW_START: u16 = 0xA000;
const CHR_APPEND_FILL_BYTE: u8 = 0xFF;
const CHR_APPEND_ROLE: &str = "append integrated candidate CHR capacity";
const CHR_HEADER_ROLE: &str = "expand integrated candidate CHR";
const RUNTIME_MATERIAL_DATA_ROLE: &str = "main dialogue runtime material data";

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

#[derive(Serialize)]
struct DomainWriteContribution {
    id: &'static str,
    translation_input_loaded: bool,
    glyph_lifetime_bound: bool,
    storage_and_address_writes_contributed: bool,
    runtime_material_writes_contributed: bool,
    font_supply_writes_contributed: bool,
    carried_consumer_writes_bound_to_exact_candidate: bool,
    new_global_consumer_writes_contributed: bool,
    all_declared_consumer_writes_contributed: bool,
    expected_write_count: usize,
    complete_for_declared_domain_plan: bool,
}

mod technical_installation;

use technical_installation::{
    IntegratedImage, MutationDerivation, TechnicalInstallationCheckInputs, mutation_expected_slice,
    plan_candidate_image_growth, plan_required_mutation_identities, runtime_hook_file_offset,
    runtime_hook_site_identity, runtime_material_routine_file_offset,
    verify_runtime_material_code_projection, verify_runtime_state_initializer_installation,
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
            crate::sha1_hex(existing) == reclaimed.expected_source_sha1,
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

    let expected_write_count_before_cross_domain = image.writes().len();
    install_cross_domain_material(&mut image, inputs.candidate, inputs.cross_domain_material)?;

    image.verify_all_changes_tracked(&expanded_base)?;
    let expected_write_count = image.writes().len();
    let actual_mutations = image.mutation_identities().to_vec();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse integrated mapper output")?;
    crate::mapper165::selector_safety::verify_final_installed_contract(
        &output_rom,
        super::runtime_code::trampoline::TRAMPOLINE_ORIGIN,
    )?;
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

    let domains = domain_contributions(
        inputs.required_domains,
        expected_write_count_before_cross_domain - chapter_title_storage_write_count,
        chapter_title_storage_write_count,
        inputs.cross_domain_material,
        inputs.fixed_ui_projection,
        inputs.chapter_save_projection,
        inputs.ending_record_projection,
        inputs.consumer_installation,
    )?;
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

fn cold_request_presentation_file_offset(
    candidate: &Rom,
    page: &ColdRequestPresentationPage,
) -> Result<usize> {
    let chr_offset = HEADER_SIZE
        .checked_add(candidate.prg().len())
        .ok_or_else(|| anyhow::anyhow!("candidate CHR offset overflow"))?;
    chr_offset
        .checked_add(usize::from(page.physical_page) * FONT_PAGE_SIZE)
        .ok_or_else(|| anyhow::anyhow!("cold-request presentation offset overflow"))
}

fn install_cold_request_presentation(
    image: &mut IntegratedImage,
    candidate: &Rom,
    baseline: &[u8],
    page: &ColdRequestPresentationPage,
) -> Result<()> {
    ensure!(
        page.bytes.len() == FONT_PAGE_SIZE,
        "cold-request presentation is not one 4 KiB CHR page"
    );
    let offset = cold_request_presentation_file_offset(candidate, page)?;
    let expected = baseline
        .get(offset..offset + FONT_PAGE_SIZE)
        .ok_or_else(|| anyhow::anyhow!("cold-request presentation page is outside candidate"))?;
    image.write_expected(
        "cold-request dialogue presentation CHR page",
        offset,
        expected,
        &page.bytes,
    )?;
    Ok(())
}

fn verify_installed_cold_request_presentation(
    installed: &[u8],
    candidate: &Rom,
    page: &ColdRequestPresentationPage,
) -> Result<()> {
    let offset = cold_request_presentation_file_offset(candidate, page)?;
    ensure!(
        installed.get(offset..offset + FONT_PAGE_SIZE) == Some(page.bytes.as_slice()),
        "installed cold-request presentation page does not match its plan"
    );
    Ok(())
}

fn install_static_consumer_font_pages(
    image: &mut IntegratedImage,
    candidate: &Rom,
    baseline: &[u8],
    plan: &ConsumerCodebookPlan,
) -> Result<()> {
    ensure!(
        plan.pages().len() == 4,
        "integrated consumer codebook must install the four fixed-content pages"
    );
    let mut physical_pages = std::collections::BTreeSet::new();
    for page in plan.pages() {
        ensure!(
            physical_pages.insert(page.physical_page())
                && page.bytes.len() == FONT_PAGE_SIZE
                && page.assignment_count() != 0,
            "static consumer page {} is empty, duplicated, or not 4 KiB",
            page.id
        );
        let offset = static_consumer_page_file_offset(candidate, page.physical_page())?;
        let expected = baseline
            .get(offset..offset + FONT_PAGE_SIZE)
            .ok_or_else(|| {
                anyhow::anyhow!("static consumer page {} is outside candidate", page.id)
            })?;
        image.write_expected(
            format!("static consumer font page {}", page.id),
            offset,
            expected,
            &page.bytes,
        )?;
    }
    Ok(())
}

fn verify_installed_static_consumer_font_pages(
    installed: &[u8],
    candidate: &Rom,
    plan: &ConsumerCodebookPlan,
) -> Result<()> {
    for page in plan.pages() {
        let offset = static_consumer_page_file_offset(candidate, page.physical_page())?;
        ensure!(
            installed.get(offset..offset + FONT_PAGE_SIZE) == Some(page.bytes.as_slice()),
            "installed static consumer page {} does not match its codebook",
            page.id
        );
    }
    Ok(())
}

fn install_catalog_consumer_font_pages(
    image: &mut IntegratedImage,
    candidate: &Rom,
    baseline: &[u8],
    plan: &ConsumerCatalogPlan,
) -> Result<()> {
    ensure!(
        !plan.pages().is_empty(),
        "integrated consumer catalog has no font pages"
    );
    let mut physical_pages = std::collections::BTreeSet::new();
    for page in plan.pages() {
        ensure!(
            physical_pages.insert(page.physical_page()) && page.bytes.len() == FONT_PAGE_SIZE,
            "catalog consumer page is duplicated or not 4 KiB"
        );
        let offset = static_consumer_page_file_offset(candidate, page.physical_page())?;
        let expected = baseline
            .get(offset..offset + FONT_PAGE_SIZE)
            .ok_or_else(|| anyhow::anyhow!("catalog consumer page is outside candidate"))?;
        image.write_expected("catalog consumer font page", offset, expected, &page.bytes)?;
    }
    Ok(())
}

fn verify_installed_catalog_consumer_font_pages(
    installed: &[u8],
    candidate: &Rom,
    plan: &ConsumerCatalogPlan,
) -> Result<()> {
    for page in plan.pages() {
        let offset = static_consumer_page_file_offset(candidate, page.physical_page())?;
        ensure!(
            installed.get(offset..offset + FONT_PAGE_SIZE) == Some(page.bytes.as_slice()),
            "installed catalog consumer page does not match its plan"
        );
    }
    Ok(())
}

fn static_consumer_page_file_offset(candidate: &Rom, physical_page: u8) -> Result<usize> {
    HEADER_SIZE
        .checked_add(candidate.prg().len())
        .and_then(|offset| offset.checked_add(usize::from(physical_page) * FONT_PAGE_SIZE))
        .ok_or_else(|| anyhow::anyhow!("static consumer CHR offset overflow"))
}

fn install_cross_domain_material(
    image: &mut IntegratedImage,
    candidate: &Rom,
    plan: &CrossDomainMaterialPlan,
) -> Result<()> {
    ensure!(
        plan.sections().len() == 13,
        "integrated cross-domain material must contain thirteen non-dialogue sections"
    );
    let recipes = plan.dialogue_page_recipes();
    let recipe_end = recipes
        .file_offset
        .checked_add(recipes.bytes.len())
        .ok_or_else(|| anyhow::anyhow!("dialogue page-recipe material range overflow"))?;
    let recipe_expected = candidate
        .data()
        .get(recipes.file_offset..recipe_end)
        .ok_or_else(|| anyhow::anyhow!("dialogue page-recipe material is outside candidate"))?;
    ensure!(
        recipe_expected.iter().all(|byte| *byte == 0xFF),
        "dialogue page-recipe material destination is not exact FF"
    );
    image.write_expected(
        "dialogue visible-page recipe material",
        recipes.file_offset,
        recipe_expected,
        &recipes.bytes,
    )?;
    for section in plan.sections() {
        let end = section
            .file_offset
            .checked_add(section.bytes.len())
            .ok_or_else(|| anyhow::anyhow!("{} material range overflow", section.id))?;
        let expected = candidate
            .data()
            .get(section.file_offset..end)
            .ok_or_else(|| anyhow::anyhow!("{} material is outside candidate", section.id))?;
        ensure!(
            expected.iter().all(|byte| *byte == 0xFF),
            "{} material destination is not exact FF",
            section.id
        );
        image.write_expected(
            format!("cross-domain material {}", section.id),
            section.file_offset,
            expected,
            &section.bytes,
        )?;
    }
    let runtime = plan.consumer_catalog_runtime();
    let end = runtime
        .file_offset
        .checked_add(runtime.bytes.len())
        .ok_or_else(|| anyhow::anyhow!("consumer catalog runtime material range overflow"))?;
    let expected = candidate
        .data()
        .get(runtime.file_offset..end)
        .ok_or_else(|| anyhow::anyhow!("consumer catalog runtime material is outside candidate"))?;
    ensure!(
        expected.iter().all(|byte| *byte == 0xFF),
        "consumer catalog runtime material destination is not exact FF"
    );
    image.write_expected(
        "consumer catalog runtime material",
        runtime.file_offset,
        expected,
        &runtime.bytes,
    )?;
    Ok(())
}

fn verify_installed_cross_domain_material(
    installed: &[u8],
    plan: &CrossDomainMaterialPlan,
) -> Result<()> {
    let recipes = plan.dialogue_page_recipes();
    let recipe_end = recipes
        .file_offset
        .checked_add(recipes.bytes.len())
        .ok_or_else(|| anyhow::anyhow!("installed dialogue page-recipe range overflow"))?;
    ensure!(
        installed.get(recipes.file_offset..recipe_end) == Some(recipes.bytes.as_slice()),
        "installed dialogue page-recipe material does not match its plan"
    );
    for section in plan.sections() {
        let end = section
            .file_offset
            .checked_add(section.bytes.len())
            .ok_or_else(|| anyhow::anyhow!("{} installed material range overflow", section.id))?;
        ensure!(
            installed.get(section.file_offset..end) == Some(section.bytes.as_slice()),
            "installed {} material does not match its plan",
            section.id
        );
    }
    let runtime = plan.consumer_catalog_runtime();
    let end = runtime
        .file_offset
        .checked_add(runtime.bytes.len())
        .ok_or_else(|| anyhow::anyhow!("installed consumer catalog runtime range overflow"))?;
    ensure!(
        installed.get(runtime.file_offset..end) == Some(runtime.bytes.as_slice()),
        "installed consumer catalog runtime material does not match its plan"
    );
    Ok(())
}

fn install_encoded_chapter_titles(
    image: &mut IntegratedImage,
    candidate: &Rom,
    titles: &[EncodedChapterTitle],
) -> Result<()> {
    ensure!(
        titles.len() == 25,
        "integrated chapter-title write set must contain all twenty-five titles"
    );
    for title in titles {
        let end = title
            .file_offset
            .checked_add(title.encoded_storage.len())
            .ok_or_else(|| anyhow::anyhow!("{} storage range overflow", title.id))?;
        let expected = candidate
            .data()
            .get(title.file_offset..end)
            .ok_or_else(|| anyhow::anyhow!("{} storage is outside candidate", title.id))?;
        image.write_expected(
            format!("chapter title storage {}", title.id),
            title.file_offset,
            expected,
            &title.encoded_storage,
        )?;
        let active_mirror_offset = active_fixed_mirror_file_offset(candidate, title.file_offset)?;
        let active_mirror_end = active_mirror_offset
            .checked_add(title.encoded_storage.len())
            .ok_or_else(|| anyhow::anyhow!("{} active mirror range overflow", title.id))?;
        let active_mirror_expected = candidate
            .data()
            .get(active_mirror_offset..active_mirror_end)
            .ok_or_else(|| anyhow::anyhow!("{} active mirror is outside candidate", title.id))?;
        image.write_expected(
            format!("active fixed-bank chapter title mirror {}", title.id),
            active_mirror_offset,
            active_mirror_expected,
            &title.encoded_storage,
        )?;
    }
    Ok(())
}

fn verify_installed_chapter_titles(
    installed: &[u8],
    candidate: &Rom,
    titles: &[EncodedChapterTitle],
) -> Result<()> {
    for title in titles {
        let end = title
            .file_offset
            .checked_add(title.encoded_storage.len())
            .ok_or_else(|| anyhow::anyhow!("{} installed range overflow", title.id))?;
        ensure!(
            installed.get(title.file_offset..end) == Some(title.encoded_storage.as_slice()),
            "installed {} does not match its resident codebook encoding",
            title.id
        );
        let active_mirror_offset = active_fixed_mirror_file_offset(candidate, title.file_offset)?;
        let active_mirror_end = active_mirror_offset
            .checked_add(title.encoded_storage.len())
            .ok_or_else(|| {
                anyhow::anyhow!("{} installed active mirror range overflow", title.id)
            })?;
        ensure!(
            installed.get(active_mirror_offset..active_mirror_end)
                == Some(title.encoded_storage.as_slice()),
            "installed active fixed-bank mirror {} does not match its resident codebook encoding",
            title.id
        );
    }
    Ok(())
}

fn active_fixed_mirror_file_offset(candidate: &Rom, source_file_offset: usize) -> Result<usize> {
    let source_fixed_start = HEADER_SIZE + PRG_SIZE - FIXED_BANK_SIZE;
    let source_fixed_end = HEADER_SIZE + PRG_SIZE;
    ensure!(
        (source_fixed_start..source_fixed_end).contains(&source_file_offset),
        "chapter-title storage is outside the supported source fixed bank"
    );
    ensure!(
        candidate.prg().len() > PRG_SIZE,
        "integrated chapter-title installation requires an expanded active fixed bank"
    );
    let active_fixed_start = HEADER_SIZE
        + candidate
            .prg()
            .len()
            .checked_sub(FIXED_BANK_SIZE)
            .ok_or_else(|| anyhow::anyhow!("candidate PRG is smaller than one fixed bank"))?;
    Ok(active_fixed_start + source_file_offset - source_fixed_start)
}

/// 최종 바이트를 다시 읽어 현재 인코딩 결과가 실제 설치됐는지 확인한다.
///
/// 계획 개수나 `TrackedImage` 등록만 확인하면 런타임 재료는 새 코드북인데 본문은 이전
/// 단계 코드북인 산출물도 만들 수 있다. 최종 산출물의 소유 구간과 포인터 바이트가
/// 현재 번들의 결과와 하나라도 다르면 빌드를 실패시킨다.
fn verify_installed_dialogue(installed: &[u8], encoded: &EncodedMainDialogueBundle) -> Result<()> {
    for (region_index, region) in encoded.regions.iter().enumerate() {
        let end = region
            .file_offset
            .checked_add(region.encoded_storage.len())
            .ok_or_else(|| anyhow::anyhow!("installed dialogue region range overflow"))?;
        ensure!(
            installed.get(region.file_offset..end) == Some(region.encoded_storage.as_slice()),
            "installed dialogue region {region_index} does not match the current encoding"
        );
    }
    for pointer in &encoded.pointer_writes {
        ensure!(
            installed.get(pointer.file_offset..pointer.file_offset + 2)
                == Some(pointer.planned_pointer.to_le_bytes().as_slice()),
            "installed dialogue pointer {} does not match the current encoding",
            pointer.record_id
        );
    }
    Ok(())
}

/// 현재 단계 후보에 정규 대사 저장소와 포인터를 함께 설치한다.
///
/// 후보 전체의 SHA-1은 호출자가 이미 빌드 보고서와 결속했다. 여기서는 그 정확한
/// 후보 바이트를 Expected Write의 선행조건으로 삼고, 현재 코드북으로 다시 만든
/// 저장소와 포인터를 한 이미지에 등록한다. 런타임 재료만 새로 쓰고 이전 단계의
/// 코드북 바이트를 남겨 두는 산출물은 이 경계를 통과할 수 없다.
fn install_encoded_dialogue(
    image: &mut IntegratedImage,
    candidate: &Rom,
    encoded: &EncodedMainDialogueBundle,
) -> Result<()> {
    for (region_index, region) in encoded.regions.iter().enumerate() {
        ensure!(
            region.encoded_storage.len() == region.source_storage.len(),
            "encoded dialogue region {region_index} changed its owned extent"
        );
        let end = region
            .file_offset
            .checked_add(region.encoded_storage.len())
            .ok_or_else(|| anyhow::anyhow!("encoded dialogue region range overflow"))?;
        let expected = candidate
            .data()
            .get(region.file_offset..end)
            .ok_or_else(|| anyhow::anyhow!("encoded dialogue region is outside candidate"))?;
        image.write_expected(
            format!("main dialogue storage region {region_index}"),
            region.file_offset,
            expected,
            &region.encoded_storage,
        )?;
    }

    for pointer in &encoded.pointer_writes {
        let expected = candidate
            .data()
            .get(pointer.file_offset..pointer.file_offset + 2)
            .ok_or_else(|| anyhow::anyhow!("main dialogue pointer is outside candidate"))?;
        image.write_expected(
            format!("main dialogue pointer {}", pointer.record_id),
            pointer.file_offset,
            expected,
            &pointer.planned_pointer.to_le_bytes(),
        )?;
    }
    Ok(())
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

fn install_fixed_ui_projection(
    image: &mut IntegratedImage,
    plan: &FixedUiProjectionPlan,
) -> Result<()> {
    ensure!(
        plan.write_count() == 80,
        "fixed UI projection must install thirty-six slots, thirty-six pointers, six map-menu labels, and two map funds-summary labels"
    );
    for write in plan.writes() {
        image.write_expected(
            &write.role,
            write.file_offset,
            &write.expected,
            &write.replacement,
        )?;
    }
    Ok(())
}

fn verify_installed_fixed_ui_projection(
    installed: &[u8],
    plan: &FixedUiProjectionPlan,
) -> Result<()> {
    for write in plan.writes() {
        ensure!(
            installed.get(write.file_offset..write.file_offset + write.replacement.len())
                == Some(write.replacement.as_slice()),
            "installed fixed UI projection does not match {}",
            write.role
        );
    }
    Ok(())
}

fn install_chapter_save_projection(
    image: &mut IntegratedImage,
    plan: &ChapterSaveProjectionPlan,
) -> Result<()> {
    ensure!(
        plan.write_count() == 3,
        "chapter-save projection must install the save question and both choices"
    );
    for write in plan.writes() {
        image.write_expected(
            &write.role,
            write.file_offset,
            &write.expected,
            &write.replacement,
        )?;
    }
    Ok(())
}

fn verify_installed_chapter_save_projection(
    installed: &[u8],
    plan: &ChapterSaveProjectionPlan,
) -> Result<()> {
    for write in plan.writes() {
        ensure!(
            installed.get(write.file_offset..write.file_offset + write.replacement.len())
                == Some(write.replacement.as_slice()),
            "installed chapter-save projection does not match {}",
            write.role
        );
    }
    Ok(())
}

fn install_ending_record_projection(
    image: &mut IntegratedImage,
    plan: &EndingRecordProjectionPlan,
) -> Result<()> {
    ensure!(
        plan.write_count() == 51,
        "ending-record projection must install twenty-five title spans, twenty-five turn suffixes, and one aggregate label"
    );
    for write in plan.writes() {
        image.write_expected(
            &write.role,
            write.file_offset,
            &write.expected,
            &write.replacement,
        )?;
    }
    Ok(())
}

fn verify_installed_ending_record_projection(
    installed: &[u8],
    plan: &EndingRecordProjectionPlan,
) -> Result<()> {
    for write in plan.writes() {
        ensure!(
            installed.get(write.file_offset..write.file_offset + write.replacement.len())
                == Some(write.replacement.as_slice()),
            "installed ending-record projection does not match {}",
            write.role
        );
    }
    Ok(())
}

fn domain_contributions(
    required_domains: &[&'static str],
    expected_dialogue_write_count: usize,
    expected_chapter_title_write_count: usize,
    cross_domain_material: &CrossDomainMaterialPlan,
    fixed_ui_projection: &FixedUiProjectionPlan,
    chapter_save_projection: &ChapterSaveProjectionPlan,
    ending_record_projection: &EndingRecordProjectionPlan,
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

fn install_dialogue_runtime_material(
    image: &mut IntegratedImage,
    candidate: &Rom,
    material: &[u8],
    runtime_code: &DialogueRuntimeCodePlan,
) -> Result<()> {
    let material_offset = main_dialogue_runtime_material_file_offset()?;
    let code_page_offset = verify_runtime_material_code_projection(material, runtime_code)?;
    let whole_expected = mutation_expected_slice(
        candidate.data(),
        material_offset,
        material.len(),
        "main dialogue runtime material",
    )?;
    ensure!(
        whole_expected.iter().all(|byte| *byte == 0xFF),
        "dialogue runtime material destination is not exact FF"
    );
    image.write_expected(
        RUNTIME_MATERIAL_DATA_ROLE,
        material_offset,
        &whole_expected[..code_page_offset],
        &material[..code_page_offset],
    )?;
    for routine in &runtime_code.code_routines {
        let offset = runtime_material_routine_file_offset(
            material_offset,
            code_page_offset,
            routine.address,
            routine.bytes.len(),
        )?;
        let expected =
            mutation_expected_slice(candidate.data(), offset, routine.bytes.len(), routine.role)?;
        image.write_runtime_routine(
            routine.role,
            offset,
            expected,
            &routine.bytes,
            routine.address,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_assets::{EncodedMainDialogueRegion, MainDialoguePointerWrite};

    fn synthetic_rom() -> Rom {
        let mut bytes = vec![0; crate::rom::HEADER_SIZE + 16 * 1024];
        bytes[..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 1;
        Rom::parse(bytes).unwrap()
    }

    #[test]
    fn installs_encoded_storage_and_pointers_in_one_tracked_image() {
        let candidate = synthetic_rom();
        let encoded = EncodedMainDialogueBundle {
            regions: vec![EncodedMainDialogueRegion {
                file_offset: 0x20,
                source_storage: vec![0, 0, 0],
                encoded_storage: vec![0x40, 0x41, 0xEF],
                used_storage_byte_count: 3,
            }],
            pointer_writes: vec![MainDialoguePointerWrite {
                record_id: "record".to_owned(),
                file_offset: 0x30,
                source_pointer: 0x8000,
                planned_pointer: 0x8123,
            }],
        };
        let mut image = IntegratedImage::new(candidate.data().to_vec(), None);

        install_encoded_dialogue(&mut image, &candidate, &encoded).unwrap();

        assert_eq!(image.writes().len(), 2);
        let output = image.into_data();
        verify_installed_dialogue(&output, &encoded).unwrap();
        assert_eq!(&output[0x20..0x23], [0x40, 0x41, 0xEF]);
        assert_eq!(&output[0x30..0x32], 0x8123_u16.to_le_bytes());
    }

    #[test]
    fn installs_all_chapter_titles_and_verifies_their_final_bytes() {
        let candidate = synthetic_expanded_rom();
        let source_fixed_start = crate::rom::HEADER_SIZE + PRG_SIZE - FIXED_BANK_SIZE;
        let titles = (0..25)
            .map(|index| EncodedChapterTitle {
                id: format!("chapter-title:{:03}", index + 1),
                file_offset: source_fixed_start + 0x100 + index * 2,
                encoded_storage: vec![index as u8 + 1, 0xED],
            })
            .collect::<Vec<_>>();
        let mut image = IntegratedImage::new(candidate.data().to_vec(), None);

        install_encoded_chapter_titles(&mut image, &candidate, &titles).unwrap();

        assert_eq!(image.writes().len(), 50);
        let output = image.into_data();
        verify_installed_chapter_titles(&output, &candidate, &titles).unwrap();
        assert_eq!(
            &output[source_fixed_start + 0x100..source_fixed_start + 0x104],
            [1, 0xED, 2, 0xED]
        );
        let active_fixed_start = crate::rom::HEADER_SIZE + 512 * 1024 - FIXED_BANK_SIZE;
        assert_eq!(
            &output[active_fixed_start + 0x100..active_fixed_start + 0x104],
            [1, 0xED, 2, 0xED]
        );
    }

    fn synthetic_expanded_rom() -> Rom {
        let mut bytes = vec![0; crate::rom::HEADER_SIZE + 512 * 1024];
        bytes[..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 32;
        Rom::parse(bytes).unwrap()
    }
}
