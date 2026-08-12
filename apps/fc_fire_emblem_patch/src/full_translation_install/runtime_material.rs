use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::sha1_hex;

const MATERIAL_MAGIC: &[u8; 4] = b"FDRM";
const MATERIAL_SCHEMA: u8 = 1;
const MATERIAL_HEADER_BYTE_COUNT: usize = 16;
const SECTION_DESCRIPTOR_BYTE_COUNT: usize = 6;
const RUNTIME_MATERIAL_PAGE_COUNT: usize = 3;
const MMC3_PAGE_BYTE_COUNT: usize = 8 * 1024;
const RUNTIME_MATERIAL_CAPACITY: usize = RUNTIME_MATERIAL_PAGE_COUNT * MMC3_PAGE_BYTE_COUNT;
const CONTENT_EMITTED_FLAG: u8 = 1;
const RUNTIME_CODE_SECTION_ID: u8 = 5;

pub(super) struct RuntimeMaterialInputs<'a> {
    pub(super) glyph_atlas: &'a [u8],
    pub(super) page_scan: &'a [u8],
    pub(super) dynamic_remap: &'a [u8],
    pub(super) runtime_identity: &'a [u8],
}

#[derive(Debug, Serialize)]
pub(super) struct DialogueRuntimeMaterialPlan {
    schema: u8,
    page_count: usize,
    capacity_byte_count: usize,
    header_byte_count: usize,
    section_descriptor_byte_count: usize,
    sections: Vec<RuntimeMaterialSection>,
    payload_byte_count: usize,
    pub(super) runtime_code_offset: usize,
    runtime_code_reserved_byte_count: usize,
    runtime_code_emitted: bool,
    stable_three_page_layout: bool,
    material_sha1: String,
    #[serde(skip)]
    pub(super) material: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct RuntimeMaterialSection {
    id: u8,
    role: &'static str,
    offset: usize,
    byte_count: usize,
    content_emitted: bool,
    content_sha1: Option<String>,
}

struct MaterialSectionInput<'a> {
    id: u8,
    role: &'static str,
    content: Option<&'a [u8]>,
}

pub(super) fn plan_dialogue_runtime_material(
    inputs: RuntimeMaterialInputs<'_>,
) -> Result<DialogueRuntimeMaterialPlan> {
    let data_sections = [
        MaterialSectionInput {
            id: 1,
            role: "glyph_atlas",
            content: Some(inputs.glyph_atlas),
        },
        MaterialSectionInput {
            id: 2,
            role: "page_scan",
            content: Some(inputs.page_scan),
        },
        MaterialSectionInput {
            id: 3,
            role: "dynamic_remap",
            content: Some(inputs.dynamic_remap),
        },
        MaterialSectionInput {
            id: 4,
            role: "runtime_identity",
            content: Some(inputs.runtime_identity),
        },
    ];
    let plan = encode_runtime_material(&data_sections, RUNTIME_MATERIAL_CAPACITY)?;
    ensure!(
        plan.payload_byte_count == 22_642
            && plan.runtime_code_reserved_byte_count == 1_888
            && plan.material.len() == RUNTIME_MATERIAL_CAPACITY,
        "main-dialogue runtime material population changed"
    );
    Ok(plan)
}

fn encode_runtime_material(
    data_sections: &[MaterialSectionInput<'_>],
    capacity: usize,
) -> Result<DialogueRuntimeMaterialPlan> {
    ensure!(
        !data_sections.is_empty() && data_sections.len() < usize::from(u8::MAX),
        "runtime material data-section count is outside u8"
    );
    ensure!(
        data_sections
            .iter()
            .enumerate()
            .all(|(index, section)| usize::from(section.id) == index + 1),
        "runtime material data-section IDs must be contiguous from one"
    );
    let section_count = data_sections.len() + 1;
    let directory_byte_count = section_count * SECTION_DESCRIPTOR_BYTE_COUNT;
    let payload_offset = MATERIAL_HEADER_BYTE_COUNT + directory_byte_count;
    let payload_byte_count = data_sections
        .iter()
        .map(|section| section.content.expect("data sections have content").len())
        .sum::<usize>();
    let runtime_code_offset = payload_offset
        .checked_add(payload_byte_count)
        .context("runtime material payload range overflow")?;
    ensure!(
        runtime_code_offset < capacity,
        "runtime material leaves no capacity for runtime code"
    );
    let runtime_code_byte_count = capacity - runtime_code_offset;

    let mut material = Vec::with_capacity(capacity);
    material.extend_from_slice(MATERIAL_MAGIC);
    material.push(MATERIAL_SCHEMA);
    material.push(u8::try_from(section_count).context("runtime section count does not fit u8")?);
    material.push(SECTION_DESCRIPTOR_BYTE_COUNT as u8);
    material.push(0);
    push_u16(&mut material, capacity, "runtime material capacity")?;
    push_u16(
        &mut material,
        MATERIAL_HEADER_BYTE_COUNT,
        "runtime material directory offset",
    )?;
    push_u16(
        &mut material,
        runtime_code_offset,
        "runtime material code offset",
    )?;
    push_u16(
        &mut material,
        runtime_code_byte_count,
        "runtime material code reservation",
    )?;
    ensure!(
        material.len() == MATERIAL_HEADER_BYTE_COUNT,
        "runtime material header length changed"
    );

    let mut sections = Vec::with_capacity(section_count);
    let mut offset = payload_offset;
    for section in data_sections {
        let content = section.content.expect("data sections have content");
        write_descriptor(
            &mut material,
            section.id,
            CONTENT_EMITTED_FLAG,
            offset,
            content.len(),
        )?;
        sections.push(RuntimeMaterialSection {
            id: section.id,
            role: section.role,
            offset,
            byte_count: content.len(),
            content_emitted: true,
            content_sha1: Some(sha1_hex(content)),
        });
        offset += content.len();
    }
    write_descriptor(
        &mut material,
        RUNTIME_CODE_SECTION_ID,
        0,
        runtime_code_offset,
        runtime_code_byte_count,
    )?;
    sections.push(RuntimeMaterialSection {
        id: RUNTIME_CODE_SECTION_ID,
        role: "runtime_code",
        offset: runtime_code_offset,
        byte_count: runtime_code_byte_count,
        content_emitted: false,
        content_sha1: None,
    });
    ensure!(
        material.len() == payload_offset,
        "runtime material directory length changed"
    );
    for section in data_sections {
        material.extend_from_slice(section.content.expect("data sections have content"));
    }
    material.resize(capacity, 0xFF);
    ensure!(
        material.len() == capacity
            && material[runtime_code_offset..]
                .iter()
                .all(|byte| *byte == 0xFF),
        "runtime code reservation is not exact FF"
    );

    Ok(DialogueRuntimeMaterialPlan {
        schema: MATERIAL_SCHEMA,
        page_count: capacity.div_ceil(MMC3_PAGE_BYTE_COUNT),
        capacity_byte_count: capacity,
        header_byte_count: MATERIAL_HEADER_BYTE_COUNT,
        section_descriptor_byte_count: SECTION_DESCRIPTOR_BYTE_COUNT,
        sections,
        payload_byte_count,
        runtime_code_offset,
        runtime_code_reserved_byte_count: runtime_code_byte_count,
        runtime_code_emitted: false,
        stable_three_page_layout: capacity == RUNTIME_MATERIAL_CAPACITY,
        material_sha1: sha1_hex(&material),
        material,
    })
}

fn write_descriptor(
    output: &mut Vec<u8>,
    id: u8,
    flags: u8,
    offset: usize,
    byte_count: usize,
) -> Result<()> {
    output.push(id);
    output.push(flags);
    push_u16(output, offset, "runtime material section offset")?;
    push_u16(output, byte_count, "runtime material section length")?;
    Ok(())
}

fn push_u16(output: &mut Vec<u8>, value: usize, role: &str) -> Result<()> {
    output.extend_from_slice(
        &u16::try_from(value)
            .with_context(|| format!("{role} does not fit u16"))?
            .to_le_bytes(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_directory_keeps_payloads_and_code_reservation_disjoint() {
        let sections = [
            MaterialSectionInput {
                id: 1,
                role: "first",
                content: Some(&[0x11, 0x12]),
            },
            MaterialSectionInput {
                id: 2,
                role: "second",
                content: Some(&[0x21]),
            },
        ];

        let plan = encode_runtime_material(&sections, 64).unwrap();
        let payload_offset = MATERIAL_HEADER_BYTE_COUNT + 3 * SECTION_DESCRIPTOR_BYTE_COUNT;

        assert_eq!(&plan.material[..4], MATERIAL_MAGIC);
        assert_eq!(plan.material[5], 3);
        assert_eq!(
            &plan.material[payload_offset..payload_offset + 3],
            &[0x11, 0x12, 0x21]
        );
        assert_eq!(
            plan.runtime_code_reserved_byte_count,
            64 - payload_offset - 3
        );
        assert!(
            plan.material[payload_offset + 3..]
                .iter()
                .all(|byte| *byte == 0xFF)
        );
    }

    #[test]
    fn container_rejects_a_payload_that_leaves_no_runtime_code_space() {
        let sections = [MaterialSectionInput {
            id: 1,
            role: "only",
            content: Some(&[0x11; 42]),
        }];

        let error = encode_runtime_material(&sections, 64).unwrap_err();

        assert!(error.to_string().contains("leaves no capacity"));
    }
}
