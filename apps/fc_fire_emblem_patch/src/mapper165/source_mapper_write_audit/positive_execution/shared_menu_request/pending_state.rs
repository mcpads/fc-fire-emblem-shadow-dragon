use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{
    AddressingMode, Location, MemoryAddress, Mnemonic, Operand, Rp2A03, decode_bytes,
};
use typed_isa_core::{AccessKind, StaticSemantics};

use crate::{
    rom::Rom, shop_flow::SharedMenuControllerSource, typed_source::decode_rp2a03_sequence,
};

use super::super::control_state::PENDING_SHARED_MENU_REQUEST_STATE;
use super::source_regions::source_bytes;

const FIXED_PRG_BANK: u8 = 0x0F;
const PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const MENU_INITIALIZER_RETURN_SUFFIX: [u8; 13] = [
    0xA2, 0x00, 0xA0, 0x00, 0x8C, 0x50, 0x04, 0x84, 0x7A, 0x8C, 0xEB, 0x05, 0x60,
];
const ZERO_RETURN_CALL_SEQUENCE: [u8; 8] = [0x20, 0x3C, 0x8E, 0x86, 0x10, 0x8E, 0xCC, 0x05];

#[derive(Clone, Copy)]
struct ConstantStoreSpec {
    bank: u8,
    sequence_start: u16,
    writer: u16,
    value: u8,
    expected: &'static [u8],
    role: &'static str,
}

const CONSTANT_STORES: [ConstantStoreSpec; 16] = [
    adjacent_store(0x02, 0xA6A0, 0x05, "publish a five-state map request"),
    adjacent_store(0x02, 0xA74A, 0x00, "clear a completed map request"),
    adjacent_store(0x02, 0xA94C, 0x00, "clear a cancelled map request"),
    adjacent_store(0x06, 0xB70F, 0x05, "publish a five-state save request"),
    adjacent_store(0x0B, 0x8E3C, 0x01, "initialize a shared-menu request"),
    adjacent_store(
        0x0B,
        0x926C,
        0x00,
        "finish an exhausted shared-menu request",
    ),
    adjacent_store(0x0B, 0x92B9, 0x05, "advance state two to state five"),
    adjacent_store(0x0B, 0x92C0, 0x00, "cancel state two"),
    adjacent_store(0x0B, 0x9315, 0x05, "advance state four to state five"),
    ConstantStoreSpec {
        bank: 0x0B,
        sequence_start: 0x932A,
        writer: 0x932F,
        value: 0x00,
        expected: &[0xA9, 0x00, 0x8D, 0xD4, 0x05, 0x8D, 0xCC, 0x05],
        role: "finish state four",
    },
    adjacent_store(0x0B, 0x93A5, 0x00, "finish state five"),
    ConstantStoreSpec {
        bank: 0x0D,
        sequence_start: 0xB009,
        writer: 0xB014,
        value: 0x00,
        expected: &[
            0xA9, 0x00, 0x85, 0x23, 0x85, 0x84, 0x85, 0x26, 0x8D, 0xDB, 0x05, 0x8D, 0xCC, 0x05,
        ],
        role: "clear pending state during title initialization",
    },
    adjacent_store(
        0x0F,
        0xA67E,
        0x03,
        "bank-fifteen lower-window copy of shared-menu state three",
    ),
    ConstantStoreSpec {
        bank: 0x0F,
        sequence_start: 0xB302,
        writer: 0xB30F,
        value: 0x00,
        expected: &[
            0xA9, 0x00, 0x85, 0x23, 0x85, 0x24, 0x85, 0x84, 0x85, 0x26, 0x8D, 0xDB, 0x05, 0x8D,
            0xCC, 0x05,
        ],
        role: "bank-fifteen lower-window copy of gameplay initialization",
    },
    adjacent_store(0x0F, 0xE67E, 0x03, "publish shared-menu state three"),
    ConstantStoreSpec {
        bank: FIXED_PRG_BANK,
        sequence_start: 0xF302,
        writer: 0xF30F,
        value: 0x00,
        expected: &[
            0xA9, 0x00, 0x85, 0x23, 0x85, 0x24, 0x85, 0x84, 0x85, 0x26, 0x8D, 0xDB, 0x05, 0x8D,
            0xCC, 0x05,
        ],
        role: "clear pending state during gameplay initialization",
    },
];

const fn adjacent_store(
    bank: u8,
    sequence_start: u16,
    value: u8,
    role: &'static str,
) -> ConstantStoreSpec {
    ConstantStoreSpec {
        bank,
        sequence_start,
        writer: sequence_start + 2,
        value,
        expected: &[],
        role,
    }
}

const ZERO_RETURN_WRITERS: [(u8, u16, u16); 2] = [(0x0B, 0x80FE, 0x8103), (0x0B, 0x8B3A, 0x8B3F)];
const INCREMENT_WRITERS: [(u8, u16, u8, u8); 2] =
    [(0x0B, 0x929E, 0x01, 0x02), (0x0B, 0x92F7, 0x03, 0x04)];
const READ_ONLY_SITES: [(u8, u16); 7] = [
    (0x06, 0x9CCB),
    (0x06, 0xA0CA),
    (0x06, 0xB6F3),
    (0x0F, 0xA65C),
    (0x0F, 0xA667),
    (FIXED_PRG_BANK, 0xE65C),
    (FIXED_PRG_BANK, 0xE667),
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct DirectAccessKinds {
    reads: bool,
    writes: bool,
}

pub(super) struct PendingRequestStateSource {
    active_states: BTreeSet<u8>,
    coherent_dispatch_memory_addresses: BTreeSet<u16>,
}

impl PendingRequestStateSource {
    pub(super) fn active_states(&self) -> &BTreeSet<u8> {
        &self.active_states
    }

    pub(super) fn coherent_dispatch_memory_addresses(&self) -> &BTreeSet<u16> {
        &self.coherent_dispatch_memory_addresses
    }
}

pub(super) fn bind_pending_request_state(
    source: &Rom,
    shared_menu: &SharedMenuControllerSource,
) -> Result<PendingRequestStateSource> {
    let actual_accesses = scan_direct_accesses(source)?;
    let expected_accesses = expected_direct_accesses();
    ensure!(
        actual_accesses == expected_accesses,
        "pending shared-menu state $05CC direct-access census changed: expected {expected_accesses:02X?}, found {actual_accesses:02X?}"
    );

    for spec in &CONSTANT_STORES {
        bind_constant_store(source, *spec)?;
    }
    ensure!(
        source_bytes(source, 0x0B, 0x8E5E, MENU_INITIALIZER_RETURN_SUFFIX.len())?
            == MENU_INITIALIZER_RETURN_SUFFIX,
        "shared-menu initializer no longer returns X=0"
    );
    decode_rp2a03_sequence(
        &MENU_INITIALIZER_RETURN_SUFFIX,
        0x8E5E,
        "shared-menu initializer X-zero return suffix",
    )?;
    for &(bank, sequence_start, writer) in &ZERO_RETURN_WRITERS {
        ensure!(
            source_bytes(
                source,
                bank,
                sequence_start,
                ZERO_RETURN_CALL_SEQUENCE.len()
            )? == ZERO_RETURN_CALL_SEQUENCE,
            "shared-menu X-zero request clear at {bank:02X}:${writer:04X} changed"
        );
        decode_rp2a03_sequence(
            &ZERO_RETURN_CALL_SEQUENCE,
            sequence_start,
            "clear pending request after shared-menu initializer returns X=0",
        )?;
    }

    ensure!(
        shared_menu.handler_target(0x01) == Some(0x9265)
            && shared_menu.handler_target(0x03) == Some(0x92C9),
        "shared-menu increment handlers no longer own request states one and three"
    );
    for &(bank, writer, input, output) in &INCREMENT_WRITERS {
        ensure!(
            source_bytes(source, bank, writer, 3)? == [0xEE, 0xCC, 0x05],
            "shared-menu request transition {input:02X}->{output:02X} changed at {bank:02X}:${writer:04X}"
        );
        ensure!(
            input.wrapping_add(1) == output,
            "shared-menu request transition is not one increment"
        );
    }

    let produced_states = CONSTANT_STORES
        .iter()
        .map(|spec| spec.value)
        .chain([0x00])
        .chain(INCREMENT_WRITERS.iter().map(|(_, _, _, output)| *output))
        .collect::<BTreeSet<_>>();
    ensure!(
        produced_states == (0..=5).collect(),
        "pending shared-menu state producer domain changed: {produced_states:02X?}"
    );
    let active_states = produced_states
        .iter()
        .copied()
        .filter(|state| *state != 0)
        .collect::<BTreeSet<_>>();
    Ok(PendingRequestStateSource {
        active_states,
        coherent_dispatch_memory_addresses: BTreeSet::from([
            PENDING_SHARED_MENU_REQUEST_STATE,
            shared_menu.state_address(),
        ]),
    })
}

fn bind_constant_store(source: &Rom, spec: ConstantStoreSpec) -> Result<()> {
    let adjacent = [0xA9, spec.value, 0x8D, 0xCC, 0x05];
    let expected = if spec.expected.is_empty() {
        &adjacent
    } else {
        spec.expected
    };
    let actual = source_bytes(source, spec.bank, spec.sequence_start, expected.len())?;
    ensure!(actual == expected, "{} changed", spec.role);
    decode_rp2a03_sequence(actual, spec.sequence_start, spec.role)?;

    let mut address = spec.sequence_start;
    let mut offset = 0_usize;
    let mut accumulator = None;
    let mut observed_writer = None;
    while offset < actual.len() {
        let instruction = decode_bytes(&actual[offset..])
            .with_context(|| format!("decode {} at +0x{offset:X}", spec.role))?;
        match (
            instruction.mnemonic(),
            instruction.addressing_mode(),
            instruction.operand(),
        ) {
            (Mnemonic::Lda, AddressingMode::Immediate, Operand::Byte(value)) => {
                accumulator = Some(value);
            }
            (Mnemonic::Sta, AddressingMode::ZeroPage | AddressingMode::Absolute, _) => {}
            _ => anyhow::bail!("{} contains an unmodeled accumulator effect", spec.role),
        }
        if address == spec.writer {
            ensure!(
                instruction.mnemonic() == Mnemonic::Sta
                    && instruction.addressing_mode() == AddressingMode::Absolute
                    && instruction.operand() == Operand::Word(PENDING_SHARED_MENU_REQUEST_STATE)
                    && accumulator == Some(spec.value),
                "{} no longer writes its bound request state",
                spec.role,
            );
            observed_writer = Some(address);
        }
        offset += instruction.encoded_len();
        address = address
            .checked_add(
                u16::try_from(instruction.encoded_len())
                    .context("request writer length overflow")?,
            )
            .context("request writer address overflow")?;
    }
    ensure!(
        observed_writer == Some(spec.writer),
        "{} lost its request-state writer",
        spec.role
    );
    Ok(())
}

fn expected_direct_accesses() -> BTreeMap<(u8, u16), DirectAccessKinds> {
    let mut expected = BTreeMap::new();
    for spec in &CONSTANT_STORES {
        expected
            .entry((spec.bank, spec.writer))
            .or_insert_with(DirectAccessKinds::default)
            .writes = true;
    }
    for &(bank, _, writer) in &ZERO_RETURN_WRITERS {
        expected
            .entry((bank, writer))
            .or_insert_with(DirectAccessKinds::default)
            .writes = true;
    }
    for &(bank, writer, _, _) in &INCREMENT_WRITERS {
        let access = expected
            .entry((bank, writer))
            .or_insert_with(DirectAccessKinds::default);
        access.reads = true;
        access.writes = true;
    }
    for site in READ_ONLY_SITES {
        expected
            .entry(site)
            .or_insert_with(DirectAccessKinds::default)
            .reads = true;
    }
    expected
}

fn scan_direct_accesses(source: &Rom) -> Result<BTreeMap<(u8, u16), DirectAccessKinds>> {
    let mut accesses = BTreeMap::new();
    let fixed = source
        .prg()
        .get(15 * PRG_BANK_BYTE_COUNT..16 * PRG_BANK_BYTE_COUNT)
        .context("fixed source PRG bank is missing")?;
    for bank in 0_u8..=FIXED_PRG_BANK {
        let start = usize::from(bank) * PRG_BANK_BYTE_COUNT;
        let bytes = source
            .prg()
            .get(start..start + PRG_BANK_BYTE_COUNT)
            .context("switchable source PRG bank is missing")?;
        for (offset, window) in bytes.windows(3).enumerate() {
            collect_direct_access(&mut accesses, bank, 0x8000 + u16::try_from(offset)?, window)?;
        }
        collect_direct_access(
            &mut accesses,
            bank,
            0xBFFE,
            &[bytes[0x3FFE], bytes[0x3FFF], fixed[0]],
        )?;
        collect_direct_access(
            &mut accesses,
            bank,
            0xBFFF,
            &[bytes[0x3FFF], fixed[0], fixed[1]],
        )?;
    }
    for (offset, window) in fixed.windows(3).enumerate() {
        collect_direct_access(
            &mut accesses,
            FIXED_PRG_BANK,
            0xC000 + u16::try_from(offset)?,
            window,
        )?;
    }
    Ok(accesses)
}

fn collect_direct_access(
    accesses: &mut BTreeMap<(u8, u16), DirectAccessKinds>,
    bank: u8,
    address: u16,
    bytes: &[u8],
) -> Result<()> {
    if bytes.get(1..3) != Some(&[0xCC, 0x05]) {
        return Ok(());
    }
    let instruction = decode_bytes(bytes).with_context(|| {
        format!("decode pending-state direct candidate at {bank:02X}:${address:04X}")
    })?;
    let semantics =
        Rp2A03::semantics(&instruction, &address).expect("RP2A03 static semantics are infallible");
    let mut kinds = DirectAccessKinds::default();
    for access in semantics.location_accesses {
        if access.location
            != Location::Memory(MemoryAddress::Direct(PENDING_SHARED_MENU_REQUEST_STATE))
        {
            continue;
        }
        match access.kind {
            AccessKind::Read => kinds.reads = true,
            AccessKind::Write => kinds.writes = true,
        }
    }
    if kinds.reads || kinds.writes {
        ensure!(
            accesses.insert((bank, address), kinds).is_none(),
            "duplicate pending-state direct access at {bank:02X}:${address:04X}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::HEADER_SIZE;

    fn synthetic_source(edits: &[(u8, usize, &[u8])]) -> Rom {
        let mut bytes = vec![0x60; HEADER_SIZE + 16 * PRG_BANK_BYTE_COUNT];
        bytes[..HEADER_SIZE].fill(0);
        bytes[..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 16;
        bytes[6] = 0xA0;
        for &(bank, bank_offset, replacement) in edits {
            let offset = HEADER_SIZE + usize::from(bank) * PRG_BANK_BYTE_COUNT + bank_offset;
            bytes[offset..offset + replacement.len()].copy_from_slice(replacement);
        }
        Rom::parse(bytes).unwrap()
    }

    #[test]
    fn bank_fifteen_bytes_are_scanned_in_lower_and_fixed_cpu_projections() {
        let source = synthetic_source(&[(FIXED_PRG_BANK, 0x2100, &[0xAD, 0xCC, 0x05])]);

        let accesses = scan_direct_accesses(&source).unwrap();

        assert_eq!(
            accesses,
            BTreeMap::from([
                (
                    (FIXED_PRG_BANK, 0xA100),
                    DirectAccessKinds {
                        reads: true,
                        writes: false,
                    },
                ),
                (
                    (FIXED_PRG_BANK, 0xE100),
                    DirectAccessKinds {
                        reads: true,
                        writes: false,
                    },
                ),
            ])
        );
    }

    #[test]
    fn switchable_bank_end_fetches_operand_bytes_from_the_fixed_window() {
        let source = synthetic_source(&[
            (0x02, 0x3FFE, &[0xAD, 0xCC]),
            (FIXED_PRG_BANK, 0x0000, &[0x05]),
        ]);

        let accesses = scan_direct_accesses(&source).unwrap();

        assert_eq!(
            accesses,
            BTreeMap::from([(
                (0x02, 0xBFFE),
                DirectAccessKinds {
                    reads: true,
                    writes: false,
                },
            )])
        );
    }
}
