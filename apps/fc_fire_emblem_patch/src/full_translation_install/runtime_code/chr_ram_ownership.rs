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
    mapper165::battle_composition_loader_probe::{
        CUMULATIVE_RUNTIME_LAYOUT, SHARED_BATTLE_PHASE_ADDRESS,
        cumulative_shared_battle_phase_active_bytes,
    },
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

const FIXED_BANK_SIZE: usize = 16 * 1024;
/// 전투 NMI 디스패처가 활성 플래그와 렌더 상태를 확인하고 4 KiB 합성기를 부르는
/// 구간이다. 대사 NMI는 이 구간 앞의 전투 화면 predicate와 같은 화면 집합을 먼저
/// 확인해 이 합성기와 한 프레임을 공유하지 않는다.
const BATTLE_COMPOSITION_GATE: u16 = 0xFC3E;
const BATTLE_COMPOSITION_SKIP: u16 = 0xFC4F;
pub(super) const BATTLE_COMPOSITION_CALL_SITE: u16 = 0xFC49;
const PPU_MASK_SHADOW: u8 = 0xCC;
const UPLOAD_RENDER_MASK: u8 = 0x06;
/// 누적 후보에서 CHR-RAM 뱅크 값 0을 직접 고르는 세 경로다.
const DIRECT_CHR_RAM_SELECTION_SITES: [u16; 3] = [0xFCE4, 0xFCEE, 0xFF36];

/// 전투 합성 진입과 후보의 직접 CHR-RAM 선택자 전수를 현재 누적 롬에 결속한다.
pub(super) fn bind_shared_chr_ram_ownership_boundary(candidate: &Rom) -> Result<()> {
    let expected_gate = battle_composition_gate()?;
    ensure!(
        fixed_bytes(candidate, BATTLE_COMPOSITION_GATE, expected_gate.len())? == expected_gate,
        "battle composition arbitration gate changed"
    );
    decode_rp2a03_sequence(
        &expected_gate,
        BATTLE_COMPOSITION_GATE,
        "battle composition arbitration gate",
    )?;
    let surface_predicate = cumulative_shared_battle_phase_active_bytes()?;
    ensure!(
        fixed_bytes(
            candidate,
            CUMULATIVE_RUNTIME_LAYOUT.shared_battle_phase_active,
            surface_predicate.len(),
        )? == surface_predicate,
        "battle composition surface predicate changed"
    );
    decode_rp2a03_sequence(
        &surface_predicate,
        CUMULATIVE_RUNTIME_LAYOUT.shared_battle_phase_active,
        "battle composition surface predicate",
    )?;

    let direct_chr_ram_selection = direct_chr_ram_selection()?;

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

fn battle_composition_gate() -> Result<Vec<u8>> {
    assemble_at(
        BATTLE_COMPOSITION_GATE,
        &[
            Instruction::LdaAbsolute(SHARED_BATTLE_PHASE_ADDRESS),
            Instruction::BmiAbsolute(BATTLE_COMPOSITION_SKIP),
            Instruction::LdaZeroPage(PPU_MASK_SHADOW),
            Instruction::CmpImmediate(UPLOAD_RENDER_MASK),
            Instruction::BneAbsolute(BATTLE_COMPOSITION_SKIP),
            Instruction::JsrAbsolute(CUMULATIVE_RUNTIME_LAYOUT.compose_page),
        ],
    )
}

fn direct_chr_ram_selection() -> Result<Vec<u8>> {
    assemble_at(
        0,
        &[
            Instruction::LdaImmediate(0),
            Instruction::StaAbsolute(0x8001),
        ],
    )
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

/// Rebinds the final installed image after the composition call has been
/// redirected through the dialogue-residency invalidator.  The original
/// surface/active/render gates must still dominate that call, and the target
/// must clear the pending dialogue request before tail-calling the compositor.
pub(super) fn verify_installed_ownership_gate(installed: &Rom) -> Result<()> {
    let source_gate = battle_composition_gate()?;
    let call_offset = source_gate
        .len()
        .checked_sub(3)
        .context("battle composition gate has no call instruction")?;
    let installed_gate = fixed_bytes(installed, BATTLE_COMPOSITION_GATE, source_gate.len())?;
    ensure!(
        installed_gate[..call_offset] == source_gate[..call_offset],
        "installed battle composition gate no longer dominates the ownership transfer"
    );
    ensure!(
        installed_gate[call_offset] == 0x20,
        "installed battle composition ownership transfer is not called with JSR"
    );
    let ownership_entry = u16::from_le_bytes([
        installed_gate[call_offset + 1],
        installed_gate[call_offset + 2],
    ]);
    let expected_transfer = build_battle_composition_ownership_transfer(ownership_entry)?;
    ensure!(
        fixed_bytes(installed, ownership_entry, expected_transfer.bytes.len(),)?
            == expected_transfer.bytes,
        "installed battle composition ownership transfer changed"
    );
    Ok(())
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
    fn a_changed_battle_composition_gate_refuses_installation() {
        let release = chr_ram_ownership_rom();
        let mut bytes = release.data().to_vec();
        let offset =
            crate::test_support::synthetic_fixed_bank_file_offset(BATTLE_COMPOSITION_CALL_SITE);
        bytes[offset] ^= 0x01;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_shared_chr_ram_ownership_boundary(&mutated).unwrap_err();
        assert!(error.to_string().contains("arbitration gate changed"));
    }

    #[test]
    fn a_changed_battle_surface_predicate_refuses_installation() {
        let release = chr_ram_ownership_rom();
        let mut bytes = release.data().to_vec();
        let offset = crate::test_support::synthetic_fixed_bank_file_offset(
            CUMULATIVE_RUNTIME_LAYOUT.shared_battle_phase_active,
        );
        bytes[offset] ^= 0x01;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_shared_chr_ram_ownership_boundary(&mutated).unwrap_err();
        assert!(error.to_string().contains("surface predicate changed"));
    }

    #[test]
    fn a_changed_direct_chr_ram_selection_refuses_installation() {
        let release = chr_ram_ownership_rom();
        let mut bytes = release.data().to_vec();
        let offset = crate::test_support::synthetic_fixed_bank_file_offset(
            DIRECT_CHR_RAM_SELECTION_SITES[0],
        );
        bytes[offset] ^= 0x01;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_shared_chr_ram_ownership_boundary(&mutated).unwrap_err();
        assert!(error.to_string().contains("selection inventory changed"));
    }

    #[test]
    fn final_gate_still_dominates_dialogue_residency_invalidation() {
        let installed = installed_ownership_rom();
        verify_installed_ownership_gate(&installed).unwrap();

        let mut gate_mutation = installed.data().to_vec();
        let gate_offset =
            crate::test_support::synthetic_fixed_bank_file_offset(BATTLE_COMPOSITION_GATE);
        gate_mutation[gate_offset + 4] ^= 0x01;
        let error =
            verify_installed_ownership_gate(&Rom::parse(gate_mutation).unwrap()).unwrap_err();
        assert!(error.to_string().contains("no longer dominates"));

        let mut transfer_mutation = installed.data().to_vec();
        let transfer_offset = crate::test_support::synthetic_fixed_bank_file_offset(0xF594);
        transfer_mutation[transfer_offset + 1] = 0x01;
        let error =
            verify_installed_ownership_gate(&Rom::parse(transfer_mutation).unwrap()).unwrap_err();
        assert!(error.to_string().contains("ownership transfer changed"));
    }

    fn chr_ram_ownership_rom() -> Rom {
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        let composition_gate = battle_composition_gate().unwrap();
        let composition_offset =
            crate::test_support::synthetic_fixed_bank_file_offset(BATTLE_COMPOSITION_GATE);
        bytes[composition_offset..composition_offset + composition_gate.len()]
            .copy_from_slice(&composition_gate);
        let surface_predicate = cumulative_shared_battle_phase_active_bytes().unwrap();
        let surface_offset = crate::test_support::synthetic_fixed_bank_file_offset(
            CUMULATIVE_RUNTIME_LAYOUT.shared_battle_phase_active,
        );
        bytes[surface_offset..surface_offset + surface_predicate.len()]
            .copy_from_slice(&surface_predicate);

        let direct_selection = direct_chr_ram_selection().unwrap();
        for address in DIRECT_CHR_RAM_SELECTION_SITES {
            let offset = crate::test_support::synthetic_fixed_bank_file_offset(address);
            bytes[offset..offset + direct_selection.len()].copy_from_slice(&direct_selection);
        }
        Rom::parse(bytes).expect("CHR-RAM ownership fixture parses")
    }

    fn installed_ownership_rom() -> Rom {
        let candidate = chr_ram_ownership_rom();
        let mut bytes = candidate.data().to_vec();
        let gate_offset =
            crate::test_support::synthetic_fixed_bank_file_offset(BATTLE_COMPOSITION_CALL_SITE);
        bytes[gate_offset..gate_offset + 3].copy_from_slice(&ownership_transfer_hook_bytes(0xF594));
        let transfer = build_battle_composition_ownership_transfer(0xF594).unwrap();
        let transfer_offset =
            crate::test_support::synthetic_fixed_bank_file_offset(transfer.address);
        bytes[transfer_offset..transfer_offset + transfer.bytes.len()]
            .copy_from_slice(&transfer.bytes);
        Rom::parse(bytes).expect("installed ownership fixture parses")
    }
}
