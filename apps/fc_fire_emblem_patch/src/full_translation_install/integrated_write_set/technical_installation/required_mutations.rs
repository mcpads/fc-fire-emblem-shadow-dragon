use super::*;

pub(in crate::full_translation_install::integrated_write_set) fn plan_candidate_image_growth(
    inputs: &IntegratedWriteSetInputs<'_>,
) -> Result<ImageGrowthPlan> {
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
    plan_candidate_image_growth_for_highest_page(inputs.candidate, highest_required_page)
}

pub(in crate::full_translation_install::integrated_write_set) fn plan_candidate_image_growth_for_highest_page(
    candidate: &Rom,
    highest_required_page: u8,
) -> Result<ImageGrowthPlan> {
    ensure!(
        candidate.chr().len().is_multiple_of(FONT_PAGE_SIZE),
        "integrated candidate CHR is not a whole number of 4 KiB pages"
    );
    let required_page_count = usize::from(highest_required_page) + 1;
    let required_bank_aligned_page_count = required_page_count.div_ceil(2) * 2;
    ensure!(
        required_bank_aligned_page_count <= 64,
        "integrated consumer pages exceed mapper 165 CHR capacity"
    );
    let current_page_count = candidate.chr().len() / FONT_PAGE_SIZE;
    ensure!(
        current_page_count <= required_bank_aligned_page_count,
        "integrated page plan would shrink the current candidate CHR"
    );
    ensure!(
        usize::from(candidate.data()[5]) * 2 == current_page_count,
        "integrated candidate CHR header does not match its current extent"
    );
    let appended_chr_page_count = required_bank_aligned_page_count - current_page_count;
    let appended_chr_byte_count = appended_chr_page_count
        .checked_mul(FONT_PAGE_SIZE)
        .context("integrated appended CHR byte count overflow")?;
    let final_byte_count = candidate
        .data()
        .len()
        .checked_add(appended_chr_byte_count)
        .context("integrated final image size overflow")?;
    let final_chr_bank_count = u8::try_from(required_bank_aligned_page_count / 2)
        .context("expanded CHR bank count exceeds iNES byte 5")?;
    Ok(ImageGrowthPlan {
        source_byte_count: candidate.data().len(),
        final_byte_count,
        appended_chr_page_count,
        appended_chr_byte_count,
        final_chr_bank_count,
    })
}

pub(in crate::full_translation_install::integrated_write_set) fn plan_required_mutation_identities(
    inputs: &IntegratedWriteSetInputs<'_>,
    growth: &ImageGrowthPlan,
    expanded_baseline: &[u8],
) -> Result<Vec<MutationIdentity>> {
    let mut required = growth.append_identity().into_iter().collect::<Vec<_>>();
    if growth.appended_chr_page_count != 0 {
        required.push(MutationIdentity::exact(
            CHR_HEADER_ROLE,
            5,
            &[inputs.candidate.data()[5]],
            &[growth.final_chr_bank_count],
        ));
    }

    for (region_index, region) in inputs.encoded_dialogue.regions.iter().enumerate() {
        ensure!(
            region.encoded_storage.len() == region.source_storage.len(),
            "encoded dialogue region {region_index} changed its owned extent"
        );
        let expected = mutation_expected_slice(
            expanded_baseline,
            region.file_offset,
            region.encoded_storage.len(),
            "main dialogue storage",
        )?;
        required.push(MutationIdentity::exact(
            format!("main dialogue storage region {region_index}"),
            region.file_offset,
            expected,
            &region.encoded_storage,
        ));
    }
    for pointer in &inputs.encoded_dialogue.pointer_writes {
        let expected = mutation_expected_slice(
            expanded_baseline,
            pointer.file_offset,
            2,
            "main dialogue pointer",
        )?;
        required.push(MutationIdentity::exact(
            format!("main dialogue pointer {}", pointer.record_id),
            pointer.file_offset,
            expected,
            &pointer.planned_pointer.to_le_bytes(),
        ));
    }

    ensure!(
        inputs.encoded_chapter_titles.len() == 25,
        "integrated chapter-title mutation plan must contain all twenty-five titles"
    );
    for title in inputs.encoded_chapter_titles {
        let expected = mutation_expected_slice(
            expanded_baseline,
            title.file_offset,
            title.encoded_storage.len(),
            &title.id,
        )?;
        required.push(MutationIdentity::exact(
            format!("chapter title storage {}", title.id),
            title.file_offset,
            expected,
            &title.encoded_storage,
        ));
        let mirror_offset = active_fixed_mirror_file_offset(inputs.candidate, title.file_offset)?;
        let mirror_expected = mutation_expected_slice(
            expanded_baseline,
            mirror_offset,
            title.encoded_storage.len(),
            "active chapter title mirror",
        )?;
        required.push(MutationIdentity::exact(
            format!("active fixed-bank chapter title mirror {}", title.id),
            mirror_offset,
            mirror_expected,
            &title.encoded_storage,
        ));
    }

    let cold_offset =
        cold_request_presentation_file_offset(inputs.candidate, inputs.cold_request_presentation)?;
    required.push(MutationIdentity::exact(
        "cold-request dialogue presentation CHR page",
        cold_offset,
        mutation_expected_slice(
            expanded_baseline,
            cold_offset,
            inputs.cold_request_presentation.bytes.len(),
            "cold-request presentation",
        )?,
        &inputs.cold_request_presentation.bytes,
    ));
    for page in inputs.consumer_codebook.pages() {
        let offset = static_consumer_page_file_offset(inputs.candidate, page.physical_page())?;
        required.push(MutationIdentity::exact(
            format!("static consumer font page {}", page.id),
            offset,
            mutation_expected_slice(
                expanded_baseline,
                offset,
                page.bytes.len(),
                "static consumer font page",
            )?,
            &page.bytes,
        ));
    }
    for page in inputs.consumer_catalog.pages() {
        let offset = static_consumer_page_file_offset(inputs.candidate, page.physical_page())?;
        required.push(MutationIdentity::exact(
            "catalog consumer font page",
            offset,
            mutation_expected_slice(
                expanded_baseline,
                offset,
                page.bytes.len(),
                "catalog consumer font page",
            )?,
            &page.bytes,
        ));
    }

    required.extend(plan_required_runtime_material_mutations(
        inputs.candidate,
        inputs.dialogue_runtime_material,
        inputs.dialogue_runtime_code,
    )?);
    for routine in &inputs.dialogue_runtime_code.fixed_routines {
        let offset = fixed_file_offset(inputs.candidate, routine.address)?;
        let expected = mutation_expected_slice(
            inputs.candidate.data(),
            offset,
            routine.bytes.len(),
            routine.role,
        )?;
        ensure!(
            expected.iter().all(|byte| *byte == 0xFF),
            "{} would overwrite bytes that are not reserved",
            routine.role
        );
        required.push(MutationIdentity::runtime_routine(
            routine.role,
            offset,
            expected,
            &routine.bytes,
            routine.address,
        ));
    }
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
        let expected =
            mutation_expected_slice(inputs.candidate.data(), offset, capacity, routine.role)?;
        ensure!(
            sha1_hex(expected) == reclaimed.expected_source_sha1,
            "{} source digest changed",
            routine.role
        );
        required.push(MutationIdentity::runtime_routine(
            routine.role,
            offset,
            expected,
            &routine.bytes,
            routine.address,
        ));
    }

    let mut hook_roles = BTreeSet::new();
    for hook in &inputs.dialogue_runtime_code.hooks {
        ensure!(
            hook_roles.insert(hook.role),
            "dialogue runtime hook role {:?} is planned more than once",
            hook.role
        );
        let site = runtime_hook_site_identity(&hook.site);
        let offset = runtime_hook_file_offset(inputs.candidate, site)?;
        let expected = mutation_expected_slice(
            inputs.candidate.data(),
            offset,
            hook.bytes.len(),
            hook.write_role,
        )?;
        ensure!(
            expected != hook.bytes,
            "{} is already installed; the candidate is not a clean base",
            hook.write_role
        );
        required.push(MutationIdentity::runtime_hook(
            hook.write_role,
            offset,
            expected,
            &hook.bytes,
            hook.role,
            site,
        ));
    }

    for write in inputs.fixed_ui_projection.writes() {
        required.push(MutationIdentity::exact(
            &write.role,
            write.file_offset,
            &write.expected,
            &write.replacement,
        ));
    }
    for write in inputs.chapter_save_projection.writes() {
        required.push(MutationIdentity::exact(
            &write.role,
            write.file_offset,
            &write.expected,
            &write.replacement,
        ));
    }
    for write in inputs.ending_record_projection.writes() {
        required.push(MutationIdentity::exact(
            &write.role,
            write.file_offset,
            &write.expected,
            &write.replacement,
        ));
    }
    let recipes = inputs.cross_domain_material.dialogue_page_recipes();
    let expected = mutation_expected_slice(
        inputs.candidate.data(),
        recipes.file_offset,
        recipes.bytes.len(),
        "dialogue visible-page recipe material",
    )?;
    ensure!(
        expected.iter().all(|byte| *byte == 0xFF),
        "dialogue page-recipe material destination is not exact FF"
    );
    required.push(MutationIdentity::exact(
        "dialogue visible-page recipe material",
        recipes.file_offset,
        expected,
        &recipes.bytes,
    ));
    for section in inputs.cross_domain_material.sections() {
        let expected = mutation_expected_slice(
            inputs.candidate.data(),
            section.file_offset,
            section.bytes.len(),
            section.id,
        )?;
        ensure!(
            expected.iter().all(|byte| *byte == 0xFF),
            "{} material destination is not exact FF",
            section.id
        );
        required.push(MutationIdentity::exact(
            format!("cross-domain material {}", section.id),
            section.file_offset,
            expected,
            &section.bytes,
        ));
    }
    let runtime = inputs.cross_domain_material.consumer_catalog_runtime();
    let expected = mutation_expected_slice(
        inputs.candidate.data(),
        runtime.file_offset,
        runtime.bytes.len(),
        "consumer catalog runtime material",
    )?;
    ensure!(
        expected.iter().all(|byte| *byte == 0xFF),
        "consumer catalog runtime material destination is not exact FF"
    );
    required.push(MutationIdentity::exact(
        "consumer catalog runtime material",
        runtime.file_offset,
        expected,
        &runtime.bytes,
    ));

    let mut ordered = required
        .iter()
        .filter(|identity| !identity.is_growth())
        .collect::<Vec<_>>();
    ordered.sort_by_key(|identity| identity.offset);
    for adjacent in ordered.windows(2) {
        let earlier = adjacent[0];
        let later = adjacent[1];
        let earlier_end = earlier
            .offset
            .checked_add(earlier.expected.len())
            .with_context(|| format!("{} mutation range overflow", earlier.role))?;
        ensure!(
            earlier_end <= later.offset,
            "required mutation {} [{:#X}..{:#X}) overlaps {} [{:#X}..{:#X})",
            earlier.role,
            earlier.offset,
            earlier_end,
            later.role,
            later.offset,
            later.offset + later.expected.len(),
        );
    }
    ensure!(
        unique_mutation_identity_set(&required).is_some(),
        "required mutation identity is duplicated"
    );
    ensure!(
        materialize_mutation_plan(inputs.candidate.data(), &required).is_some(),
        "required mutation identities escape the image or do not bind the immutable candidate"
    );
    Ok(required)
}

pub(in crate::full_translation_install::integrated_write_set) fn mutation_expected_slice<'a>(
    source: &'a [u8],
    offset: usize,
    len: usize,
    role: &str,
) -> Result<&'a [u8]> {
    let end = offset
        .checked_add(len)
        .with_context(|| format!("{role} mutation range overflow"))?;
    source
        .get(offset..end)
        .with_context(|| format!("{role} mutation is outside its source image"))
}

pub(in crate::full_translation_install::integrated_write_set) fn plan_required_runtime_material_mutations(
    candidate: &Rom,
    material: &[u8],
    runtime_code: &DialogueRuntimeCodePlan,
) -> Result<Vec<MutationIdentity>> {
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
    let mut required = vec![MutationIdentity::exact(
        RUNTIME_MATERIAL_DATA_ROLE,
        material_offset,
        &whole_expected[..code_page_offset],
        &material[..code_page_offset],
    )];
    for routine in &runtime_code.code_routines {
        let offset = runtime_material_routine_file_offset(
            material_offset,
            code_page_offset,
            routine.address,
            routine.bytes.len(),
        )?;
        let expected =
            mutation_expected_slice(candidate.data(), offset, routine.bytes.len(), routine.role)?;
        required.push(MutationIdentity::runtime_routine(
            routine.role,
            offset,
            expected,
            &routine.bytes,
            routine.address,
        ));
    }
    Ok(required)
}

pub(in crate::full_translation_install::integrated_write_set) fn verify_runtime_material_code_projection(
    material: &[u8],
    runtime_code: &DialogueRuntimeCodePlan,
) -> Result<usize> {
    let code_page_offset = material
        .len()
        .checked_sub(MMC3_PAGE_BYTE_COUNT)
        .context("dialogue runtime material has no complete runtime-code page")?;
    let mut projected = vec![0xFF; MMC3_PAGE_BYTE_COUNT];
    let mut covered = vec![false; MMC3_PAGE_BYTE_COUNT];
    ensure!(
        !runtime_code.code_routines.is_empty(),
        "dialogue runtime code plan has no material-page routines"
    );
    for routine in &runtime_code.code_routines {
        let start = usize::from(
            routine
                .address
                .checked_sub(RUNTIME_CODE_WINDOW_START)
                .ok_or_else(|| {
                    anyhow::anyhow!("{} begins below the A000 code page", routine.role)
                })?,
        );
        let end = start
            .checked_add(routine.bytes.len())
            .with_context(|| format!("{} runtime routine range overflow", routine.role))?;
        let range = covered
            .get_mut(start..end)
            .with_context(|| format!("{} exceeds the runtime-code page", routine.role))?;
        ensure!(
            range.iter().all(|byte| !*byte),
            "{} overlaps another runtime material routine",
            routine.role
        );
        range.fill(true);
        projected[start..end].copy_from_slice(&routine.bytes);
    }
    ensure!(
        material.get(code_page_offset..) == Some(projected.as_slice()),
        "runtime material code page is not exactly the complete routine plan"
    );
    Ok(code_page_offset)
}

pub(in crate::full_translation_install::integrated_write_set) fn runtime_material_routine_file_offset(
    material_offset: usize,
    code_page_offset: usize,
    cpu_address: u16,
    byte_count: usize,
) -> Result<usize> {
    let within_page = usize::from(
        cpu_address
            .checked_sub(RUNTIME_CODE_WINDOW_START)
            .context("runtime material routine begins below A000")?,
    );
    ensure!(
        within_page
            .checked_add(byte_count)
            .is_some_and(|end| end <= MMC3_PAGE_BYTE_COUNT),
        "runtime material routine exceeds the A000 code page"
    );
    material_offset
        .checked_add(code_page_offset)
        .and_then(|offset| offset.checked_add(within_page))
        .context("runtime material routine file offset overflow")
}

pub(in crate::full_translation_install::integrated_write_set) fn runtime_hook_site_identity(
    site: &DialogueRuntimeHookSite,
) -> RuntimeHookSiteIdentity {
    match *site {
        DialogueRuntimeHookSite::Fixed(address) => RuntimeHookSiteIdentity::Fixed(address),
        DialogueRuntimeHookSite::Switchable { bank, address } => {
            RuntimeHookSiteIdentity::Switchable { bank, address }
        }
    }
}

pub(in crate::full_translation_install::integrated_write_set) fn runtime_hook_file_offset(
    candidate: &Rom,
    site: RuntimeHookSiteIdentity,
) -> Result<usize> {
    match site {
        RuntimeHookSiteIdentity::Fixed(address) => fixed_file_offset(candidate, address),
        RuntimeHookSiteIdentity::Switchable { bank, address } => {
            switchable_cpu_to_file_offset(bank, address)
        }
    }
}
