use anyhow::{Result, ensure};

use crate::rom::Rom;

use super::{
    super::cross_domain_material::CrossDomainMaterialPlan, technical_installation::IntegratedImage,
};

pub(super) fn install_cross_domain_material(
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

pub(super) fn verify_installed_cross_domain_material(
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
