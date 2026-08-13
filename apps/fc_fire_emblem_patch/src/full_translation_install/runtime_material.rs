use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::sha1_hex;

const MATERIAL_MAGIC: &[u8; 4] = b"FDRM";
const MATERIAL_SCHEMA: u8 = 1;
pub(super) const MATERIAL_HEADER_BYTE_COUNT: usize = 16;
pub(super) const SECTION_DESCRIPTOR_BYTE_COUNT: usize = 6;
/// 용기가 차지하는 MMC3 8 KiB 페이지 수다.
///
/// 셋이었다가 넷이 됐다. 그룹별 타일 목록이 스캔 재료에 들어오면서 세 페이지로는
/// 실행 코드 자리가 남지 않았다. 페이지 `2F`부터 `3D`까지가 전부 비어 있어 늘려도
/// 밀려나는 도메인이 없다.
pub(super) const RUNTIME_MATERIAL_PAGE_COUNT: usize = 4;
/// 용기가 시작하는 MMC3 페이지다.
pub(super) const RUNTIME_MATERIAL_FIRST_PAGE: u8 = 0x2C;
/// 실행 코드가 놓이는 페이지다. 용기의 마지막 장이고 `$A000` 창에 걸린다.
pub(super) const RUNTIME_CODE_MMC3_PAGE: u8 =
    RUNTIME_MATERIAL_FIRST_PAGE + RUNTIME_MATERIAL_PAGE_COUNT as u8 - 1;
const MMC3_PAGE_BYTE_COUNT: usize = 8 * 1024;
const RUNTIME_MATERIAL_CAPACITY: usize = RUNTIME_MATERIAL_PAGE_COUNT * MMC3_PAGE_BYTE_COUNT;
const CONTENT_EMITTED_FLAG: u8 = 1;
const RUNTIME_CODE_SECTION_ID: u8 = 5;
/// 자료 구역 넷과 실행 코드 예약 하나다. 용기 안에서 payload가 시작하는 자리를
/// 계산할 때 쓴다.
pub(super) const MATERIAL_SECTION_COUNT: usize = 5;
/// 용기의 마지막 페이지가 걸리는 CPU 창의 시작이다.
const RUNTIME_CODE_WINDOW_START: usize = 0xA000;
/// 세 페이지 용기 안에서 실행 코드에 남겨 두기로 한 하한이다. 아직 코드를 쓰지 않아
/// 실제 크기는 모르지만, 이 값은 자료 배치를 정할 때 이미 확보해 둔 자리다.
/// 자료가 커져 이 아래로 내려가면 배치를 다시 정해야 한다.
const MINIMUM_RUNTIME_CODE_RESERVATION: usize = 1_888;

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
    // 용기는 세 MMC3 페이지로 고정한다. 다른 도메인의 시작점이 후속 구현에서 움직이지
    // 않게 하려는 것이라 정확한 크기 자체가 요구사항이다.
    ensure!(
        plan.material.len() == RUNTIME_MATERIAL_CAPACITY,
        "main-dialogue runtime material container is no longer three MMC3 pages"
    );
    // 반면 payload와 실행 코드 예약이 나뉘는 지점은 요구사항이 아니다. 자료가 줄면
    // 예약이 늘어야 정상이다. 지킬 것은 예약이 확보한 하한이다. 의사결정 56번을 따른다.
    ensure!(
        plan.runtime_code_reserved_byte_count >= MINIMUM_RUNTIME_CODE_RESERVATION,
        "main-dialogue runtime material left only {} bytes for runtime code, below the {MINIMUM_RUNTIME_CODE_RESERVATION}-byte reservation",
        plan.runtime_code_reserved_byte_count
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

impl DialogueRuntimeMaterialPlan {
    /// 실행 코드가 놓이는 CPU 주소다. 용기는 페이지 `2C`부터 세 장이고 `$A000` 창에
    /// 걸리는 것은 마지막 장이므로, 예약 시작에서 그 장의 시작을 뺀 만큼이 창 안의
    /// 위치가 된다.
    pub(super) fn runtime_code_cpu_start(&self) -> Result<u16> {
        let last_page_offset = (RUNTIME_MATERIAL_PAGE_COUNT - 1) * MMC3_PAGE_BYTE_COUNT;
        let within_window = self
            .runtime_code_offset
            .checked_sub(last_page_offset)
            .context("runtime code reservation is not inside the last container page")?;
        u16::try_from(RUNTIME_CODE_WINDOW_START + within_window)
            .context("runtime code CPU start does not fit the A000 window")
    }

    /// 글리프 atlas가 용기 안에서 시작하는 곳이다.
    pub(super) fn glyph_atlas_offset(&self) -> Result<usize> {
        self.sections
            .iter()
            .find(|section| section.role == "glyph_atlas")
            .map(|section| section.offset)
            .context("runtime material has no glyph atlas section")
    }

    /// 방출한 실행 코드를 예약 자리에 넣는다.
    pub(super) fn place_runtime_code(&mut self, code: &[u8]) -> Result<()> {
        let reserved = self
            .material
            .get_mut(self.runtime_code_offset..)
            .context("runtime code reservation is outside the container")?;
        ensure!(
            code.len() <= reserved.len(),
            "runtime code is {} bytes and the reservation holds {}",
            code.len(),
            reserved.len()
        );
        ensure!(
            reserved.iter().all(|byte| *byte == 0xFF),
            "runtime code reservation is not exact FF before placement"
        );
        reserved[..code.len()].copy_from_slice(code);
        self.runtime_code_emitted = true;
        self.material_sha1 = sha1_hex(&self.material);
        Ok(())
    }
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
