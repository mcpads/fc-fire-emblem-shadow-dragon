//! 가변 이름 화면을 전면 런타임 합성 없이 유한한 CHR 페이지 집합으로 만든다.
//!
//! 아이템명·병종명·요약/상태 라벨·아이템 동작 라벨은 모든 카탈로그 페이지에서 같은
//! 코드와 글꼴을 사용한다. 한 화면에는 유닛명 또는 적 이름 하나만 보이므로 이름별로
//! 추가 글리프를 페이지에 나눠 담고, 원천 이름 ID가 해당 페이지를 고르게 한다.

mod packing;
mod runtime_material;

pub(in crate::full_translation_install) use runtime_material::{
    ConsumerCatalogRuntimeLayout, ConsumerCatalogRuntimeMaterialInputs,
    ConsumerCatalogRuntimeMaterialPlan, plan_consumer_catalog_runtime_material,
};

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, FONT_PAGE_SIZE, active_hangul_codes},
    mapper165::{dialogue_probe_font::build_font_page_by_code, encode_chr_page_register},
    semantic_translation::SemanticTranslationPlan,
    sha1_hex,
    text_inventory::{FixedTextLogicalByte, FixedTextPlan, FixedTextPlannedEntry},
    unit_names::UnitNamePlan,
    unit_ui_text::summary_and_status_label_ids,
};

use packing::{CatalogNameDemand, pack_name_demands};

pub(super) struct ConsumerCatalogInputs<'a> {
    pub(super) source_font_page: &'a [u8],
    pub(super) first_physical_page: u8,
    pub(super) available_page_count: usize,
    pub(super) preserved_unit_ui_display_codes: &'a BTreeSet<u8>,
    pub(super) fixed: &'a FixedTextPlan,
    pub(super) unit_names: &'a UnitNamePlan,
    pub(super) unit_ui: &'a SemanticTranslationPlan,
    pub(super) item_actions: &'a SemanticTranslationPlan,
}

#[derive(Serialize)]
pub(super) struct ConsumerCatalogPlan {
    schema: u8,
    strategy: &'static str,
    base_glyph_count: usize,
    preserved_active_code_count: usize,
    per_page_name_slot_count: usize,
    playable_name_count: usize,
    enemy_name_count: usize,
    name_identity_count: usize,
    page_count: usize,
    first_physical_page: u8,
    available_page_count: usize,
    maximum_page_slot_demand: usize,
    pages: Vec<ConsumerCatalogPage>,
    identity_pages: Vec<CatalogIdentityPage>,
    every_base_glyph_has_one_stable_code: bool,
    every_name_identity_fits_one_page: bool,
    every_page_fits_active_codes: bool,
    pages_fit_reclaimable_tail: bool,
    #[serde(skip)]
    base_assignments: BTreeMap<char, u8>,
}

impl ConsumerCatalogPlan {
    pub(super) fn pages(&self) -> &[ConsumerCatalogPage] {
        &self.pages
    }

    pub(super) fn base_assignments(&self) -> &BTreeMap<char, u8> {
        &self.base_assignments
    }

    pub(super) fn mapper_registers(&self) -> Result<[u8; 2]> {
        ensure!(
            self.pages.len() == 2,
            "consumer catalog selector requires exactly two pages"
        );
        Ok([
            self.pages[0].mapper_register(),
            self.pages[1].mapper_register(),
        ])
    }

    pub(super) fn page_for_name(
        &self,
        domain: &'static str,
        source_index: usize,
    ) -> Result<&ConsumerCatalogPage> {
        let identity = self
            .identity_pages
            .iter()
            .find(|identity| identity.domain == domain && identity.source_index == source_index)
            .with_context(|| {
                format!("catalog page missing {domain} source index {source_index}")
            })?;
        self.pages
            .get(identity.page_index)
            .context("catalog identity page index is outside the page list")
    }

    pub(super) fn encode_base_logical(&self, logical: &[FixedTextLogicalByte]) -> Result<Vec<u8>> {
        logical
            .iter()
            .map(|byte| match byte {
                FixedTextLogicalByte::Encoded(value) => Ok(*value),
                FixedTextLogicalByte::TargetGlyph(glyph) => self
                    .base_assignments
                    .get(glyph)
                    .copied()
                    .with_context(|| format!("consumer catalog has no base code for {glyph:?}")),
            })
            .collect()
    }
}

#[derive(Serialize)]
pub(super) struct ConsumerCatalogPage {
    id: String,
    page_index: usize,
    physical_page: u8,
    mapper_register: u8,
    base_glyph_count: usize,
    additional_name_glyph_count: usize,
    slot_demand: usize,
    playable_identity_count: usize,
    enemy_identity_count: usize,
    assignment_sha1: String,
    page_sha1: String,
    #[serde(skip)]
    assignments: BTreeMap<char, u8>,
    #[serde(skip)]
    pub(super) bytes: Vec<u8>,
}

impl ConsumerCatalogPage {
    pub(super) fn physical_page(&self) -> u8 {
        self.physical_page
    }

    pub(super) fn mapper_register(&self) -> u8 {
        self.mapper_register
    }

    pub(super) fn assignments(&self) -> &BTreeMap<char, u8> {
        &self.assignments
    }
}

#[derive(Serialize)]
struct CatalogIdentityPage {
    domain: &'static str,
    source_index: usize,
    page_index: usize,
    physical_page: u8,
    mapper_register: u8,
}

pub(super) fn plan_consumer_catalog(
    inputs: ConsumerCatalogInputs<'_>,
) -> Result<ConsumerCatalogPlan> {
    ensure!(
        inputs.source_font_page.len() == FONT_PAGE_SIZE,
        "consumer catalog source font page is not 4 KiB"
    );
    let item_entries = table_entries(inputs.fixed, "item-names");
    let class_entries = table_entries(inputs.fixed, "class-names");
    let enemy_entries = table_entries(inputs.fixed, "enemy-names");
    ensure!(
        item_entries.len() == 91 && class_entries.len() == 22 && enemy_entries.len() == 69,
        "consumer catalog fixed-text populations changed"
    );

    let summary_label_ids = summary_and_status_label_ids();
    let mut base_logical = item_entries
        .iter()
        .chain(&class_entries)
        .flat_map(|entry| entry.logical_bytes.iter())
        .cloned()
        .collect::<Vec<_>>();
    for id in &summary_label_ids {
        base_logical.extend_from_slice(
            inputs
                .unit_ui
                .entry_logical_bytes(id)
                .with_context(|| format!("consumer catalog lost unit UI label {id}"))?,
        );
    }
    for id in inputs.item_actions.entry_ids() {
        base_logical.extend_from_slice(
            inputs
                .item_actions
                .entry_logical_bytes(id)
                .with_context(|| format!("consumer catalog lost item action {id}"))?,
        );
    }
    let base_glyphs = target_glyphs(&base_logical);

    let mut all_logical = base_logical.clone();
    all_logical.extend(
        inputs
            .unit_names
            .entries
            .iter()
            .chain(enemy_entries.iter().copied())
            .flat_map(|entry| entry.logical_bytes.iter())
            .cloned(),
    );
    let mut preserved_active_codes = inputs.preserved_unit_ui_display_codes.clone();
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    preserved_active_codes.extend(all_logical.iter().filter_map(|byte| match byte {
        FixedTextLogicalByte::Encoded(code) if active_codes.contains(code) => Some(*code),
        FixedTextLogicalByte::Encoded(_) | FixedTextLogicalByte::TargetGlyph(_) => None,
    }));
    let available_codes = assignable_catalog_codes(&preserved_active_codes)?;
    ensure!(
        base_glyphs.len() < available_codes.len(),
        "consumer catalog base needs {} glyphs but only {} active codes remain",
        base_glyphs.len(),
        available_codes.len()
    );
    let base_assignments = base_glyphs
        .iter()
        .copied()
        .zip(available_codes.iter().copied())
        .collect::<BTreeMap<_, _>>();
    let extra_codes = available_codes[base_assignments.len()..].to_vec();

    let demands = inputs
        .unit_names
        .entries
        .iter()
        .map(|entry| name_demand("unit_names", entry, &base_glyphs))
        .chain(
            enemy_entries
                .iter()
                .map(|entry| name_demand("enemy_names", entry, &base_glyphs)),
        )
        .collect::<Vec<_>>();
    let packing = pack_name_demands(&demands, extra_codes.len())?;
    ensure!(
        !packing.pages.is_empty() && packing.pages.len() <= inputs.available_page_count,
        "consumer catalog needs {} pages but only {} remain",
        packing.pages.len(),
        inputs.available_page_count
    );

    let mut pages = Vec::with_capacity(packing.pages.len());
    for (page_index, name_glyphs) in packing.pages.iter().enumerate() {
        let physical_page = inputs
            .first_physical_page
            .checked_add(u8::try_from(page_index).context("catalog page index exceeds u8")?)
            .context("catalog physical page overflow")?;
        let mut assignments = base_assignments.clone();
        assignments.extend(name_glyphs.iter().copied().zip(extra_codes.iter().copied()));
        ensure!(
            assignments.len() + preserved_active_codes.len() <= ACTIVE_HANGUL_SLOT_COUNT,
            "catalog page {page_index} exceeds the active code ceiling"
        );
        let glyphs_by_code = assignments
            .iter()
            .map(|(glyph, code)| (*code, *glyph))
            .collect::<BTreeMap<_, _>>();
        ensure!(
            glyphs_by_code.len() == assignments.len(),
            "catalog page {page_index} assigns one code to multiple glyphs"
        );
        let bytes = build_font_page_by_code(inputs.source_font_page, &glyphs_by_code)?;
        for code in &preserved_active_codes {
            let tile_start = usize::from(*code) * crate::font_slots::FONT_TILE_SIZE;
            let tile_end = tile_start + crate::font_slots::FONT_TILE_SIZE;
            ensure!(
                bytes[tile_start..tile_end] == inputs.source_font_page[tile_start..tile_end],
                "catalog page {page_index} changed preserved source tile 0x{code:02X}"
            );
        }
        let identity_count = |domain| {
            packing
                .identity_page_indices
                .iter()
                .filter(|((identity_domain, _), assigned_page)| {
                    *identity_domain == domain && **assigned_page == page_index
                })
                .count()
        };
        pages.push(ConsumerCatalogPage {
            id: format!("unit_ui_catalog_{page_index:02}"),
            page_index,
            physical_page,
            mapper_register: encode_chr_page_register(physical_page)?,
            base_glyph_count: base_glyphs.len(),
            additional_name_glyph_count: name_glyphs.len(),
            slot_demand: assignments.len() + preserved_active_codes.len(),
            playable_identity_count: identity_count("unit_names"),
            enemy_identity_count: identity_count("enemy_names"),
            assignment_sha1: assignment_sha1(&assignments),
            page_sha1: sha1_hex(&bytes),
            assignments,
            bytes,
        });
    }
    let identity_pages = packing
        .identity_page_indices
        .into_iter()
        .map(|((domain, source_index), page_index)| {
            let page = &pages[page_index];
            CatalogIdentityPage {
                domain,
                source_index,
                page_index,
                physical_page: page.physical_page,
                mapper_register: page.mapper_register,
            }
        })
        .collect::<Vec<_>>();
    ensure!(
        identity_pages.len() == demands.len()
            && identity_pages
                .iter()
                .all(|identity| identity.page_index < pages.len()),
        "consumer catalog did not assign every name identity exactly once"
    );
    let maximum_page_slot_demand = pages
        .iter()
        .map(|page| page.slot_demand)
        .max()
        .context("consumer catalog emitted no pages")?;

    Ok(ConsumerCatalogPlan {
        schema: 1,
        strategy: "preserve source-bound direct unit-UI glyphs; keep item, class, summary/status, and item-action glyphs at stable codes on every page; partition mutually exclusive unit and enemy name identities across deterministic best-fit pages",
        base_glyph_count: base_glyphs.len(),
        preserved_active_code_count: preserved_active_codes.len(),
        per_page_name_slot_count: extra_codes.len(),
        playable_name_count: inputs.unit_names.entries.len(),
        enemy_name_count: enemy_entries.len(),
        name_identity_count: demands.len(),
        page_count: pages.len(),
        first_physical_page: inputs.first_physical_page,
        available_page_count: inputs.available_page_count,
        maximum_page_slot_demand,
        pages,
        identity_pages,
        every_base_glyph_has_one_stable_code: true,
        every_name_identity_fits_one_page: true,
        every_page_fits_active_codes: true,
        pages_fit_reclaimable_tail: true,
        base_assignments,
    })
}

fn assignable_catalog_codes(preserved_active_codes: &BTreeSet<u8>) -> Result<Vec<u8>> {
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    ensure!(
        preserved_active_codes.is_subset(&active_codes),
        "consumer catalog display preservation includes a non-active font code"
    );
    Ok(active_codes
        .difference(preserved_active_codes)
        .copied()
        .collect())
}

fn table_entries<'a>(plan: &'a FixedTextPlan, table_id: &str) -> Vec<&'a FixedTextPlannedEntry> {
    plan.entries
        .iter()
        .filter(|entry| entry.table_id == table_id)
        .collect()
}

fn name_demand(
    domain: &'static str,
    entry: &FixedTextPlannedEntry,
    base_glyphs: &BTreeSet<char>,
) -> CatalogNameDemand {
    CatalogNameDemand {
        domain,
        source_index: entry.source_index,
        additional_glyphs: entry
            .unique_glyphs()
            .difference(base_glyphs)
            .copied()
            .collect(),
    }
}

fn target_glyphs(logical: &[FixedTextLogicalByte]) -> BTreeSet<char> {
    logical
        .iter()
        .filter_map(|byte| match byte {
            FixedTextLogicalByte::TargetGlyph(glyph) => Some(*glyph),
            FixedTextLogicalByte::Encoded(_) => None,
        })
        .collect()
}

fn assignment_sha1(assignments: &BTreeMap<char, u8>) -> String {
    let mut bytes = Vec::new();
    for (glyph, code) in assignments {
        bytes.extend_from_slice(glyph.to_string().as_bytes());
        bytes.push(*code);
    }
    sha1_hex(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_bound_unit_ui_codes_are_never_catalog_assignment_slots() {
        let preserved = BTreeSet::from([0xAD, 0xAF, 0xBF]);

        let assignable = assignable_catalog_codes(&preserved).unwrap();

        assert!(preserved.iter().all(|code| !assignable.contains(code)));
        assert_eq!(assignable.len() + preserved.len(), ACTIVE_HANGUL_SLOT_COUNT);
    }
}
