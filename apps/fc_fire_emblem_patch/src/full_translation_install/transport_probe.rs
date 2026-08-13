//! 전송 계층만 실은 탐침 ROM을 만든다.
//!
//! 전체 설치는 부분 ROM을 내보내지 않는다. 그런데 소비자가 vblank를 넘지 않는지는
//! 실행해 봐야만 알 수 있고, 그 확인이 나머지 도메인을 기다릴 이유가 없다. 그래서
//! 전투 합성이 쓰던 것과 같은 탐침 방식으로 전송 계층만 따로 싣는다.
//!
//! 탐침이 확인하려는 것은 **타이밍**이다. 글리프 atlas는 아직 설치되지 않아 읽는
//! 바이트가 `FF`지만, 사이클 비용은 내용과 무관하므로 확인에 영향이 없다. 화면도
//! 깨지지 않는다. 원본은 CHR RAM을 고르지 않으므로 `$2007` 쓰기가 보이는 타일에
//! 닿지 않기 때문이다.

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::{
    runtime_code::{
        DialogueRuntimeCodePlan, dispatcher_gate::{COLD_ENTRY, DISPATCHER_ENTRY},
        plan_dialogue_runtime_code,
    },
    runtime_nmi_contract::CONSUMER_HOOK,
};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
};

const MAIN_DIALOGUE_BANK: u8 = 0x0A;
const FIXED_BANK_SIZE: usize = 16 * 1024;
const MMC3_PAGE_BYTE_COUNT: usize = 8 * 1024;
use super::runtime_material::{RUNTIME_CODE_MMC3_PAGE, RUNTIME_MATERIAL_FIRST_PAGE as MATERIAL_FIRST_PAGE};
/// 재료 용기 헤더와 구역 표를 지난 자리다. 실제 설치에서 atlas가 시작하는 곳과 같다.
const ATLAS_CONTAINER_OFFSET: u16 = 46;
/// 실행 코드가 놓이는 CPU 주소다. 용기의 마지막 페이지 전체를 쓰므로 상수다.
const RUNTIME_CODE_CPU_START: u16 = 0xA000;
/// 탐침이 발행하는 콜드 요청의 타일 수다. 한 그룹이 요구할 수 있는 최대치를 쓴다.
/// 최악의 프레임 수를 그대로 재려는 것이다.
const PROBE_TILE_COUNT: u8 = 206;

#[derive(Debug, Serialize)]
pub(crate) struct DialogueTransportProbePlan {
    pub(crate) schema: u8,
    pub(crate) input_sha1: String,
    pub(crate) output_sha1: String,
    pub(crate) runtime_code_cpu_start_hex: String,
    pub(crate) transport_byte_count: usize,
    pub(crate) fixed_routines: Vec<ProbeRoutine>,
    pub(crate) hook_count: usize,
    pub(crate) cold_request_tile_count: u8,
    pub(crate) frames_to_complete_one_cold_request: usize,
    pub(crate) glyph_atlas_installed: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProbeRoutine {
    pub(crate) role: &'static str,
    pub(crate) cpu_address_hex: String,
    pub(crate) byte_count: usize,
}

pub(crate) fn build_dialogue_transport_probe(
    source: &Rom,
    candidate: &Rom,
) -> Result<(Vec<u8>, DialogueTransportProbePlan)> {
    let atlas_cpu_base = 0x8000 + ATLAS_CONTAINER_OFFSET;
    let code = plan_dialogue_runtime_code(
        source,
        candidate,
        RUNTIME_CODE_CPU_START,
        MATERIAL_FIRST_PAGE,
        atlas_cpu_base,
        PROBE_TILE_COUNT,
    )?;

    let mut output = candidate.data().to_vec();
    apply(&mut output, candidate, &code)?;

    let plan = DialogueTransportProbePlan {
        schema: 1,
        input_sha1: sha1_hex(candidate.data()),
        output_sha1: sha1_hex(&output),
        runtime_code_cpu_start_hex: format!("0x{RUNTIME_CODE_CPU_START:04X}"),
        transport_byte_count: code.transport.bytes.len(),
        fixed_routines: code
            .fixed_routines
            .iter()
            .map(|routine| ProbeRoutine {
                role: routine.role,
                cpu_address_hex: format!("0x{:04X}", routine.address),
                byte_count: routine.bytes.len(),
            })
            .collect(),
        hook_count: 3,
        cold_request_tile_count: PROBE_TILE_COUNT,
        frames_to_complete_one_cold_request: usize::from(PROBE_TILE_COUNT)
            .div_ceil(usize::from(super::runtime_code::transport::TILES_PER_FRAME)),
        glyph_atlas_installed: false,
    };
    Ok((output, plan))
}

fn apply(output: &mut [u8], candidate: &Rom, code: &DialogueRuntimeCodePlan) -> Result<()> {
    let transport_offset = runtime_code_file_offset(candidate, code.transport.address)?;
    write_reserved(
        output,
        transport_offset,
        &code.transport.bytes,
        code.transport.role,
    )?;
    for routine in &code.fixed_routines {
        let offset = fixed_file_offset(candidate, routine.address)?;
        write_reserved(output, offset, &routine.bytes, routine.role)?;
    }
    for (role, offset, bytes) in [
        (
            "dialogue consumer hook",
            fixed_file_offset(candidate, CONSUMER_HOOK)?,
            code.consumer_hook,
        ),
        (
            "dialogue dispatcher hook",
            switchable_cpu_to_file_offset(MAIN_DIALOGUE_BANK, DISPATCHER_ENTRY)?,
            code.dispatcher_hook,
        ),
        (
            "dialogue cold initializer hook",
            switchable_cpu_to_file_offset(MAIN_DIALOGUE_BANK, COLD_ENTRY)?,
            code.cold_hook,
        ),
    ] {
        let destination = output
            .get_mut(offset..offset + bytes.len())
            .with_context(|| format!("{role} is outside the probe image"))?;
        ensure!(
            destination != bytes,
            "{role} is already installed; the probe base is not clean"
        );
        destination.copy_from_slice(&bytes);
    }
    Ok(())
}

/// 예약 자리에만 쓴다. `FF`가 아니면 원본을 덮는 것이므로 거부한다.
fn write_reserved(output: &mut [u8], offset: usize, bytes: &[u8], role: &str) -> Result<()> {
    let destination = output
        .get_mut(offset..offset + bytes.len())
        .with_context(|| format!("{role} is outside the probe image"))?;
    ensure!(
        destination.iter().all(|byte| *byte == 0xFF),
        "{role} would overwrite bytes that are not reserved"
    );
    destination.copy_from_slice(bytes);
    Ok(())
}

fn fixed_file_offset(rom: &Rom, address: u16) -> Result<usize> {
    ensure!(address >= 0xC000, "fixed-bank address is below C000");
    let base = rom
        .prg()
        .len()
        .checked_sub(FIXED_BANK_SIZE)
        .context("PRG is smaller than one fixed bank")?;
    Ok(HEADER_SIZE + base + usize::from(address) - 0xC000)
}

/// 실행 코드는 용기의 마지막 페이지에 있고 그 페이지가 `$A000` 창에 걸린다.
fn runtime_code_file_offset(rom: &Rom, address: u16) -> Result<usize> {
    ensure!(
        (0xA000..0xC000).contains(&address),
        "runtime code address is outside the A000 window"
    );
    let page = usize::from(RUNTIME_CODE_MMC3_PAGE);
    ensure!(
        (page + 1) * MMC3_PAGE_BYTE_COUNT <= rom.prg().len(),
        "the runtime code page is outside PRG"
    );
    Ok(HEADER_SIZE + page * MMC3_PAGE_BYTE_COUNT + usize::from(address) - 0xA000)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 탐침은 예약된 자리와 훅 셋만 바꾼다. 그 밖을 건드리면 확인 대상이 아닌
    /// 변화가 섞여 무엇을 재는지 알 수 없게 된다.
    #[test]
    fn the_probe_changes_nothing_outside_the_reserved_regions_and_the_hook_sites() {
        let rom = crate::test_support::release_rom();
        let hook_sites = [
            fixed_file_offset(&rom, CONSUMER_HOOK).unwrap(),
            switchable_cpu_to_file_offset(MAIN_DIALOGUE_BANK, DISPATCHER_ENTRY).unwrap(),
            switchable_cpu_to_file_offset(MAIN_DIALOGUE_BANK, COLD_ENTRY).unwrap(),
        ];

        let (output, _) = build_dialogue_transport_probe(&rom, &rom).unwrap();

        for (index, (before, after)) in rom.data().iter().zip(&output).enumerate() {
            if before == after {
                continue;
            }
            let reserved = *before == 0xFF;
            let in_hook = hook_sites.iter().any(|site| (*site..site + 3).contains(&index));
            assert!(
                reserved || in_hook,
                "the probe changed {index:#X}, which is neither reserved nor a hook site"
            );
        }
    }

    /// 한 콜드 요청이 몇 프레임 걸리는지가 이 계층의 품질 지표다. 정확성 지표가
    /// 아니므로 값을 고정하지 않고 관측 가능한지만 확인한다.
    #[test]
    fn the_probe_reports_how_long_one_cold_request_takes() {
        let rom = crate::test_support::release_rom();

        let (_, plan) = build_dialogue_transport_probe(&rom, &rom).unwrap();

        assert!(plan.frames_to_complete_one_cold_request > 0);
        assert!(!plan.glyph_atlas_installed);
    }

    /// 이미 설치된 이미지 위에 다시 얹으면 거부해야 한다. 두 번 얹으면 훅이
    /// 자기 자신을 가리켜 무한 재귀가 되거나 예약 자리를 덮는다.
    #[test]
    fn a_probe_base_that_already_has_the_runtime_is_refused() {
        let rom = crate::test_support::release_rom();
        let (output, _) = build_dialogue_transport_probe(&rom, &rom).unwrap();
        let installed = Rom::parse(output).unwrap();

        assert!(build_dialogue_transport_probe(&rom, &installed).is_err());
    }
}
