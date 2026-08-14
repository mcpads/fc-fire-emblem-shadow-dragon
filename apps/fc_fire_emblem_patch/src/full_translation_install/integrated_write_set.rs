use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::EncodedMainDialogueBundle,
    font_slots::FONT_PAGE_SIZE,
    rom::{HEADER_SIZE, Rom},
    tracked::TrackedImage,
};

use super::{
    chapter_intro_residency::EncodedChapterTitle,
    cold_request_presentation::ColdRequestPresentationPage,
    consumer_catalog::ConsumerCatalogPlan,
    consumer_codebook::ConsumerCodebookPlan,
    consumer_installation::ConsumerInstallationPlan,
    cross_domain_material::CrossDomainMaterialPlan,
    installation_layout::main_dialogue_runtime_material_file_offset,
    runtime_code::{DialogueRuntimeCodePlan, DialogueRuntimeHookRole, DialogueRuntimeHookSite},
};
use crate::dialogue_inventory::switchable_cpu_to_file_offset;

const FIXED_BANK_SIZE: usize = 16 * 1024;

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
    pub(super) consumer_installation: &'a ConsumerInstallationPlan,
    pub(super) required_domains: &'a [&'static str],
}

#[derive(Serialize)]
pub(super) struct IntegratedWriteSetPlan {
    required_domain_count: usize,
    domains: Vec<DomainWriteContribution>,
    contributing_domain_count: usize,
    fully_planned_domain_count: usize,
    expected_write_count: usize,
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
    installed_cold_request_presentation_matches_plan: bool,
    installed_static_consumer_font_pages_match_plan: bool,
    installed_catalog_consumer_font_pages_match_plan: bool,
    installed_cross_domain_material_matches_plan: bool,
    changed_byte_count: usize,
    installed_dialogue_matches_current_encoding: bool,
    installed_chapter_titles_match_resident_encoding: bool,
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
    carried_consumer_writes_bound_to_exact_candidate: bool,
    new_global_consumer_writes_contributed: bool,
    all_consumer_writes_contributed: bool,
    expected_write_count: usize,
    complete_in_integrated_plan: bool,
}

pub(super) fn plan_integrated_write_set(
    inputs: IntegratedWriteSetInputs<'_>,
) -> Result<(Vec<u8>, IntegratedWriteSetPlan)> {
    let (expanded_base, appended_chr_page_count) = expand_candidate_for_consumer_pages(&inputs)?;
    let mut image = TrackedImage::new(expanded_base.clone());
    let chr_expansion_header_write_count = usize::from(appended_chr_page_count != 0);
    if appended_chr_page_count != 0 {
        let expanded_chr_bank_count = u8::try_from(
            (inputs.candidate.chr().len() / FONT_PAGE_SIZE + appended_chr_page_count) / 2,
        )
        .map_err(|_| anyhow::anyhow!("expanded CHR bank count exceeds iNES byte 5"))?;
        image.write_expected(
            "expand integrated candidate CHR",
            5,
            &[inputs.candidate.data()[5]],
            &[expanded_chr_bank_count],
        )?;
    }
    let dialogue_storage_write_count =
        inputs.encoded_dialogue.regions.len() + inputs.encoded_dialogue.pointer_writes.len();
    install_encoded_dialogue(&mut image, inputs.candidate, inputs.encoded_dialogue)?;
    install_encoded_chapter_titles(&mut image, inputs.candidate, inputs.encoded_chapter_titles)?;
    ensure!(
        image.writes().len()
            == chr_expansion_header_write_count
                + dialogue_storage_write_count
                + inputs.encoded_chapter_titles.len(),
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
        image.write_expected(routine.role, offset, existing, &routine.bytes)?;
    }

    // 훅 역할과 원본 자리와 쓸 바이트는 코드 계획이 한 단위로 제공한다. 설치자가
    // 별도 배열로 다시 세면 새 훅을 추가할 때 보고서와 실제 쓰기가 갈라진다.
    let mut hook_roles = std::collections::BTreeSet::new();
    for hook in &inputs.dialogue_runtime_code.hooks {
        ensure!(
            hook_roles.insert(hook.role),
            "dialogue runtime hook role {:?} is emitted more than once",
            hook.role
        );
        let offset = match hook.site {
            DialogueRuntimeHookSite::Fixed(address) => {
                fixed_file_offset(inputs.candidate, address)?
            }
            DialogueRuntimeHookSite::Switchable { bank, address } => {
                switchable_cpu_to_file_offset(bank, address)?
            }
        };
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
        image.write_expected(hook.write_role, offset, existing, &hook.bytes)?;
    }

    let expected_write_count_before_cross_domain = image.writes().len();
    install_cross_domain_material(&mut image, inputs.candidate, inputs.cross_domain_material)?;

    image.verify_all_changes_tracked(&expanded_base)?;
    let expected_write_count = image.writes().len();
    let output = image.into_data();
    verify_installed_dialogue(&output, inputs.encoded_dialogue)?;
    verify_installed_chapter_titles(&output, inputs.encoded_chapter_titles)?;
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
    let installed_image = output.clone();
    let changed_byte_count = expanded_base
        .iter()
        .zip(&output)
        .filter(|(before, after)| before != after)
        .count();

    let domains = domain_contributions(
        inputs.required_domains,
        expected_write_count_before_cross_domain - inputs.encoded_chapter_titles.len(),
        inputs.encoded_chapter_titles.len(),
        inputs.cross_domain_material,
        inputs.consumer_installation,
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
        contributing_domain_count == inputs.required_domains.len()
            && fully_planned_domain_count
                == inputs.consumer_installation.fully_planned_domain_count(),
        "integrated write gate advanced without every domain layer"
    );

    Ok((
        installed_image,
        IntegratedWriteSetPlan {
            required_domain_count: inputs.required_domains.len(),
            domains,
            contributing_domain_count,
            fully_planned_domain_count,
            expected_write_count,
            dialogue_runtime_hook_count: hook_roles.len(),
            dialogue_runtime_hook_roles: hook_roles.into_iter().collect(),
            dialogue_runtime_fixed_routine_count: inputs.dialogue_runtime_code.fixed_routines.len()
                + inputs.dialogue_runtime_code.reclaimed_fixed_routines.len(),
            dialogue_runtime_code_routine_count: inputs.dialogue_runtime_code.code_routines.len(),
            dialogue_storage_region_count: inputs.encoded_dialogue.regions.len(),
            dialogue_pointer_write_count: inputs.encoded_dialogue.pointer_writes.len(),
            chapter_title_storage_write_count: inputs.encoded_chapter_titles.len(),
            cold_request_presentation_write_count: 1,
            chr_expansion_header_write_count,
            appended_chr_page_count,
            static_consumer_font_page_write_count: inputs.consumer_codebook.pages().len(),
            catalog_consumer_font_page_write_count: inputs.consumer_catalog.pages().len(),
            cross_domain_material_write_count: inputs.cross_domain_material.sections().len() + 1,
            installed_cold_request_presentation_matches_plan: true,
            installed_static_consumer_font_pages_match_plan: true,
            installed_catalog_consumer_font_pages_match_plan: true,
            installed_cross_domain_material_matches_plan: true,
            changed_byte_count,
            installed_dialogue_matches_current_encoding: true,
            installed_chapter_titles_match_resident_encoding: true,
            every_change_tracked: true,
            one_shared_image: true,
            all_domains_contribute_expected_writes: true,
            output_materialized_in_memory_only: true,
            rom_emitted: false,
        },
    ))
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
    image: &mut TrackedImage,
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
    image: &mut TrackedImage,
    candidate: &Rom,
    baseline: &[u8],
    plan: &ConsumerCodebookPlan,
) -> Result<()> {
    ensure!(
        plan.pages().len() == 3,
        "integrated consumer codebook must install the three fixed-content pages"
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
    image: &mut TrackedImage,
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

fn expand_candidate_for_consumer_pages(
    inputs: &IntegratedWriteSetInputs<'_>,
) -> Result<(Vec<u8>, usize)> {
    ensure!(
        inputs.candidate.chr().len().is_multiple_of(FONT_PAGE_SIZE),
        "integrated candidate CHR is not a whole number of 4 KiB pages"
    );
    let highest_required_page = std::iter::once(inputs.cold_request_presentation.physical_page)
        .chain(
            inputs
                .consumer_codebook
                .pages()
                .iter()
                .map(|page| page.physical_page()),
        )
        .chain(
            inputs
                .consumer_catalog
                .pages()
                .iter()
                .map(|page| page.physical_page()),
        )
        .max()
        .ok_or_else(|| anyhow::anyhow!("integrated consumer page set is empty"))?;
    let required_page_count = usize::from(highest_required_page) + 1;
    let required_bank_aligned_page_count = required_page_count.div_ceil(2) * 2;
    ensure!(
        required_bank_aligned_page_count <= 64,
        "integrated consumer pages exceed mapper 165 CHR capacity"
    );
    let current_page_count = inputs.candidate.chr().len() / FONT_PAGE_SIZE;
    ensure!(
        current_page_count <= required_bank_aligned_page_count,
        "integrated page plan would shrink the current candidate CHR"
    );
    let appended_page_count = required_bank_aligned_page_count - current_page_count;
    let mut expanded = inputs.candidate.data().to_vec();
    expanded.resize(
        expanded
            .len()
            .checked_add(appended_page_count * FONT_PAGE_SIZE)
            .ok_or_else(|| anyhow::anyhow!("integrated CHR expansion overflow"))?,
        0xFF,
    );
    Ok((expanded, appended_page_count))
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
    image: &mut TrackedImage,
    candidate: &Rom,
    plan: &CrossDomainMaterialPlan,
) -> Result<()> {
    ensure!(
        plan.sections().len() == 12,
        "integrated cross-domain material must contain twelve non-dialogue sections"
    );
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
    image: &mut TrackedImage,
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
    }
    Ok(())
}

fn verify_installed_chapter_titles(installed: &[u8], titles: &[EncodedChapterTitle]) -> Result<()> {
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
    }
    Ok(())
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
    image: &mut TrackedImage,
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

fn domain_contributions(
    required_domains: &[&'static str],
    expected_dialogue_write_count: usize,
    expected_chapter_title_write_count: usize,
    cross_domain_material: &CrossDomainMaterialPlan,
    consumer_installation: &ConsumerInstallationPlan,
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
            let all_consumers_statically_accounted =
                consumer_installation.domain_all_consumers_statically_accounted(id);
            DomainWriteContribution {
                id,
                translation_input_loaded: true,
                glyph_lifetime_bound: true,
                storage_and_address_writes_contributed: dialogue
                    || chapter_titles
                    || all_consumers_statically_accounted,
                runtime_material_writes_contributed: dialogue || material,
                font_supply_writes_contributed: true,
                carried_consumer_writes_bound_to_exact_candidate: consumer_installation
                    .domain_has_carried_consumers(id),
                new_global_consumer_writes_contributed: consumer_installation
                    .domain_has_newly_planned_consumers(id),
                all_consumer_writes_contributed: all_consumers_statically_accounted,
                expected_write_count: usize::from(material)
                    + if dialogue {
                        expected_dialogue_write_count
                    } else if chapter_titles {
                        expected_chapter_title_write_count
                    } else {
                        0
                    },
                complete_in_integrated_plan: all_consumers_statically_accounted,
            }
        })
        .collect())
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
        let mut image = TrackedImage::new(candidate.data().to_vec());

        install_encoded_dialogue(&mut image, &candidate, &encoded).unwrap();

        assert_eq!(image.writes().len(), 2);
        let output = image.into_data();
        verify_installed_dialogue(&output, &encoded).unwrap();
        assert_eq!(&output[0x20..0x23], [0x40, 0x41, 0xEF]);
        assert_eq!(&output[0x30..0x32], 0x8123_u16.to_le_bytes());
    }

    #[test]
    fn installs_all_chapter_titles_and_verifies_their_final_bytes() {
        let candidate = synthetic_rom();
        let titles = (0..25)
            .map(|index| EncodedChapterTitle {
                id: format!("chapter-title:{:03}", index + 1),
                file_offset: 0x100 + index * 2,
                encoded_storage: vec![index as u8 + 1, 0xED],
            })
            .collect::<Vec<_>>();
        let mut image = TrackedImage::new(candidate.data().to_vec());

        install_encoded_chapter_titles(&mut image, &candidate, &titles).unwrap();

        assert_eq!(image.writes().len(), 25);
        let output = image.into_data();
        verify_installed_chapter_titles(&output, &titles).unwrap();
        assert_eq!(&output[0x100..0x104], [1, 0xED, 2, 0xED]);
    }
}
