use anyhow::{Context, Result, ensure};

use crate::{rom::Rom, typed_source::decode_rp2a03_sequence};

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const SOURCE_PRG_BANK_COUNT: u8 = 16;
const FIXED_PRG_BANK: u8 = 0x0F;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const FIXED_CPU_START: u16 = 0xC000;

pub(crate) const BANKED_CALL_DISPATCH_ADDRESS: u16 = 0xC9FA;
const BANKED_CALL_TABLE_ADDRESS: u16 = 0xBFA0;
const BANKED_CALL_DISPATCH_CODE: [u8; 35] = [
    0xAA, 0xA5, 0x29, 0x48, 0x8A, 0x20, 0xA6, 0xC9, 0xA9, 0xCA, 0x48, 0xA9, 0x18, 0x48, 0xA5, 0x44,
    0x0A, 0xAA, 0xBD, 0xA0, 0xBF, 0x85, 0x45, 0xBD, 0xA1, 0xBF, 0x85, 0x46, 0x6C, 0x45, 0x00, 0x68,
    0x4C, 0xA6, 0xC9,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BankedCallTransfer {
    Call,
    TailJump,
}

impl BankedCallTransfer {
    fn opcode(self) -> u8 {
        match self {
            Self::Call => 0x20,
            Self::TailJump => 0x4C,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BankedCallDispatchBinding {
    call_address: u16,
    requested_bank: u8,
    selector: u8,
    target: u16,
}

impl BankedCallDispatchBinding {
    pub(crate) fn call_address(self) -> u16 {
        self.call_address
    }

    pub(crate) fn requested_bank(self) -> u8 {
        self.requested_bank
    }

    pub(crate) fn selector(self) -> u8 {
        self.selector
    }

    pub(crate) fn target(self) -> u16 {
        self.target
    }
}

/// Binds the source `$C9FA` banked call for one caller-owned bank and handler selector.
///
/// The helper saves the current bank shadow, selects `requested_bank`, reads a handler pointer
/// from that bank's `$BFA0` table using the original eight-bit `ASL; TAX` selector arithmetic,
/// tail-jumps to the handler, and restores the saved bank after the handler returns. This binder
/// establishes the structure and exact target only; it does not claim that the caller is live.
pub(crate) fn bind_banked_call_dispatch(
    source: &Rom,
    caller_bank: u8,
    call_address: u16,
    transfer: BankedCallTransfer,
    requested_bank: u8,
    selector: u8,
    role: &str,
) -> Result<BankedCallDispatchBinding> {
    ensure!(!role.is_empty(), "banked call role is empty");
    ensure!(
        caller_bank < SOURCE_PRG_BANK_COUNT && requested_bank < SOURCE_PRG_BANK_COUNT,
        "{role} names a PRG bank outside the source MMC4 selector range"
    );
    ensure!(
        call_address >= SWITCHABLE_CPU_START,
        "{role} call is below executable PRG space"
    );

    let call = source_cpu_bytes(source, caller_bank, call_address, 3)?;
    ensure!(
        call == [
            transfer.opcode(),
            BANKED_CALL_DISPATCH_ADDRESS as u8,
            (BANKED_CALL_DISPATCH_ADDRESS >> 8) as u8,
        ],
        "{role} no longer enters the banked-call dispatcher with the declared transfer"
    );
    decode_rp2a03_sequence(&call, call_address, role)?;

    let helper = source_cpu_bytes(
        source,
        FIXED_PRG_BANK,
        BANKED_CALL_DISPATCH_ADDRESS,
        BANKED_CALL_DISPATCH_CODE.len(),
    )?;
    ensure!(
        helper == BANKED_CALL_DISPATCH_CODE,
        "source banked-call dispatcher changed"
    );
    decode_rp2a03_sequence(
        &helper,
        BANKED_CALL_DISPATCH_ADDRESS,
        "source banked-call dispatcher",
    )?;

    let doubled = selector.wrapping_mul(2);
    let low_address = BANKED_CALL_TABLE_ADDRESS.wrapping_add(u16::from(doubled));
    let high_address = low_address.wrapping_add(1);
    let low = source_cpu_byte(source, requested_bank, low_address)?;
    let high = source_cpu_byte(source, requested_bank, high_address)?;
    Ok(BankedCallDispatchBinding {
        call_address,
        requested_bank,
        selector,
        target: u16::from_le_bytes([low, high]),
    })
}

fn source_cpu_bytes(
    source: &Rom,
    selected_bank: u8,
    address: u16,
    byte_count: usize,
) -> Result<Vec<u8>> {
    (0..byte_count)
        .map(|offset| {
            source_cpu_byte(
                source,
                selected_bank,
                address.wrapping_add(u16::try_from(offset)?),
            )
        })
        .collect()
}

fn source_cpu_byte(source: &Rom, selected_bank: u8, address: u16) -> Result<u8> {
    ensure!(
        selected_bank < SOURCE_PRG_BANK_COUNT,
        "source banked call selected a bank outside the source PRG"
    );
    ensure!(
        address >= SWITCHABLE_CPU_START,
        "source banked call tried to read RAM at ${address:04X}"
    );
    let (physical_bank, relative) = if address >= FIXED_CPU_START {
        (FIXED_PRG_BANK, usize::from(address - FIXED_CPU_START))
    } else {
        (selected_bank, usize::from(address - SWITCHABLE_CPU_START))
    };
    let offset = usize::from(physical_bank)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(relative))
        .context("source banked-call PRG offset overflow")?;
    source
        .prg()
        .get(offset)
        .copied()
        .with_context(|| format!("source banked-call read exceeds PRG at ${address:04X}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::HEADER_SIZE;

    fn synthetic_source(
        caller_bank: u8,
        call_address: u16,
        requested_bank: u8,
        selector: u8,
        target: u16,
    ) -> Rom {
        let mut bytes = vec![0x60; HEADER_SIZE + 16 * SOURCE_PRG_BANK_BYTE_COUNT];
        bytes[..HEADER_SIZE].fill(0);
        bytes[..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 16;
        bytes[6] = 0xA0;
        let caller = HEADER_SIZE
            + usize::from(caller_bank) * SOURCE_PRG_BANK_BYTE_COUNT
            + usize::from(call_address - SWITCHABLE_CPU_START);
        bytes[caller..caller + 3].copy_from_slice(&[
            0x20,
            BANKED_CALL_DISPATCH_ADDRESS as u8,
            (BANKED_CALL_DISPATCH_ADDRESS >> 8) as u8,
        ]);
        let fixed = HEADER_SIZE + usize::from(FIXED_PRG_BANK) * SOURCE_PRG_BANK_BYTE_COUNT;
        let helper = fixed + usize::from(BANKED_CALL_DISPATCH_ADDRESS - FIXED_CPU_START);
        bytes[helper..helper + BANKED_CALL_DISPATCH_CODE.len()]
            .copy_from_slice(&BANKED_CALL_DISPATCH_CODE);
        let doubled = selector.wrapping_mul(2);
        let table_address = BANKED_CALL_TABLE_ADDRESS.wrapping_add(u16::from(doubled));
        for (offset, value) in target.to_le_bytes().into_iter().enumerate() {
            let address = table_address.wrapping_add(u16::try_from(offset).unwrap());
            let (bank, relative) = if address >= FIXED_CPU_START {
                (FIXED_PRG_BANK, usize::from(address - FIXED_CPU_START))
            } else {
                (requested_bank, usize::from(address - SWITCHABLE_CPU_START))
            };
            bytes[HEADER_SIZE + usize::from(bank) * SOURCE_PRG_BANK_BYTE_COUNT + relative] = value;
        }
        Rom::parse(bytes).unwrap()
    }

    #[test]
    fn resolves_the_selected_banks_handler_pointer() {
        let source = synthetic_source(0x02, 0x9000, 0x05, 1, 0xA123);

        let binding = bind_banked_call_dispatch(
            &source,
            0x02,
            0x9000,
            BankedCallTransfer::Call,
            0x05,
            1,
            "test",
        )
        .unwrap();

        assert_eq!(binding.requested_bank(), 0x05);
        assert_eq!(binding.selector(), 1);
        assert_eq!(binding.target(), 0xA123);
    }

    #[test]
    fn high_selector_uses_the_fixed_window_after_crossing_c000() {
        let source = synthetic_source(0x02, 0x9000, 0x05, 0x7F, 0xC234);

        let binding = bind_banked_call_dispatch(
            &source,
            0x02,
            0x9000,
            BankedCallTransfer::Call,
            0x05,
            0x7F,
            "test",
        )
        .unwrap();

        assert_eq!(binding.target(), 0xC234);
    }

    #[test]
    fn changed_dispatcher_fails_before_a_target_is_admitted() {
        let source = synthetic_source(0x02, 0x9000, 0x05, 1, 0xA123);
        let mut bytes = source.data().to_vec();
        let helper = HEADER_SIZE
            + usize::from(FIXED_PRG_BANK) * SOURCE_PRG_BANK_BYTE_COUNT
            + usize::from(BANKED_CALL_DISPATCH_ADDRESS - FIXED_CPU_START);
        bytes[helper] = 0xEA;
        let source = Rom::parse(bytes).unwrap();

        let error = bind_banked_call_dispatch(
            &source,
            0x02,
            0x9000,
            BankedCallTransfer::Call,
            0x05,
            1,
            "test",
        )
        .unwrap_err();

        assert!(error.to_string().contains("dispatcher changed"));
    }

    #[test]
    fn tail_jump_uses_the_same_banked_dispatch_structure() {
        let source = synthetic_source(0x02, 0x9000, 0x05, 1, 0xA123);
        let mut bytes = source.data().to_vec();
        let caller = HEADER_SIZE
            + 2 * SOURCE_PRG_BANK_BYTE_COUNT
            + usize::from(0x9000 - SWITCHABLE_CPU_START);
        bytes[caller] = BankedCallTransfer::TailJump.opcode();
        let source = Rom::parse(bytes).unwrap();

        let binding = bind_banked_call_dispatch(
            &source,
            0x02,
            0x9000,
            BankedCallTransfer::TailJump,
            0x05,
            1,
            "test",
        )
        .unwrap();

        assert_eq!(binding.target(), 0xA123);
    }
}
