//! CHR-RAM을 공유하는 전투와 주 대사 런타임의 소유권 경계다.
//!
//! E7 외부 인계 자체는 CHR-RAM 쓰기가 아니다. 그때 대사 상주권을 버리면 같은
//! 페이지 그룹도 화면 위에서 다시 만든다. 반대로 외부 상태기 안에서 전투 합성기가
//! 실제로 페이지를 덮으면 이전 대사 페이지를 재사용할 수 없다. 따라서 전투 합성
//! 진입만 대사 `ready`를 무효화하고, E7은 selector만 원본 경로로 물러난다.

use anyhow::{Context, Result, ensure};

use super::super::runtime_state_storage::REQUEST_STATE;
use super::{RuntimeRoutine, next_address};
use crate::{
    mapper165::battle_composition_loader_probe::CUMULATIVE_RUNTIME_LAYOUT,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

const FIXED_BANK_SIZE: usize = 16 * 1024;
/// 전투 NMI 디스패처가 실제 4 KiB 합성기를 부르는 자리다.
pub(super) const BATTLE_COMPOSITION_CALL_SITE: u16 = 0xFC49;
/// 누적 후보에서 CHR-RAM 뱅크 값 0을 직접 고르는 세 경로다.
const DIRECT_CHR_RAM_SELECTION_SITES: [u16; 3] = [0xFCE4, 0xFCEE, 0xFF36];

/// 전투 합성 진입과 후보의 직접 CHR-RAM 선택자 전수를 현재 누적 롬에 결속한다.
pub(super) fn bind_shared_chr_ram_ownership_boundary(candidate: &Rom) -> Result<()> {
    let composition_entry = CUMULATIVE_RUNTIME_LAYOUT.compose_page;
    let expected_call = assemble_at(
        BATTLE_COMPOSITION_CALL_SITE,
        &[Instruction::JsrAbsolute(composition_entry)],
    )?;
    ensure!(
        fixed_bytes(candidate, BATTLE_COMPOSITION_CALL_SITE, expected_call.len())? == expected_call,
        "battle composition call site changed"
    );
    decode_rp2a03_sequence(
        &expected_call,
        BATTLE_COMPOSITION_CALL_SITE,
        "battle composition call",
    )?;

    let direct_chr_ram_selection = assemble_at(
        0,
        &[
            Instruction::LdaImmediate(0),
            Instruction::StaAbsolute(0x8001),
        ],
    )?;

    let fixed_start = candidate
        .prg()
        .len()
        .checked_sub(FIXED_BANK_SIZE)
        .context("candidate PRG is smaller than one fixed bank")?;
    let actual_sites = candidate
        .prg()
        .windows(direct_chr_ram_selection.len())
        .enumerate()
        .filter_map(|(offset, bytes)| {
            (bytes == direct_chr_ram_selection).then(|| {
                offset
                    .checked_sub(fixed_start)
                    .and_then(|relative| u16::try_from(relative).ok())
                    .and_then(|relative| 0xC000_u16.checked_add(relative))
            })
        })
        .collect::<Option<Vec<_>>>();
    ensure!(
        actual_sites.as_deref() == Some(DIRECT_CHR_RAM_SELECTION_SITES.as_slice()),
        "candidate direct CHR-RAM selection inventory changed: expected {:?}, found {:?}",
        DIRECT_CHR_RAM_SELECTION_SITES,
        actual_sites
    );
    Ok(())
}

/// 대사 상주권을 내린 뒤 원래 전투 합성기로 꼬리 호출한다.
pub(super) fn build_battle_composition_ownership_transfer(origin: u16) -> Result<RuntimeRoutine> {
    let composition_entry = CUMULATIVE_RUNTIME_LAYOUT.compose_page;
    let instructions = [
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(REQUEST_STATE),
        Instruction::JmpAbsolute(composition_entry),
    ];
    let bytes = assemble_at(origin, &instructions)
        .context("cannot assemble battle CHR-RAM ownership transfer")?;
    ensure!(
        next_address(origin, &instructions)? == origin + 8,
        "battle CHR-RAM ownership transfer changed length"
    );
    Ok(RuntimeRoutine {
        role: "battle-to-dialogue CHR RAM ownership transfer",
        address: origin,
        bytes,
    })
}

pub(super) fn ownership_transfer_hook_bytes(entry: u16) -> [u8; 3] {
    [0x20, entry as u8, (entry >> 8) as u8]
}

fn fixed_bytes(rom: &Rom, address: u16, length: usize) -> Result<&[u8]> {
    ensure!(
        address >= 0xC000,
        "fixed CHR ownership address is below C000"
    );
    let fixed_start = rom
        .prg()
        .len()
        .checked_sub(FIXED_BANK_SIZE)
        .context("candidate PRG is smaller than one fixed bank")?;
    let offset = fixed_start + usize::from(address - 0xC000);
    rom.prg()
        .get(offset..offset + length)
        .context("fixed CHR ownership range is outside candidate")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_candidate_binds_the_complete_direct_chr_ram_selection_inventory() {
        bind_shared_chr_ram_ownership_boundary(&crate::test_support::release_rom()).unwrap();
    }

    #[test]
    fn ownership_transfer_invalidates_dialogue_before_tail_calling_battle() {
        let routine = build_battle_composition_ownership_transfer(0xF594).unwrap();

        assert_eq!(
            routine.bytes,
            [
                0xA9,
                0x00,
                0x8D,
                REQUEST_STATE as u8,
                (REQUEST_STATE >> 8) as u8,
                0x4C,
                CUMULATIVE_RUNTIME_LAYOUT.compose_page as u8,
                (CUMULATIVE_RUNTIME_LAYOUT.compose_page >> 8) as u8,
            ]
        );
    }

    #[test]
    fn a_changed_battle_composition_call_refuses_installation() {
        let release = crate::test_support::release_rom();
        let fixed_start = release.prg().len() - FIXED_BANK_SIZE;
        let offset = fixed_start + usize::from(BATTLE_COMPOSITION_CALL_SITE - 0xC000);
        let mut bytes = release.data().to_vec();
        bytes[crate::rom::HEADER_SIZE + offset] ^= 0x01;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_shared_chr_ram_ownership_boundary(&mutated).unwrap_err();
        assert!(error.to_string().contains("composition call site changed"));
    }

    #[test]
    fn a_changed_direct_chr_ram_selection_refuses_installation() {
        let release = crate::test_support::release_rom();
        let fixed_start = release.prg().len() - FIXED_BANK_SIZE;
        let offset = fixed_start + usize::from(DIRECT_CHR_RAM_SELECTION_SITES[0] - 0xC000);
        let mut bytes = release.data().to_vec();
        bytes[crate::rom::HEADER_SIZE + offset] ^= 0x01;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_shared_chr_ram_ownership_boundary(&mutated).unwrap_err();
        assert!(error.to_string().contains("selection inventory changed"));
    }
}
