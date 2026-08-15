use anyhow::{Context, Result, ensure};

use crate::{
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::count_direct_transfers_to_range,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    tracked::TrackedImage,
    typed_source::decode_rp2a03_sequence,
};

use super::{
    BattleCompositionRuntimeLayout, CACHE_UPLOADED_MARKER, CACHED_DIALOGUE_SELECTOR_ADDRESS,
    REMAP_STATE_ADDRESS, UPLOAD_RENDER_MASK,
};

const FINAL_LOADER_BANK: u8 = 0x04;
const FINAL_LOADER_ADDRESS: u16 = 0x800F;
const OVERRIDE_READER_BANK: u8 = 0x05;
const OVERRIDE_READER_ADDRESS: u16 = 0x85A5;
const DIALOGUE_SELECTOR_ADDRESS: u16 = 0x7936;

// Bank 04 has one source-bound FF run at $BD82..$BFA0. Use a smaller interior span whose
// whole-PRG raw JSR/JMP-target backstop is also empty, leaving both boundaries untouched.
const CACHE_REFRESH_ADDRESS: u16 = 0xBF40;
const CACHE_REFRESH_CAVE_END: u16 = 0xBF80;

const FINAL_LOADER_REGION_ADDRESS: u16 = 0x8000;
const FINAL_LOADER_REGION: [u8; 29] = [
    0xAD, 0x35, 0x79, 0x0A, 0xA8, 0xB9, 0x2D, 0x80, 0x85, 0x00, 0xB9, 0x2E, 0x80, 0x85, 0x01, 0xAD,
    0x36, 0x79, 0x0A, 0xA8, 0xB1, 0x00, 0x85, 0x76, 0xC8, 0xB1, 0x00, 0x85, 0x77,
];
const OVERRIDE_READER_REGION: [u8; 9] = [0xAD, 0x36, 0x79, 0xF0, 0x04, 0xC9, 0x03, 0xD0, 0x2F];

const NMI_PPU_RESTORE_CALLS_ADDRESS: u16 = 0xC185;
const NMI_PPU_RESTORE_CALLS: [u8; 6] = [0x20, 0x33, 0xC7, 0x20, 0x6A, 0xC3];
const PPU_CONTROL_AND_MASK_RESTORE_ADDRESS: u16 = 0xC733;
const PPU_CONTROL_AND_MASK_RESTORE: [u8; 11] = [
    0xA5, 0xCD, 0x8D, 0x00, 0x20, 0xA5, 0xCC, 0x8D, 0x01, 0x20, 0x60,
];
const PPU_SCROLL_RESTORE_ADDRESS: u16 = 0xC36A;
const PPU_SCROLL_RESTORE: [u8; 14] = [
    0xAD, 0x02, 0x20, 0xA5, 0xCB, 0x8D, 0x05, 0x20, 0xA5, 0xCA, 0x8D, 0x05, 0x20, 0x60,
];

/// Bind the final record consumer, the only other direct selector reader, and the local code
/// cave. Installing here covers every producer that changes `$7936` before the record is chosen.
pub(super) fn bind_final_dialogue_cache_refresh_source(source: &Rom) -> Result<()> {
    bind_switchable_code(
        source,
        FINAL_LOADER_BANK,
        FINAL_LOADER_REGION_ADDRESS,
        &FINAL_LOADER_REGION,
        "final battle-dialogue record loader",
    )?;
    bind_switchable_code(
        source,
        OVERRIDE_READER_BANK,
        OVERRIDE_READER_ADDRESS,
        &OVERRIDE_READER_REGION,
        "battle-dialogue override reader",
    )?;
    bind_cache_refresh_cave(source)?;
    ensure!(
        count_direct_transfers_to_range(
            source.prg(),
            CACHE_REFRESH_ADDRESS,
            CACHE_REFRESH_CAVE_END,
        )? == 0,
        "bank-04 dialogue cache-refresh cave gained a raw direct transfer candidate"
    );

    let actual = direct_selector_readers(source)?;
    let expected = [
        (FINAL_LOADER_BANK, FINAL_LOADER_ADDRESS),
        (OVERRIDE_READER_BANK, OVERRIDE_READER_ADDRESS),
    ];
    ensure!(
        actual == expected,
        "direct battle-dialogue selector reader census changed: expected {expected:02X?}, found {actual:02X?}"
    );
    Ok(())
}

/// Bind the ordinary NMI path that restores the PPU mask and scroll after a mismatched cache key
/// caused a synchronous refresh. The matching-key path never touches the PPU.
pub(super) fn bind_final_dialogue_cache_refresh_base(base: &Rom) -> Result<()> {
    bind_cache_refresh_cave(base)?;
    for (role, address, expected) in [
        (
            "NMI PPU restore call pair",
            NMI_PPU_RESTORE_CALLS_ADDRESS,
            NMI_PPU_RESTORE_CALLS.as_slice(),
        ),
        (
            "PPU control and mask restore",
            PPU_CONTROL_AND_MASK_RESTORE_ADDRESS,
            PPU_CONTROL_AND_MASK_RESTORE.as_slice(),
        ),
        (
            "PPU scroll restore",
            PPU_SCROLL_RESTORE_ADDRESS,
            PPU_SCROLL_RESTORE.as_slice(),
        ),
    ] {
        let actual = fixed_bytes(base, address, expected.len())?;
        ensure!(actual == expected, "{role} changed");
        decode_rp2a03_sequence(actual, address, role)?;
    }
    Ok(())
}

pub(super) fn install_final_dialogue_cache_refresh(
    image: &mut TrackedImage,
    layout: BattleCompositionRuntimeLayout,
) -> Result<()> {
    let refresh = build_final_dialogue_cache_refresh(layout)?;
    ensure!(
        usize::from(CACHE_REFRESH_ADDRESS) + refresh.len() <= usize::from(CACHE_REFRESH_CAVE_END),
        "final dialogue cache refresh exceeds its bank-04 cave"
    );
    image.write_expected(
        "guarded final battle-dialogue cache refresh",
        switchable_bank_file_offset(FINAL_LOADER_BANK, CACHE_REFRESH_ADDRESS)?,
        &vec![0xFF; refresh.len()],
        &refresh,
    )?;
    redirect_final_dialogue_loader(image, CACHE_REFRESH_ADDRESS)
}

fn build_final_dialogue_cache_refresh(layout: BattleCompositionRuntimeLayout) -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::LdaAbsolute(REMAP_STATE_ADDRESS),
        Instruction::AndImmediate(CACHE_UPLOADED_MARKER),
    ];
    let invalid_cache_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(CACHE_REFRESH_ADDRESS));
    instructions.extend([
        Instruction::JsrAbsolute(layout.project_dialogue_selector),
        Instruction::CmpAbsolute(CACHED_DIALOGUE_SELECTOR_ADDRESS),
    ]);
    let matching_key_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(CACHE_REFRESH_ADDRESS));
    let refresh = next_address(CACHE_REFRESH_ADDRESS, &instructions)?;
    instructions[invalid_cache_placeholder] = Instruction::BeqAbsolute(refresh);
    instructions.extend([
        Instruction::LdaImmediate(UPLOAD_RENDER_MASK),
        Instruction::StaAbsolute(0x2001),
        Instruction::JsrAbsolute(layout.compose_page),
    ]);
    let return_selector = next_address(CACHE_REFRESH_ADDRESS, &instructions)?;
    instructions[matching_key_placeholder] = Instruction::BeqAbsolute(return_selector);
    instructions.extend([
        Instruction::LdaAbsolute(DIALOGUE_SELECTOR_ADDRESS),
        Instruction::Rts,
    ]);
    assemble_at(CACHE_REFRESH_ADDRESS, &instructions)
}

fn redirect_final_dialogue_loader(image: &mut TrackedImage, refresh_entry: u16) -> Result<()> {
    image.write_expected(
        "guard the battle cache at the final dialogue loader",
        switchable_bank_file_offset(FINAL_LOADER_BANK, FINAL_LOADER_ADDRESS)?,
        &assemble_at(
            FINAL_LOADER_ADDRESS,
            &[Instruction::LdaAbsolute(DIALOGUE_SELECTOR_ADDRESS)],
        )?,
        &assemble_at(
            FINAL_LOADER_ADDRESS,
            &[Instruction::JsrAbsolute(refresh_entry)],
        )?,
    )
}

fn bind_cache_refresh_cave(rom: &Rom) -> Result<()> {
    let cave = switchable_bytes(
        rom,
        FINAL_LOADER_BANK,
        CACHE_REFRESH_ADDRESS,
        usize::from(CACHE_REFRESH_CAVE_END - CACHE_REFRESH_ADDRESS),
    )?;
    ensure!(
        cave.iter().all(|byte| *byte == 0xFF),
        "bank-04 dialogue cache-refresh cave is no longer all FF"
    );
    Ok(())
}

fn direct_selector_readers(source: &Rom) -> Result<Vec<(u8, u16)>> {
    ensure!(
        source.prg().len() % 0x4000 == 0,
        "source PRG is not a whole number of 16 KiB banks"
    );
    let mut readers = Vec::new();
    for (bank, bytes) in source.prg().chunks_exact(0x4000).enumerate() {
        for (offset, window) in bytes.windows(3).enumerate() {
            if window == [0xAD, 0x36, 0x79] {
                readers.push((
                    u8::try_from(bank)?,
                    0x8000_u16
                        .checked_add(u16::try_from(offset)?)
                        .context("selector reader address overflow")?,
                ));
            }
        }
    }
    Ok(readers)
}

fn bind_switchable_code(
    source: &Rom,
    bank: u8,
    address: u16,
    expected: &[u8],
    role: &str,
) -> Result<()> {
    let actual = switchable_bytes(source, bank, address, expected.len())?;
    ensure!(actual == expected, "{role} source bytes changed");
    decode_rp2a03_sequence(actual, address, role)?;
    Ok(())
}

fn switchable_bytes(rom: &Rom, bank: u8, address: u16, length: usize) -> Result<&[u8]> {
    let offset = switchable_bank_file_offset(bank, address)?;
    rom.data()
        .get(offset..offset + length)
        .with_context(|| format!("switchable read at {bank:02X}:{address:04X} is out of range"))
}

fn fixed_bytes(rom: &Rom, address: u16, length: usize) -> Result<&[u8]> {
    ensure!(address >= 0xC000, "fixed-bank address is below C000");
    let base = rom
        .prg()
        .len()
        .checked_sub(0x4000)
        .context("ROM has no fixed 16 KiB bank")?;
    let offset = base + usize::from(address - 0xC000);
    rom.prg()
        .get(offset..offset + length)
        .with_context(|| format!("fixed-bank read at {address:04X} is out of range"))
}

fn next_address(origin: u16, instructions: &[Instruction]) -> Result<u16> {
    origin
        .checked_add(u16::try_from(assemble_at(origin, instructions)?.len())?)
        .context("final dialogue cache-refresh address overflow")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapper165::battle_composition_loader_probe::PROBE_RUNTIME_LAYOUT;

    #[test]
    fn final_loader_hook_replaces_only_the_last_selector_read() {
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0x02);
        let offset = switchable_bank_file_offset(FINAL_LOADER_BANK, FINAL_LOADER_ADDRESS).unwrap();
        bytes[offset..offset + 3].copy_from_slice(&[0xAD, 0x36, 0x79]);
        let mut image = TrackedImage::new(bytes);

        redirect_final_dialogue_loader(&mut image, CACHE_REFRESH_ADDRESS).unwrap();

        assert_eq!(image.writes().len(), 1);
        let output = image.into_data();
        assert_eq!(&output[offset..offset + 3], &[0x20, 0x40, 0xBF]);
    }

    #[test]
    fn matching_projected_key_skips_the_ppu_refresh() {
        let bytes = build_final_dialogue_cache_refresh(PROBE_RUNTIME_LAYOUT).unwrap();

        assert_eq!(
            bytes,
            [
                0xAD, 0xDF, 0x07, // LDA $07DF: cache validity
                0x29, 0x80, // AND #$80
                0xF0, 0x08, // invalid -> refresh
                0x20, 0x30, 0xFD, // project the current dialogue selector
                0xCD, 0xDE, 0x07, // compare the key composed into $07DE
                0xF0, 0x08, // matching key -> return without touching the PPU
                0xA9, 0x06, // mismatched key: blank live rendering
                0x8D, 0x01, 0x20, 0x20, 0x30, 0xFB, // rebuild from all current recipe inputs
                0xAD, 0x36, 0x79, // reproduce the displaced raw-selector read
                0x60,
            ]
        );
    }

    #[test]
    fn ppu_recovery_requires_the_mask_then_scroll_call_pair_and_clean_cave() {
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        for (address, expected) in [
            (
                NMI_PPU_RESTORE_CALLS_ADDRESS,
                NMI_PPU_RESTORE_CALLS.as_slice(),
            ),
            (
                PPU_CONTROL_AND_MASK_RESTORE_ADDRESS,
                PPU_CONTROL_AND_MASK_RESTORE.as_slice(),
            ),
            (PPU_SCROLL_RESTORE_ADDRESS, PPU_SCROLL_RESTORE.as_slice()),
        ] {
            let offset = crate::test_support::synthetic_fixed_bank_file_offset(address);
            bytes[offset..offset + expected.len()].copy_from_slice(expected);
        }
        let candidate = Rom::parse(bytes.clone()).unwrap();
        bind_final_dialogue_cache_refresh_base(&candidate).unwrap();

        let offset = crate::test_support::synthetic_fixed_bank_file_offset(
            PPU_CONTROL_AND_MASK_RESTORE_ADDRESS,
        );
        bytes[offset + 6] ^= 1;
        let error =
            bind_final_dialogue_cache_refresh_base(&Rom::parse(bytes).unwrap()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("control and mask restore changed")
        );
    }
}
