use anyhow::{Result, ensure};

use crate::{
    font_slots::FONT_PAGE_SIZE,
    rom::{HEADER_SIZE, Rom},
};

use super::{
    super::{
        cold_request_presentation::ColdRequestPresentationPage,
        consumer_catalog::ConsumerCatalogPlan, consumer_codebook::ConsumerCodebookPlan,
    },
    technical_installation::IntegratedImage,
};

pub(super) fn cold_request_presentation_file_offset(
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

pub(super) fn install_cold_request_presentation(
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

pub(super) fn verify_installed_cold_request_presentation(
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

pub(super) fn install_static_consumer_font_pages(
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

pub(super) fn verify_installed_static_consumer_font_pages(
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

pub(super) fn install_catalog_consumer_font_pages(
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

pub(super) fn verify_installed_catalog_consumer_font_pages(
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

pub(super) fn static_consumer_page_file_offset(
    candidate: &Rom,
    physical_page: u8,
) -> Result<usize> {
    HEADER_SIZE
        .checked_add(candidate.prg().len())
        .and_then(|offset| offset.checked_add(usize::from(physical_page) * FONT_PAGE_SIZE))
        .ok_or_else(|| anyhow::anyhow!("static consumer CHR offset overflow"))
}
