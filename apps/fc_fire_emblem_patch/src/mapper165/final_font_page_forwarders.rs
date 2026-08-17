//! 중앙 화면 거주 정책으로 이관할 수 있는 누적 글꼴 선택기의 설치 상태를 묶는다.
//!
//! 누적 패치는 독립 실행 단계였으므로 화면마다 자체 조건문을 가진다. 최종 통합
//! 런타임이 같은 화면을 먼저 판정하게 된 뒤에도 그 조건문을 fallback으로 남겨 두면
//! 두 소유자가 서로 다른 상태 집합을 읽게 된다. 이 모듈은 최종 계획이 판단을 제거할
//! 수 있도록, 현재 선택기 전체와 그 선택기로 들어오는 직접 경로를 정확히 제공한다.

use anyhow::{Context, Result, ensure};

use crate::{
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

use super::{
    MAXIMUM_CHR_PAGE_COUNT,
    cumulative_patch::DIALOGUE_SELECTOR_ADDRESS,
    front_end_page::{
        PAGE_ROUTINE_ADDRESS as FRONT_END_SELECTOR_ADDRESS,
        PAGE_ROUTINE_END as FRONT_END_SELECTOR_END, build_page_selector,
    },
    shop_dialogue_page::{
        PAGE_ROUTINE_ADDRESS as SHOP_SELECTOR_ADDRESS, PAGE_ROUTINE_END as SHOP_SELECTOR_END,
        build_page_selector as build_shop_selector,
    },
};

const FIXED_BANK_BYTE_COUNT: usize = 16 * 1024;
const JMP_ABSOLUTE: u8 = 0x4C;
const SHOP_FALLBACK_JUMP_ADDRESS: u16 = SHOP_SELECTOR_END - 3;

#[derive(Debug)]
pub(crate) struct BoundFrontEndFontPageSelector {
    pub(crate) cpu_address: u16,
    pub(crate) cpu_end_exclusive: u16,
    pub(crate) fallback_target: u16,
    pub(crate) mapper_register: u8,
    pub(crate) direct_predecessor_address: u16,
    pub(crate) expected_bytes: Vec<u8>,
}

/// 누적 프런트엔드 선택기와 그 유일한 활성 고정 뱅크 진입을 묶는다.
///
/// 물리 PRG의 예전 고정 뱅크 사본이나 CHR 자료에서 우연히 보이는 피연산자는 실행
/// 소유권이 아니다. 최종 CPU `$C000..$FFFF` 창에 실제로 고정되는 마지막 16 KiB만
/// 센서스하며, 그 안에서는 무분류 직접 진입을 하나도 허용하지 않는다.
pub(crate) fn bind_front_end_font_page_selector(
    candidate: &Rom,
) -> Result<BoundFrontEndFontPageSelector> {
    ensure!(
        candidate.mapper() == 165,
        "front-end selector candidate is not mapper 165"
    );
    let fixed = active_fixed_bank(candidate)?;
    let selector = fixed_slice(
        fixed,
        FRONT_END_SELECTOR_ADDRESS,
        usize::from(FRONT_END_SELECTOR_END - FRONT_END_SELECTOR_ADDRESS),
    )?;
    let mapper_register = bind_generated_selector_register(selector, |register| {
        build_page_selector(register, DIALOGUE_SELECTOR_ADDRESS)
    })?;
    ensure!(
        usize::from(mapper_register / 4)
            < candidate.chr().len() / crate::font_slots::FONT_PAGE_SIZE,
        "front-end selector names a CHR page outside the candidate"
    );
    decode_rp2a03_sequence(
        selector,
        FRONT_END_SELECTOR_ADDRESS,
        "installed cumulative front-end font-page selector",
    )?;

    let shop_selector = fixed_slice(
        fixed,
        SHOP_SELECTOR_ADDRESS,
        usize::from(SHOP_SELECTOR_END - SHOP_SELECTOR_ADDRESS),
    )?;
    bind_generated_selector_register(shop_selector, |register| {
        build_shop_selector(register, FRONT_END_SELECTOR_ADDRESS)
    })?;
    decode_rp2a03_sequence(
        shop_selector,
        SHOP_SELECTOR_ADDRESS,
        "installed cumulative weapon-shop font-page selector",
    )?;

    let direct_transfers =
        direct_transfer_sites(fixed, FRONT_END_SELECTOR_ADDRESS, FRONT_END_SELECTOR_END);
    ensure!(
        direct_transfers
            == vec![(
                SHOP_FALLBACK_JUMP_ADDRESS,
                JMP_ABSOLUTE,
                FRONT_END_SELECTOR_ADDRESS,
            )],
        "front-end font-page selector direct-entry census changed: {direct_transfers:?}"
    );

    Ok(BoundFrontEndFontPageSelector {
        cpu_address: FRONT_END_SELECTOR_ADDRESS,
        cpu_end_exclusive: FRONT_END_SELECTOR_END,
        fallback_target: DIALOGUE_SELECTOR_ADDRESS,
        mapper_register,
        direct_predecessor_address: SHOP_FALLBACK_JUMP_ADDRESS,
        expected_bytes: selector.to_vec(),
    })
}

pub(crate) fn build_front_end_font_page_forwarder(
    selector: &BoundFrontEndFontPageSelector,
) -> Result<Vec<u8>> {
    let mut bytes = assemble_at(
        selector.cpu_address,
        &[Instruction::JmpAbsolute(selector.fallback_target)],
    )?;
    let capacity = usize::from(selector.cpu_end_exclusive - selector.cpu_address);
    ensure!(
        selector.expected_bytes.len() == capacity && bytes.len() <= capacity,
        "front-end selector forwarder does not own the complete source selector span"
    );
    bytes.resize(capacity, 0xEA);
    decode_rp2a03_sequence(
        &bytes,
        selector.cpu_address,
        "central-policy front-end font-page fallback forwarder",
    )?;
    ensure!(
        bytes != selector.expected_bytes,
        "front-end selector was already replaced before final integration"
    );
    Ok(bytes)
}

fn bind_generated_selector_register(
    actual: &[u8],
    build: impl Fn(u8) -> Result<Vec<u8>>,
) -> Result<u8> {
    let matching = (1_u8..MAXIMUM_CHR_PAGE_COUNT)
        .filter_map(|physical_page| {
            let register = physical_page.checked_mul(4)?;
            (build(register).ok()?.as_slice() == actual).then_some(register)
        })
        .collect::<Vec<_>>();
    ensure!(
        matching.len() == 1,
        "installed selector does not identify exactly one generated CHR page: {matching:?}"
    );
    Ok(matching[0])
}

fn active_fixed_bank(candidate: &Rom) -> Result<&[u8]> {
    candidate
        .prg()
        .get(candidate.prg().len().saturating_sub(FIXED_BANK_BYTE_COUNT)..)
        .context("mapper-165 candidate has no active fixed PRG bank")
}

fn fixed_slice(fixed: &[u8], address: u16, len: usize) -> Result<&[u8]> {
    ensure!(address >= 0xC000, "fixed selector address is below $C000");
    fixed
        .get(usize::from(address - 0xC000)..usize::from(address - 0xC000) + len)
        .context("fixed selector range is outside the active bank")
}

fn direct_transfer_sites(fixed: &[u8], start: u16, end: u16) -> Vec<(u16, u8, u16)> {
    fixed
        .windows(3)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            let opcode = bytes[0];
            if !matches!(opcode, 0x20 | JMP_ABSOLUTE) {
                return None;
            }
            let target = u16::from_le_bytes([bytes[1], bytes[2]]);
            (start..end).contains(&target).then_some((
                0xC000 + u16::try_from(offset).expect("16 KiB offset fits u16"),
                opcode,
                target,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed_candidate() -> Rom {
        const SYNTHETIC_CHR_BANK_COUNT: u8 = 32;
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        bytes[5] = SYNTHETIC_CHR_BANK_COUNT;
        bytes.resize(
            bytes.len() + usize::from(SYNTHETIC_CHR_BANK_COUNT) * 8 * 1024,
            0,
        );
        let front_end = build_page_selector(0xA8, DIALOGUE_SELECTOR_ADDRESS).unwrap();
        let front_end_offset =
            crate::test_support::synthetic_fixed_bank_file_offset(FRONT_END_SELECTOR_ADDRESS);
        bytes[front_end_offset..front_end_offset + front_end.len()].copy_from_slice(&front_end);
        let shop = build_shop_selector(0xC0, FRONT_END_SELECTOR_ADDRESS).unwrap();
        let shop_offset =
            crate::test_support::synthetic_fixed_bank_file_offset(SHOP_SELECTOR_ADDRESS);
        bytes[shop_offset..shop_offset + shop.len()].copy_from_slice(&shop);
        Rom::parse(bytes).unwrap()
    }

    #[test]
    fn binds_the_complete_front_end_selector_and_its_only_direct_predecessor() {
        let binding = bind_front_end_font_page_selector(&installed_candidate()).unwrap();

        assert_eq!(binding.cpu_address, FRONT_END_SELECTOR_ADDRESS);
        assert_eq!(binding.cpu_end_exclusive, FRONT_END_SELECTOR_END);
        assert_eq!(binding.fallback_target, DIALOGUE_SELECTOR_ADDRESS);
        assert_eq!(binding.mapper_register, 0xA8);
        assert_eq!(
            binding.direct_predecessor_address,
            SHOP_FALLBACK_JUMP_ADDRESS
        );
    }

    #[test]
    fn rejects_an_unclassified_direct_entry_into_the_selector() {
        let mut bytes = installed_candidate().data().to_vec();
        let extra = crate::test_support::synthetic_fixed_bank_file_offset(0xC100);
        bytes[extra..extra + 3].copy_from_slice(&[
            JMP_ABSOLUTE,
            (FRONT_END_SELECTOR_ADDRESS + 1) as u8,
            ((FRONT_END_SELECTOR_ADDRESS + 1) >> 8) as u8,
        ]);
        let error = bind_front_end_font_page_selector(&Rom::parse(bytes).unwrap()).unwrap_err();

        assert!(error.to_string().contains("direct-entry census changed"));
    }

    #[test]
    fn rejects_selector_bytes_that_no_longer_match_the_generated_structure() {
        let mut bytes = installed_candidate().data().to_vec();
        let offset =
            crate::test_support::synthetic_fixed_bank_file_offset(FRONT_END_SELECTOR_ADDRESS);
        bytes[offset + 4] ^= 1;
        let error = bind_front_end_font_page_selector(&Rom::parse(bytes).unwrap()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not identify exactly one generated CHR page")
        );
    }

    #[test]
    fn forwarder_replaces_the_complete_selector_with_one_unconditional_route() {
        let selector = bind_front_end_font_page_selector(&installed_candidate()).unwrap();
        let forwarder = build_front_end_font_page_forwarder(&selector).unwrap();

        assert_eq!(&forwarder[..3], &[0x4C, 0xD4, 0xFB]);
        assert!(forwarder[3..].iter().all(|byte| *byte == 0xEA));
        assert_eq!(forwarder.len(), selector.expected_bytes.len());
        assert!(
            !forwarder
                .windows(3)
                .any(|bytes| bytes == [0x8D, 0x01, 0x80])
        );
    }
}
