use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{Location, MemoryAddress, Operand, Rp2A03, decode_bytes};
use typed_isa_core::StaticSemantics;

use crate::rom::Rom;

use super::{SELECTED_REGISTER_SHADOW, SOURCE_PRG_SHADOW_READER};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RawShadowOperandCandidate {
    bank: u8,
    address: u16,
    opcode: u8,
    operand_bytes: u8,
}

const fn byte_candidate(bank: u8, address: u16, opcode: u8) -> RawShadowOperandCandidate {
    RawShadowOperandCandidate {
        bank,
        address,
        opcode,
        operand_bytes: 1,
    }
}

const fn word_candidate(bank: u8, address: u16, opcode: u8) -> RawShadowOperandCandidate {
    RawShadowOperandCandidate {
        bank,
        address,
        opcode,
        operand_bytes: 2,
    }
}

const SOURCE_EXECUTABLE_SHADOW_ACCESSES: [RawShadowOperandCandidate; 2] = [
    byte_candidate(0x0F, 0xC9A8, 0x85),
    byte_candidate(0x0F, SOURCE_PRG_SHADOW_READER, 0xA5),
];

// These are all remaining raw documented memory-operand windows in the exact source. They have
// not yet been admitted as instruction starts by the global executable-image ledger. Keeping the
// complete negative-space partition here prevents a new apparent consumer from being silently
// ignored, but this raw census alone deliberately does not call them non-code.
const SOURCE_UNADMITTED_SHADOW_OPERAND_CANDIDATES: [RawShadowOperandCandidate; 53] = [
    byte_candidate(0x00, 0x98B3, 0x41),
    byte_candidate(0x01, 0x802B, 0x86),
    byte_candidate(0x01, 0xA595, 0xA6),
    byte_candidate(0x01, 0xACB2, 0x85),
    byte_candidate(0x01, 0xB292, 0x91),
    byte_candidate(0x01, 0xB2CC, 0x96),
    byte_candidate(0x01, 0xB60B, 0x84),
    byte_candidate(0x01, 0xB63B, 0x84),
    byte_candidate(0x01, 0xB7A6, 0x91),
    byte_candidate(0x01, 0xB7E1, 0x91),
    byte_candidate(0x01, 0xB81D, 0x91),
    byte_candidate(0x01, 0xB83B, 0x91),
    byte_candidate(0x02, 0x836E, 0x31),
    byte_candidate(0x02, 0x8736, 0x31),
    byte_candidate(0x02, 0x8776, 0x31),
    byte_candidate(0x02, 0x87BA, 0x31),
    byte_candidate(0x02, 0x881A, 0x31),
    byte_candidate(0x02, 0x95F2, 0xB4),
    byte_candidate(0x03, 0xAEEF, 0x35),
    byte_candidate(0x04, 0x8470, 0x86),
    byte_candidate(0x04, 0x8480, 0x86),
    byte_candidate(0x04, 0x848C, 0x86),
    byte_candidate(0x04, 0x8496, 0x86),
    byte_candidate(0x04, 0x84A4, 0x85),
    byte_candidate(0x04, 0x84B2, 0x86),
    byte_candidate(0x04, 0xA576, 0xA6),
    byte_candidate(0x04, 0xAC00, 0x35),
    word_candidate(0x05, 0xAC3B, 0xCC),
    byte_candidate(0x08, 0xB2D3, 0x35),
    byte_candidate(0x08, 0xB41D, 0x35),
    byte_candidate(0x09, 0x8A08, 0x31),
    byte_candidate(0x09, 0xACB2, 0x85),
    byte_candidate(0x09, 0xB292, 0x91),
    byte_candidate(0x09, 0xB2CC, 0x96),
    byte_candidate(0x09, 0xB60B, 0x84),
    byte_candidate(0x09, 0xB63B, 0x84),
    byte_candidate(0x09, 0xB7A6, 0x91),
    byte_candidate(0x09, 0xB7E1, 0x91),
    byte_candidate(0x09, 0xB81D, 0x91),
    byte_candidate(0x09, 0xB83B, 0x91),
    byte_candidate(0x0C, 0xA371, 0x35),
    byte_candidate(0x0C, 0xA3B6, 0x35),
    byte_candidate(0x0C, 0xA54E, 0x35),
    byte_candidate(0x0C, 0xA5E4, 0x35),
    byte_candidate(0x0E, 0x8872, 0x95),
    byte_candidate(0x0E, 0x88D4, 0x91),
    byte_candidate(0x0E, 0x96F0, 0xB5),
    byte_candidate(0x0E, 0x9851, 0xA1),
    byte_candidate(0x0E, 0x99F0, 0xA1),
    byte_candidate(0x0E, 0x9A5B, 0xA1),
    byte_candidate(0x0F, 0xE0C0, 0x35),
    byte_candidate(0x0F, 0xEF6A, 0x35),
    byte_candidate(0x0F, 0xF816, 0x01),
];

pub(super) fn bind_source_contract(source: &Rom) -> Result<()> {
    let actual = scan_raw_shadow_operand_candidates(source.prg())?;
    let expected = SOURCE_EXECUTABLE_SHADOW_ACCESSES
        .into_iter()
        .chain(SOURCE_UNADMITTED_SHADOW_OPERAND_CANDIDATES)
        .collect::<BTreeSet<_>>();
    ensure!(
        actual == expected,
        "source $51 explicit-operand census changed: expected {expected:?}, found {actual:?}"
    );
    Ok(())
}

fn scan_raw_shadow_operand_candidates(prg: &[u8]) -> Result<BTreeSet<RawShadowOperandCandidate>> {
    const BANK_LEN: usize = 0x4000;
    const FIXED_BANK: u8 = 0x0F;
    ensure!(prg.len() % BANK_LEN == 0, "PRG bank size changed");
    let mut candidates = BTreeSet::new();
    for (bank, bytes) in prg.chunks_exact(BANK_LEN).enumerate() {
        let bank = u8::try_from(bank).context("PRG bank index overflow")?;
        let base = if bank == FIXED_BANK { 0xC000 } else { 0x8000 };
        for (relative, window) in bytes.windows(2).enumerate() {
            if window[1] == SELECTED_REGISTER_SHADOW
                && documented_memory_operand(window[0], &[window[1]])
            {
                candidates.insert(byte_candidate(
                    bank,
                    base + u16::try_from(relative)?,
                    window[0],
                ));
            }
        }
        for (relative, window) in bytes.windows(3).enumerate() {
            if window[1..] == [0x51, 0x00] && documented_memory_operand(window[0], &window[1..]) {
                candidates.insert(word_candidate(
                    bank,
                    base + u16::try_from(relative)?,
                    window[0],
                ));
            }
        }
    }
    Ok(candidates)
}

/// Uses the RP2A03 profile instead of maintaining a second opcode table. Both direct operands
/// and runtime-effective operands are candidates: the latter may name `$51` as an indexed base or
/// pointer, so silently omitting them would overstate the shadow-ownership proof.
pub(super) fn documented_memory_operand(opcode: u8, operand_bytes: &[u8]) -> bool {
    let mut encoded = Vec::with_capacity(1 + operand_bytes.len());
    encoded.push(opcode);
    encoded.extend_from_slice(operand_bytes);
    let Ok(instruction) = decode_bytes(&encoded) else {
        return false;
    };
    if !instruction.opcode_is_documented() || instruction.encoded_len() != encoded.len() {
        return false;
    }
    let semantics =
        Rp2A03::semantics(&instruction, &0_u16).expect("RP2A03 static semantics are infallible");
    semantics.location_accesses.into_iter().any(|access| {
        let Location::Memory(memory) = access.location else {
            return false;
        };
        match memory {
            MemoryAddress::Direct(target) => target == u16::from(SELECTED_REGISTER_SHADOW),
            MemoryAddress::Effective { operand, .. } | MemoryAddress::Pointer { operand, .. } => {
                matches!(
                    operand,
                    Operand::Byte(target) if target == SELECTED_REGISTER_SHADOW
                ) || matches!(
                    operand,
                    Operand::Word(target) if target == u16::from(SELECTED_REGISTER_SHADOW)
                )
            }
            MemoryAddress::Stack | MemoryAddress::InterruptVector => false,
        }
    })
}
