//! 지원 원본의 실행 구간이 MMC4 레지스터를 직접 바꾸지 않음을 결속한다.
//!
//! 화면 생명주기마다 opcode 목록과 `$A000` 경계를 다시 구현하면 alias나 indexed
//! write 처리가 쉽게 어긋난다. 이 모듈은 typed ISA의 write semantics와 실제 MMC4
//! decode를 한 곳에서 적용한다. 간접 목적지는 이 좁은 결속기가 증명할 수 없으므로
//! fail-closed로 거부한다.

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Location, MemoryAddress, Operand, Rp2A03, decode_bytes};
use typed_isa_core::{AccessKind, StaticSemantics};

use crate::{rom::Rom, sha1_hex, typed_source::decode_rp2a03_sequence};

use super::executable_mapper_writes::decode_source_mmc4_write;

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const SWITCHABLE_CPU_END: u16 = 0xC000;
const LAST_SOURCE_PRG_BANK: u8 = 0x0F;

pub(crate) fn bind_exact_switchable_code_without_mmc4_writes(
    source: &Rom,
    prg_bank: u8,
    address: u16,
    expected: &[u8],
    role: &str,
) -> Result<()> {
    ensure!(!expected.is_empty(), "{role} source code is empty");
    let actual = switchable_source_bytes(source, prg_bank, address, expected.len())?;
    ensure!(actual == expected, "{role} source bytes changed");
    validate_code_without_mmc4_writes(actual, address, role)
}

pub(crate) fn bind_hashed_switchable_code_without_mmc4_writes(
    source: &Rom,
    prg_bank: u8,
    start: u16,
    end_exclusive: u16,
    expected_sha1: &str,
    role: &str,
) -> Result<()> {
    ensure!(
        start < end_exclusive,
        "{role} source code range is empty or reversed"
    );
    let byte_count = usize::from(end_exclusive - start);
    let actual = switchable_source_bytes(source, prg_bank, start, byte_count)?;
    ensure!(
        sha1_hex(actual) == expected_sha1,
        "{role} source bytes changed"
    );
    validate_code_without_mmc4_writes(actual, start, role)
}

fn validate_code_without_mmc4_writes(actual: &[u8], address: u16, role: &str) -> Result<()> {
    decode_rp2a03_sequence(actual, address, role)?;

    let mut offset = 0_usize;
    while offset < actual.len() {
        let instruction = decode_bytes(&actual[offset..])
            .with_context(|| format!("decode {role} at +0x{offset:X}"))?;
        let instruction_address = address
            .checked_add(u16::try_from(offset)?)
            .context("source instruction address overflow")?;
        for access in Rp2A03::semantics(&instruction, &instruction_address)
            .expect("RP2A03 static semantics are infallible")
            .location_accesses
        {
            if access.kind != AccessKind::Write {
                continue;
            }
            match access.location {
                Location::Memory(MemoryAddress::Direct(target)) => ensure!(
                    decode_source_mmc4_write(target).is_none(),
                    "{role} gained a direct source MMC4 write at ${instruction_address:04X}"
                ),
                Location::Memory(MemoryAddress::Effective {
                    mode: AddressingMode::AbsoluteX | AddressingMode::AbsoluteY,
                    operand: Operand::Word(base),
                }) => ensure!(
                    (0_u16..=0xFF)
                        .map(|index| base.wrapping_add(index))
                        .all(|target| decode_source_mmc4_write(target).is_none()),
                    "{role} gained an indexed source MMC4-write envelope at ${instruction_address:04X}"
                ),
                Location::Memory(MemoryAddress::Effective {
                    mode: AddressingMode::ZeroPageX | AddressingMode::ZeroPageY,
                    operand: Operand::Byte(_),
                }) => {}
                Location::Memory(MemoryAddress::Effective { .. }) => anyhow::bail!(
                    "{role} gained an unbounded effective write at ${instruction_address:04X}"
                ),
                _ => {}
            }
        }
        offset += instruction.encoded_len();
    }
    ensure!(
        offset == actual.len(),
        "{role} typed decode ended mid-region"
    );
    Ok(())
}

fn switchable_source_bytes(
    source: &Rom,
    prg_bank: u8,
    address: u16,
    byte_count: usize,
) -> Result<&[u8]> {
    ensure!(
        prg_bank <= LAST_SOURCE_PRG_BANK,
        "source PRG bank is outside the supported image"
    );
    let end = address
        .checked_add(u16::try_from(byte_count)?)
        .context("source code range address overflow")?;
    ensure!(
        (SWITCHABLE_CPU_START..SWITCHABLE_CPU_END).contains(&address) && end <= SWITCHABLE_CPU_END,
        "source code range leaves its switchable PRG window"
    );
    let start = usize::from(prg_bank)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - SWITCHABLE_CPU_START)))
        .context("source code PRG offset overflow")?;
    source
        .prg()
        .get(start..start + byte_count)
        .context("source code range exceeds PRG")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_and_indexed_mmc4_writes_fail_closed() {
        let direct = validate_code_without_mmc4_writes(&[0x8D, 0x01, 0xB0], 0x9000, "direct alias")
            .unwrap_err();
        assert!(direct.to_string().contains("direct source MMC4 write"));

        let indexed =
            validate_code_without_mmc4_writes(&[0x9D, 0x80, 0x9F], 0x9000, "indexed envelope")
                .unwrap_err();
        assert!(indexed.to_string().contains("indexed source MMC4-write"));
    }

    #[test]
    fn indirect_write_is_not_misreported_as_safe() {
        let error =
            validate_code_without_mmc4_writes(&[0x91, 0x10], 0x9000, "indirect destination")
                .unwrap_err();

        assert!(error.to_string().contains("unbounded effective write"));
    }

    #[test]
    fn ordinary_ram_writes_are_admitted() {
        validate_code_without_mmc4_writes(
            &[0x85, 0x10, 0x9D, 0x00, 0x70, 0x60],
            0x9000,
            "bounded RAM writes",
        )
        .unwrap();
    }
}
