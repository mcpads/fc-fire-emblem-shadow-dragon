use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::{ChapterTitlePlan, TransitionTranslationPlans},
    choice_labels::ChoiceLabelPlan,
    map_menu::MapMenuPlan,
    rom::HEADER_SIZE,
    semantic_translation::SemanticTranslationPlan,
    sha1_hex,
    text_inventory::FixedTextPlan,
    unit_names::UnitNamePlan,
};

use super::{
    consumer_catalog::{
        ConsumerCatalogPlan, ConsumerCatalogRuntimeLayout, ConsumerCatalogRuntimeMaterialInputs,
        ConsumerCatalogRuntimeMaterialPlan, plan_consumer_catalog_runtime_material,
    },
    installation_layout::cross_domain_material_pool,
};
use encoding::{GLYPH_CELL_FLAG, encode_section};
use entries::{collect_section_inputs, entry_identity_sha1};

mod encoding;
mod entries;

const SECTION_ALIGNMENT: usize = 16;
const MMC3_PAGE_BYTE_COUNT: usize = 8 * 1024;

pub(super) struct CrossDomainMaterialInputs<'a> {
    pub(super) main_dialogue_runtime_material_byte_count: usize,
    pub(super) shared_atlas_characters: &'a [char],
    pub(super) fixed: &'a FixedTextPlan,
    pub(super) unit_names: &'a UnitNamePlan,
    pub(super) chapter_titles: &'a ChapterTitlePlan,
    pub(super) choices: &'a ChoiceLabelPlan,
    pub(super) map_menu: &'a MapMenuPlan,
    pub(super) unit_ui: &'a SemanticTranslationPlan,
    pub(super) item_actions: &'a SemanticTranslationPlan,
    pub(super) fixed_menu_labels: &'a SemanticTranslationPlan,
    pub(super) transitions: &'a TransitionTranslationPlans,
    pub(super) locations: &'a FixedTextPlan,
    pub(super) consumer_catalog: &'a ConsumerCatalogPlan,
}

#[derive(Serialize)]
pub(super) struct CrossDomainMaterialPlan {
    schema: u8,
    first_mmc3_page: u8,
    capacity_byte_count: usize,
    material_span_byte_count: usize,
    material_payload_byte_count: usize,
    alignment_padding_byte_count: usize,
    section_count: usize,
    entry_count: usize,
    shared_atlas_tile_count: usize,
    sections: Vec<CrossDomainMaterialSection>,
    consumer_catalog_runtime: ConsumerCatalogRuntimeMaterialPlan,
    every_required_non_dialogue_domain_serialized: bool,
    every_target_glyph_resolves_to_shared_atlas: bool,
    capacity_bound: bool,
}

impl CrossDomainMaterialPlan {
    pub(super) fn material_span_byte_count(&self) -> usize {
        self.material_span_byte_count
    }

    pub(super) fn sections(&self) -> &[CrossDomainMaterialSection] {
        &self.sections
    }

    pub(super) fn consumer_catalog_runtime(&self) -> &ConsumerCatalogRuntimeMaterialPlan {
        &self.consumer_catalog_runtime
    }

    pub(super) fn consumer_catalog_runtime_layout(&self) -> Result<ConsumerCatalogRuntimeLayout> {
        self.consumer_catalog_runtime.layout()
    }
}

#[derive(Serialize)]
pub(super) struct CrossDomainMaterialSection {
    pub(super) id: &'static str,
    translation_input_sha1: String,
    entry_count: usize,
    entry_identity_sha1: String,
    pub(super) file_offset: usize,
    byte_count: usize,
    content_sha1: String,
    #[serde(skip)]
    pub(super) bytes: Vec<u8>,
}

pub(super) fn plan_cross_domain_material(
    inputs: CrossDomainMaterialInputs<'_>,
) -> Result<CrossDomainMaterialPlan> {
    ensure!(
        inputs
            .shared_atlas_characters
            .windows(2)
            .all(|pair| pair[0] < pair[1]),
        "shared glyph atlas characters are not strictly sorted"
    );
    let atlas_indices = inputs
        .shared_atlas_characters
        .iter()
        .copied()
        .enumerate()
        .map(|(index, glyph)| {
            let index = u16::try_from(index).context("shared glyph atlas index exceeds u16")?;
            ensure!(
                index < GLYPH_CELL_FLAG,
                "shared glyph atlas index uses the material cell tag bit"
            );
            Ok((glyph, index))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let section_inputs = collect_section_inputs(&inputs)?;

    let pool = cross_domain_material_pool(inputs.main_dialogue_runtime_material_byte_count)?;
    let mut cursor = pool.file_offset;
    let mut payload_byte_count = 0usize;
    let mut alignment_padding_byte_count = 0usize;
    let mut sections = Vec::with_capacity(section_inputs.len());
    for input in section_inputs {
        let aligned = align_up(cursor, SECTION_ALIGNMENT)?;
        alignment_padding_byte_count += aligned - cursor;
        cursor = aligned;
        let bytes = encode_section(&input.entries, &atlas_indices)?;
        let entry_identity_sha1 =
            entry_identity_sha1(&input.translation_input_sha1, &input.entries);
        payload_byte_count = payload_byte_count
            .checked_add(bytes.len())
            .context("cross-domain material payload size overflow")?;
        sections.push(CrossDomainMaterialSection {
            id: input.id,
            translation_input_sha1: input.translation_input_sha1,
            entry_count: input.entries.len(),
            entry_identity_sha1,
            file_offset: cursor,
            byte_count: bytes.len(),
            content_sha1: sha1_hex(&bytes),
            bytes,
        });
        cursor = cursor
            .checked_add(sections.last().expect("section was pushed").byte_count)
            .context("cross-domain material address overflow")?;
    }
    let catalog_file_offset = align_mmc3_page(cursor)?;
    alignment_padding_byte_count += catalog_file_offset - cursor;
    let catalog_page = u8::try_from((catalog_file_offset - HEADER_SIZE) / MMC3_PAGE_BYTE_COUNT)
        .context("consumer catalog runtime page exceeds u8")?;
    let consumer_catalog_runtime =
        plan_consumer_catalog_runtime_material(ConsumerCatalogRuntimeMaterialInputs {
            file_offset: catalog_file_offset,
            mmc3_page: catalog_page,
            fixed: inputs.fixed,
            unit_names: inputs.unit_names,
            catalog: inputs.consumer_catalog,
        })?;
    payload_byte_count = payload_byte_count
        .checked_add(consumer_catalog_runtime.bytes.len())
        .context("consumer catalog runtime payload size overflow")?;
    cursor = catalog_file_offset
        .checked_add(consumer_catalog_runtime.bytes.len())
        .context("consumer catalog runtime address overflow")?;
    let material_span_byte_count = cursor
        .checked_sub(pool.file_offset)
        .context("cross-domain material starts after its end")?;
    ensure!(
        material_span_byte_count <= pool.capacity_byte_count,
        "cross-domain material needs {material_span_byte_count} bytes but only {} remain",
        pool.capacity_byte_count
    );
    let entry_count = sections.iter().map(|section| section.entry_count).sum();

    Ok(CrossDomainMaterialPlan {
        schema: 1,
        first_mmc3_page: pool.first_mmc3_page,
        capacity_byte_count: pool.capacity_byte_count,
        material_span_byte_count,
        material_payload_byte_count: payload_byte_count,
        alignment_padding_byte_count,
        section_count: sections.len(),
        entry_count,
        shared_atlas_tile_count: atlas_indices.len(),
        sections,
        consumer_catalog_runtime,
        every_required_non_dialogue_domain_serialized: true,
        every_target_glyph_resolves_to_shared_atlas: true,
        capacity_bound: true,
    })
}

fn align_mmc3_page(file_offset: usize) -> Result<usize> {
    let prg_offset = file_offset
        .checked_sub(HEADER_SIZE)
        .context("cross-domain material begins before the iNES PRG")?;
    HEADER_SIZE
        .checked_add(prg_offset.div_ceil(MMC3_PAGE_BYTE_COUNT) * MMC3_PAGE_BYTE_COUNT)
        .context("cross-domain MMC3 page alignment overflow")
}

fn align_up(value: usize, alignment: usize) -> Result<usize> {
    ensure!(
        alignment.is_power_of_two(),
        "material alignment is not a power of two"
    );
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .context("material alignment overflow")
}
