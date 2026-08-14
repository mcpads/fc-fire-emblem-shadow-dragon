use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::sha1_hex;

const MATERIAL_MAGIC: &[u8; 4] = b"FDRM";
const MATERIAL_SCHEMA: u8 = 1;
pub(super) const MATERIAL_HEADER_BYTE_COUNT: usize = 16;
pub(super) const SECTION_DESCRIPTOR_BYTE_COUNT: usize = 6;
/// 용기가 차지하는 MMC3 8 KiB 페이지 수다.
///
/// 셋에서 넷, 넷에서 다섯이 됐다. 그룹 덩이가 항목마다 atlas 주소를 담으면서
/// 커졌기 때문이다.
///
/// 이 교환을 받아들이는 이유는 두 자원의 여유가 다르기 때문이다. PRG는 페이지
/// `31`부터 `3D`까지 13장이 아직 비어 있고, vblank는 프레임당 1,243사이클이 전부다.
/// 주소를 미리 더해 두면 소비자가 타일마다 쓰는 가변 시프트 루프가 통째로 사라진다.
/// 남는 것을 써서 모자란 것을 사는 쪽이 맞다.
pub(super) const RUNTIME_MATERIAL_PAGE_COUNT: usize = 5;
/// 용기가 시작하는 MMC3 페이지다.
pub(super) const RUNTIME_MATERIAL_FIRST_PAGE: u8 = 0x2C;
/// 실행 코드가 놓이는 페이지다. 용기의 마지막 장이고 `$A000` 창에 걸린다.
pub(super) const RUNTIME_CODE_MMC3_PAGE: u8 =
    RUNTIME_MATERIAL_FIRST_PAGE + RUNTIME_MATERIAL_PAGE_COUNT as u8 - 1;
const MMC3_PAGE_BYTE_COUNT: usize = 8 * 1024;
const RUNTIME_MATERIAL_CAPACITY: usize = RUNTIME_MATERIAL_PAGE_COUNT * MMC3_PAGE_BYTE_COUNT;
const CONTENT_SUPPLIED_DURING_LAYOUT_FLAG: u8 = 1;
const RUNTIME_CODE_SECTION_ID: u8 = 5;
/// 자료 구역 넷과 실행 코드 예약 하나다. 용기 안에서 payload가 시작하는 자리를
/// 계산할 때 쓴다.
pub(super) const MATERIAL_SECTION_COUNT: usize = 5;
/// 용기의 마지막 페이지가 걸리는 CPU 창의 시작이다.
const RUNTIME_CODE_WINDOW_START: usize = 0xA000;
/// 실행 코드가 받는 자리다. 마지막 페이지 전체이므로 자료 크기와 무관한 상수다.
const MINIMUM_RUNTIME_CODE_RESERVATION: usize = MMC3_PAGE_BYTE_COUNT;

pub(super) struct RuntimeMaterialInputs<'a> {
    pub(super) glyph_atlas: &'a [u8],
    pub(super) page_scan: &'a [u8],
    pub(super) runtime_identity: &'a [u8],
    pub(super) dynamic_producer_encoding: &'a [u8],
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
    runtime_code_routine_placement_count: usize,
    stable_fixed_page_layout: bool,
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
    content_supplied_during_layout: bool,
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
            role: "runtime_identity",
            content: Some(inputs.runtime_identity),
        },
        MaterialSectionInput {
            id: 4,
            role: "dynamic_producer_encoding",
            content: Some(inputs.dynamic_producer_encoding),
        },
    ];
    let plan = encode_runtime_material(&data_sections, RUNTIME_MATERIAL_CAPACITY)?;
    // 용기는 다섯 MMC3 페이지로 고정한다. 다른 도메인의 시작점이 후속 구현에서 움직이지
    // 않게 하려는 것이라 정확한 크기 자체가 요구사항이다.
    ensure!(
        plan.material.len() == RUNTIME_MATERIAL_CAPACITY,
        "main-dialogue runtime material container is no longer five MMC3 pages"
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
    let payload_end = payload_offset
        .checked_add(payload_byte_count)
        .context("runtime material payload range overflow")?;
    // 실행 코드는 용기의 **마지막 페이지 전체**를 쓴다. 남는 만큼 주는 방식이면
    // 자료가 늘 때마다 코드가 놓이는 주소가 움직이고, 그러면 페이지 경계를 걸치는
    // 순간 실행이 불가능해진다. 한 페이지로 고정하면 CPU 시작 주소가 `$A000` 상수다.
    let runtime_code_offset = capacity - MMC3_PAGE_BYTE_COUNT;
    ensure!(
        payload_end <= runtime_code_offset,
        "runtime material payload reaches {payload_end} and overruns the runtime code page at {runtime_code_offset}; section byte counts: {:?}",
        data_sections
            .iter()
            .map(|section| (
                section.role,
                section.content.expect("data sections have content").len()
            ))
            .collect::<Vec<_>>()
    );
    let runtime_code_byte_count = MMC3_PAGE_BYTE_COUNT;

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
            CONTENT_SUPPLIED_DURING_LAYOUT_FLAG,
            offset,
            content.len(),
        )?;
        sections.push(RuntimeMaterialSection {
            id: section.id,
            role: section.role,
            offset,
            byte_count: content.len(),
            content_supplied_during_layout: true,
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
        content_supplied_during_layout: false,
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
        material.len() == capacity,
        "runtime material container length changed"
    );
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
        runtime_code_routine_placement_count: 0,
        stable_fixed_page_layout: capacity == RUNTIME_MATERIAL_CAPACITY,
        material_sha1: sha1_hex(&material),
        material,
    })
}

impl DialogueRuntimeMaterialPlan {
    /// 실행 코드가 놓이는 CPU 주소다. 용기는 페이지 `2C`부터 다섯 장이고 `$A000` 창에
    /// 걸리는 것은 마지막 장이므로, 예약 시작에서 그 장의 시작을 뺀 만큼이 창 안의
    /// 위치가 된다.
    pub(super) fn runtime_code_cpu_start(&self) -> Result<u16> {
        ensure!(
            self.runtime_code_offset == (RUNTIME_MATERIAL_PAGE_COUNT - 1) * MMC3_PAGE_BYTE_COUNT,
            "runtime code no longer starts at the last container page"
        );
        u16::try_from(RUNTIME_CODE_WINDOW_START)
            .context("runtime code CPU start does not fit the A000 window")
    }

    /// 글리프 atlas가 용기 안에서 시작하는 곳이다.
    pub(super) fn glyph_atlas_offset(&self) -> Result<usize> {
        self.section_offset("glyph_atlas")
    }

    /// 구역 하나가 용기 안에서 시작하는 곳이다.
    pub(super) fn section_offset(&self, role: &str) -> Result<usize> {
        self.sections
            .iter()
            .find(|section| section.role == role)
            .map(|section| section.offset)
            .with_context(|| format!("runtime material has no {role} section"))
    }

    /// 실행 코드가 놓이는 MMC3 페이지다.
    pub(super) fn runtime_code_mmc3_page(&self) -> u8 {
        RUNTIME_CODE_MMC3_PAGE
    }

    /// 방출한 실행 코드 조각을 그 CPU 주소가 가리키는 예약 자리에 넣는다.
    pub(super) fn place_runtime_code(&mut self, cpu_address: u16, code: &[u8]) -> Result<()> {
        let within_page = usize::from(cpu_address)
            .checked_sub(RUNTIME_CODE_WINDOW_START)
            .context("runtime code address is outside the A000 window")?;
        let start = self.runtime_code_offset + within_page;
        let destination = self
            .material
            .get_mut(start..start + code.len())
            .context("runtime code does not fit the reserved page")?;
        ensure!(
            destination.iter().all(|byte| *byte == 0xFF),
            "runtime code placement is not exact FF before writing"
        );
        destination.copy_from_slice(code);
        self.runtime_code_routine_placement_count = self
            .runtime_code_routine_placement_count
            .checked_add(1)
            .context("runtime code routine placement count overflow")?;
        self.material_sha1 = sha1_hex(&self.material);
        Ok(())
    }

    pub(super) fn runtime_code_routine_placement_count(&self) -> usize {
        self.runtime_code_routine_placement_count
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

    /// 자료와 실행 코드 자리는 겹치면 안 되고, 코드 자리는 마지막 페이지 전체여야
    /// 한다. 남는 만큼 주면 자료가 늘 때마다 코드 주소가 움직인다.
    #[test]
    fn the_code_reservation_is_always_the_last_page_whatever_the_payload_size() {
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
        let capacity = 2 * MMC3_PAGE_BYTE_COUNT;

        let plan = encode_runtime_material(&sections, capacity).unwrap();
        let payload_offset = MATERIAL_HEADER_BYTE_COUNT + 3 * SECTION_DESCRIPTOR_BYTE_COUNT;

        assert_eq!(&plan.material[..4], MATERIAL_MAGIC);
        assert_eq!(plan.material[5], 3);
        assert_eq!(
            &plan.material[payload_offset..payload_offset + 3],
            &[0x11, 0x12, 0x21]
        );
        assert_eq!(plan.runtime_code_offset, capacity - MMC3_PAGE_BYTE_COUNT);
        assert_eq!(plan.runtime_code_reserved_byte_count, MMC3_PAGE_BYTE_COUNT);
        assert!(
            plan.material[payload_offset + 3..]
                .iter()
                .all(|byte| *byte == 0xFF)
        );
    }

    /// 자료가 코드 페이지를 침범하면 만들지 않는다. 침범한 채로 내보내면 실행 코드
    /// 자리에 자료가 들어가 있는 ROM이 나온다.
    #[test]
    fn a_payload_that_reaches_the_code_page_is_refused() {
        let oversized = vec![0x11; MMC3_PAGE_BYTE_COUNT];
        let sections = [MaterialSectionInput {
            id: 1,
            role: "only",
            content: Some(&oversized),
        }];

        let error = encode_runtime_material(&sections, 2 * MMC3_PAGE_BYTE_COUNT).unwrap_err();

        assert!(error.to_string().contains("overruns the runtime code page"));
    }
}
