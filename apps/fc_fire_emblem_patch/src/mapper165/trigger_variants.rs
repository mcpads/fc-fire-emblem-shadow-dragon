use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

use super::trigger_planes::{PatternWindow, observed_variant_pairs};

const CHR_PAGE_SIZE: usize = 0x1000;
const FD_TILE_HIGH_PLANE_OFFSET: usize = 0x0FD8;
const RESERVED_PREFIX_PAGE_COUNT: usize = 2;
const VARIANT_PHYSICAL_PAGES: [u8; RESERVED_PREFIX_PAGE_COUNT] = [1, 0];

#[derive(Debug, Clone)]
pub(super) struct InstalledTriggerVariant {
    pub(super) physical_page: u8,
    pub(super) mapper_register_value: u8,
    pub(super) fd_source_page: u8,
    pub(super) required_high_plane: [u8; 8],
    pub(super) compatible_fe_source_pages: Vec<u8>,
    pub(super) pattern_windows: Vec<PatternWindow>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PairSelectorEntry {
    pub(super) pattern_window: PatternWindow,
    pub(super) fd_source_page: u8,
    pub(super) fe_source_page: u8,
    pub(super) mapper_register_value: u8,
}

#[derive(Debug)]
pub(super) struct TriggerVariantPlan {
    pub(super) installed_variants: Vec<InstalledTriggerVariant>,
    pub(super) selector_entries: Vec<PairSelectorEntry>,
}

pub(super) fn install_observed_trigger_variants(
    source_chr: &[u8],
    reserved_prefix: &mut [u8],
) -> Result<TriggerVariantPlan> {
    ensure!(
        reserved_prefix.len() == RESERVED_PREFIX_PAGE_COUNT * CHR_PAGE_SIZE,
        "mapper 165 reserved CHR prefix must be exactly 8 KiB"
    );
    ensure!(
        reserved_prefix.iter().all(|byte| *byte == 0),
        "mapper 165 reserved CHR prefix is not blank before variant installation"
    );

    let observed_pairs = observed_variant_pairs(source_chr)?;
    let mut grouped = BTreeMap::<(u8, [u8; 8]), (BTreeSet<u8>, BTreeSet<PatternWindow>)>::new();
    for pair in &observed_pairs {
        let (fe_pages, windows) = grouped
            .entry((pair.fd_source_page, pair.required_high_plane))
            .or_default();
        fe_pages.insert(pair.fe_source_page);
        windows.insert(pair.pattern_window);
    }
    ensure!(
        grouped.len() <= VARIANT_PHYSICAL_PAGES.len(),
        "observed trigger pairs require {} variants but the reserved prefix holds {}",
        grouped.len(),
        VARIANT_PHYSICAL_PAGES.len()
    );

    let mut assignment_by_requirement = BTreeMap::<(u8, [u8; 8]), u8>::new();
    let mut installed_variants = Vec::with_capacity(grouped.len());
    for (((fd_source_page, required_high_plane), (fe_pages, windows)), physical_page) in
        grouped.into_iter().zip(VARIANT_PHYSICAL_PAGES)
    {
        let mapper_register_value = mapper_register_for_physical_page(physical_page)?;
        let source_start = fd_source_page as usize * CHR_PAGE_SIZE;
        let source_end = source_start + CHR_PAGE_SIZE;
        ensure!(
            source_end <= source_chr.len(),
            "FD source page {fd_source_page:02X} is outside CHR"
        );
        let destination_start = physical_page as usize * CHR_PAGE_SIZE;
        let destination_end = destination_start + CHR_PAGE_SIZE;
        reserved_prefix[destination_start..destination_end]
            .copy_from_slice(&source_chr[source_start..source_end]);
        let plane_start = destination_start + FD_TILE_HIGH_PLANE_OFFSET;
        reserved_prefix[plane_start..plane_start + 8].copy_from_slice(&required_high_plane);

        assignment_by_requirement
            .insert((fd_source_page, required_high_plane), mapper_register_value);
        installed_variants.push(InstalledTriggerVariant {
            physical_page,
            mapper_register_value,
            fd_source_page,
            required_high_plane,
            compatible_fe_source_pages: fe_pages.into_iter().collect(),
            pattern_windows: windows.into_iter().collect(),
        });
    }

    let selector_entries = observed_pairs
        .into_iter()
        .map(|pair| {
            let mapper_register_value = assignment_by_requirement
                .get(&(pair.fd_source_page, pair.required_high_plane))
                .copied()
                .expect("every observed variant pair has an installed requirement");
            PairSelectorEntry {
                pattern_window: pair.pattern_window,
                fd_source_page: pair.fd_source_page,
                fe_source_page: pair.fe_source_page,
                mapper_register_value,
            }
        })
        .collect();

    Ok(TriggerVariantPlan {
        installed_variants,
        selector_entries,
    })
}

pub(super) fn verify_installed_trigger_variants(
    source_chr: &[u8],
    output_prefix: &[u8],
    plan: &TriggerVariantPlan,
) -> Result<()> {
    ensure!(
        output_prefix.len() == RESERVED_PREFIX_PAGE_COUNT * CHR_PAGE_SIZE,
        "mapper 165 output prefix is not 8 KiB"
    );
    for physical_page in 0..RESERVED_PREFIX_PAGE_COUNT as u8 {
        let start = physical_page as usize * CHR_PAGE_SIZE;
        let end = start + CHR_PAGE_SIZE;
        if let Some(variant) = plan
            .installed_variants
            .iter()
            .find(|variant| variant.physical_page == physical_page)
        {
            let source_start = variant.fd_source_page as usize * CHR_PAGE_SIZE;
            let source_end = source_start + CHR_PAGE_SIZE;
            let mut expected = source_chr[source_start..source_end].to_vec();
            expected[FD_TILE_HIGH_PLANE_OFFSET..FD_TILE_HIGH_PLANE_OFFSET + 8]
                .copy_from_slice(&variant.required_high_plane);
            ensure!(
                output_prefix[start..end] == expected,
                "mapper 165 trigger variant page {physical_page:02X} changed"
            );
        } else {
            ensure!(
                output_prefix[start..end].iter().all(|byte| *byte == 0),
                "unused mapper 165 prefix page {physical_page:02X} is not blank"
            );
        }
    }
    Ok(())
}

fn mapper_register_for_physical_page(physical_page: u8) -> Result<u8> {
    if physical_page == 0 {
        return Ok(1);
    }
    physical_page
        .checked_mul(4)
        .ok_or_else(|| anyhow::anyhow!("physical CHR page cannot be encoded for mapper 165"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_chr() -> Vec<u8> {
        let mut chr = vec![0; 32 * CHR_PAGE_SIZE];
        for (index, byte) in chr.iter_mut().enumerate() {
            *byte = (index / CHR_PAGE_SIZE) as u8;
        }
        for page in [0x00, 0x18, 0x19] {
            let start = page * CHR_PAGE_SIZE + FD_TILE_HIGH_PLANE_OFFSET;
            chr[start..start + 8].fill(0x20);
        }
        chr[0x14 * CHR_PAGE_SIZE + FD_TILE_HIGH_PLANE_OFFSET
            ..0x14 * CHR_PAGE_SIZE + FD_TILE_HIGH_PLANE_OFFSET + 8]
            .fill(0);
        chr
    }

    #[test]
    fn observed_variant_copies_the_fd_page_and_replaces_only_the_trigger_plane() {
        let source = source_chr();
        let mut prefix = vec![0; RESERVED_PREFIX_PAGE_COUNT * CHR_PAGE_SIZE];
        let plan = install_observed_trigger_variants(&source, &mut prefix).unwrap();

        assert_eq!(plan.installed_variants.len(), 1);
        let variant = &plan.installed_variants[0];
        assert_eq!(variant.physical_page, 1);
        assert_eq!(variant.mapper_register_value, 4);
        assert_eq!(variant.fd_source_page, 0);
        assert_eq!(variant.compatible_fe_source_pages, vec![0x14]);
        assert_eq!(variant.pattern_windows, vec![PatternWindow::Right]);
        assert_eq!(plan.selector_entries.len(), 1);

        let start = CHR_PAGE_SIZE;
        let mut expected = source[..CHR_PAGE_SIZE].to_vec();
        expected[FD_TILE_HIGH_PLANE_OFFSET..FD_TILE_HIGH_PLANE_OFFSET + 8].fill(0);
        assert_eq!(prefix[start..start + CHR_PAGE_SIZE], expected);
        assert!(prefix[..CHR_PAGE_SIZE].iter().all(|byte| *byte == 0));
        verify_installed_trigger_variants(&source, &prefix, &plan).unwrap();
    }

    #[test]
    fn physical_page_zero_uses_a_nonzero_register_value_instead_of_chr_ram() {
        assert_eq!(mapper_register_for_physical_page(0).unwrap(), 1);
        assert_eq!(mapper_register_for_physical_page(1).unwrap(), 4);
    }
}
