use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{rom::Rom, typed_source::decode_rp2a03_sequence};

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const FIXED_PRG_BANK: u8 = 0x0F;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const FIXED_CPU_START: u16 = 0xC000;

pub(crate) const INLINE_POINTER_DISPATCH_ADDRESS: u16 = 0xC34C;
pub(super) const INLINE_POINTER_TARGET_JUMP_ADDRESS: u16 = 0xC367;
pub(super) const INLINE_POINTER_DISPATCH_CODE: [u8; 30] = [
    0x0A, 0x84, 0x0F, 0x86, 0x0E, 0xA8, 0xC8, 0x68, 0x85, 0x0C, 0x68, 0x85, 0x0D, 0xB1, 0x0C, 0xAA,
    0xC8, 0xB1, 0x0C, 0x85, 0x0D, 0x86, 0x0C, 0xA6, 0x0E, 0xA4, 0x0F, 0x6C, 0x0C, 0x00,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct InlinePointerDispatchBinding {
    call_address: u16,
    table_start: u16,
    selector_targets: Vec<(u8, u16)>,
}

impl InlinePointerDispatchBinding {
    pub(super) fn call_address(&self) -> u16 {
        self.call_address
    }

    pub(super) fn table_start(&self) -> u16 {
        self.table_start
    }

    pub(super) fn selector_count(&self) -> usize {
        self.selector_targets.len()
    }

    pub(super) fn targets_in_selector_order(&self) -> Vec<u16> {
        self.selector_targets
            .iter()
            .map(|(_, target)| *target)
            .collect()
    }

    pub(super) fn distinct_targets(&self) -> BTreeSet<u16> {
        self.selector_targets
            .iter()
            .map(|(_, target)| *target)
            .collect()
    }
}

/// Binds the source `$C34C` stack-return dispatcher for one exact call site and one caller-owned
/// selector domain.
///
/// The helper does not return normally. It removes the JSR return address, treats the bytes after
/// the call as a little-endian pointer table, restores X/Y, and tail-jumps through `$000C`.
/// Selector arithmetic is intentionally evaluated as the original eight-bit `ASL; TAY; INY`
/// sequence, including the selector-127 high-byte wrap back to the saved return address.
pub(super) fn bind_inline_pointer_dispatch(
    source: &Rom,
    caller_bank: u8,
    call_address: u16,
    selectors: impl IntoIterator<Item = u8>,
    role: &str,
) -> Result<InlinePointerDispatchBinding> {
    ensure!(!role.is_empty(), "inline pointer dispatch role is empty");
    ensure!(
        call_address >= SWITCHABLE_CPU_START,
        "{role} call is below executable PRG space"
    );

    let call = source_cpu_bytes(source, caller_bank, call_address, 3)?;
    ensure!(
        call == [
            0x20,
            INLINE_POINTER_DISPATCH_ADDRESS as u8,
            (INLINE_POINTER_DISPATCH_ADDRESS >> 8) as u8,
        ],
        "{role} no longer calls the inline pointer dispatcher"
    );
    decode_rp2a03_sequence(&call, call_address, role)?;

    let helper = source_cpu_bytes(
        source,
        FIXED_PRG_BANK,
        INLINE_POINTER_DISPATCH_ADDRESS,
        INLINE_POINTER_DISPATCH_CODE.len(),
    )?;
    ensure!(
        helper == INLINE_POINTER_DISPATCH_CODE,
        "source inline pointer dispatcher changed"
    );
    decode_rp2a03_sequence(
        &helper,
        INLINE_POINTER_DISPATCH_ADDRESS,
        "source inline pointer dispatcher",
    )?;

    let selectors = selectors.into_iter().collect::<BTreeSet<_>>();
    ensure!(!selectors.is_empty(), "{role} selector domain is empty");
    let return_address_on_stack = call_address
        .checked_add(2)
        .context("inline pointer dispatch return address overflow")?;
    let table_start = return_address_on_stack
        .checked_add(1)
        .context("inline pointer dispatch table address overflow")?;
    let selector_targets = selectors
        .into_iter()
        .map(|selector| {
            let doubled = selector.wrapping_mul(2);
            let low_index = doubled.wrapping_add(1);
            let high_index = low_index.wrapping_add(1);
            let low_address = return_address_on_stack.wrapping_add(u16::from(low_index));
            let high_address = return_address_on_stack.wrapping_add(u16::from(high_index));
            let low = source_cpu_byte(source, caller_bank, low_address)?;
            let high = source_cpu_byte(source, caller_bank, high_address)?;
            Ok((selector, u16::from_le_bytes([low, high])))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(InlinePointerDispatchBinding {
        call_address,
        table_start,
        selector_targets,
    })
}

fn source_cpu_bytes(
    source: &Rom,
    caller_bank: u8,
    address: u16,
    byte_count: usize,
) -> Result<Vec<u8>> {
    (0..byte_count)
        .map(|offset| {
            source_cpu_byte(
                source,
                caller_bank,
                address.wrapping_add(u16::try_from(offset)?),
            )
        })
        .collect()
}

fn source_cpu_byte(source: &Rom, caller_bank: u8, address: u16) -> Result<u8> {
    ensure!(
        caller_bank <= FIXED_PRG_BANK,
        "source inline dispatcher caller bank is outside the source PRG"
    );
    ensure!(
        address >= SWITCHABLE_CPU_START,
        "source inline dispatcher tried to read RAM at ${address:04X}"
    );
    let (physical_bank, relative) = if address >= FIXED_CPU_START {
        (FIXED_PRG_BANK, usize::from(address - FIXED_CPU_START))
    } else {
        (caller_bank, usize::from(address - SWITCHABLE_CPU_START))
    };
    let offset = usize::from(physical_bank)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(relative))
        .context("source inline dispatcher PRG offset overflow")?;
    source
        .prg()
        .get(offset)
        .copied()
        .with_context(|| format!("source inline dispatcher read exceeds PRG at ${address:04X}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::HEADER_SIZE;

    fn synthetic_source(call_address: u16, table_bytes: &[u8]) -> Rom {
        let mut bytes = vec![0x60; HEADER_SIZE + 16 * SOURCE_PRG_BANK_BYTE_COUNT];
        bytes[..HEADER_SIZE].fill(0);
        bytes[..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 16;
        bytes[6] = 0xA0;
        let fixed_start = HEADER_SIZE + 15 * SOURCE_PRG_BANK_BYTE_COUNT;
        let helper = fixed_start + usize::from(INLINE_POINTER_DISPATCH_ADDRESS - FIXED_CPU_START);
        bytes[helper..helper + INLINE_POINTER_DISPATCH_CODE.len()]
            .copy_from_slice(&INLINE_POINTER_DISPATCH_CODE);
        let call = fixed_start + usize::from(call_address - FIXED_CPU_START);
        bytes[call..call + 3].copy_from_slice(&[
            0x20,
            INLINE_POINTER_DISPATCH_ADDRESS as u8,
            (INLINE_POINTER_DISPATCH_ADDRESS >> 8) as u8,
        ]);
        bytes[call + 3..call + 3 + table_bytes.len()].copy_from_slice(table_bytes);
        Rom::parse(bytes).unwrap()
    }

    #[test]
    fn selector_domain_reads_the_owned_pointer_slots_in_order() {
        let source = synthetic_source(0xF000, &[0x34, 0xC0, 0x78, 0xE1]);

        let binding =
            bind_inline_pointer_dispatch(&source, FIXED_PRG_BANK, 0xF000, [0, 1], "test").unwrap();

        assert_eq!(binding.table_start(), 0xF003);
        assert_eq!(binding.targets_in_selector_order(), [0xC034, 0xE178]);
    }

    #[test]
    fn selector_arithmetic_keeps_the_helpers_eight_bit_aliasing() {
        let mut table = vec![0x60; 0x100];
        table[0] = 0x34;
        table[1] = 0xC0;
        table[0xFE] = 0x78;
        let source = synthetic_source(0xF000, &table);

        let binding = bind_inline_pointer_dispatch(
            &source,
            FIXED_PRG_BANK,
            0xF000,
            [0, 0x7F, 0x80, 0xFF],
            "test",
        )
        .unwrap();

        // 0x80 aliases selector 0 after ASL. Selector 0x7F reads its low byte at +0xFE and its
        // high byte from the JSR high operand after the second INY wraps Y to zero.
        assert_eq!(
            binding.targets_in_selector_order(),
            [0xC034, 0xC378, 0xC034, 0xC378]
        );
    }

    #[test]
    fn a_changed_helper_fails_before_routes_are_admitted() {
        let source = synthetic_source(0xF000, &[0x34, 0xC0]);
        let mut bytes = source.data().to_vec();
        let helper_offset = HEADER_SIZE
            + 15 * SOURCE_PRG_BANK_BYTE_COUNT
            + usize::from(INLINE_POINTER_DISPATCH_ADDRESS - FIXED_CPU_START);
        bytes[helper_offset] = 0xEA;
        let source = Rom::parse(bytes).unwrap();

        let error =
            bind_inline_pointer_dispatch(&source, FIXED_PRG_BANK, 0xF000, [0], "test").unwrap_err();

        assert!(error.to_string().contains("dispatcher changed"));
    }
}
