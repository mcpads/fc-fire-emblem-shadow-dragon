use super::*;

#[derive(Debug)]
pub(crate) struct SharedMenuControllerSource {
    dispatch_call: u16,
}

impl SharedMenuControllerSource {
    pub(crate) fn dispatch_call(&self) -> u16 {
        self.dispatch_call
    }
}

pub(crate) fn bind_shared_menu_controller_source(rom: &Rom) -> Result<SharedMenuControllerSource> {
    let menu_dispatch = switchable_slice(rom, 0x0B, 0x9251, 20)?;
    ensure!(
        menu_dispatch[..6] == [0xAD, 0xDE, 0x05, 0x20, 0x4C, 0xC3],
        "shared menu-controller dispatcher changed"
    );
    let actual_menu_handlers = menu_dispatch[6..]
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        actual_menu_handlers == MENU_CONTROLLER_HANDLERS,
        "shared menu-controller handler table changed"
    );

    Ok(SharedMenuControllerSource {
        dispatch_call: 0x9254,
    })
}

pub(super) fn validate_state_tables(rom: &Rom) -> Result<()> {
    let shop_dispatch = switchable_slice(rom, 0x06, 0x99AC, 32)?;
    ensure!(
        shop_dispatch[..6] == [0xAD, 0xDB, 0x05, 0x20, 0x4C, 0xC3],
        "shop outer-state dispatcher changed"
    );
    let actual_shop_handlers = shop_dispatch[6..]
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        actual_shop_handlers == SHOP_STATE_HANDLERS,
        "shop outer-state handler table changed"
    );

    bind_shared_menu_controller_source(rom)?;

    ensure!(
        MENU_CONTROLLER_INDEX_ADDRESS == 0x05CE,
        "menu controller index address drift"
    );
    ensure!(
        MENU_CONTROLLER_STATE_ADDRESS == 0x05DE,
        "menu controller address drift"
    );
    ensure!(
        MENU_CHOICE_MASK_ADDRESS == 0x7FEE,
        "menu choice-mask address drift"
    );
    ensure!(
        MENU_SELECTION_BASE_ADDRESS == 0x7FF3,
        "menu selection base address drift"
    );
    ensure!(MENU_RESULT_ADDRESS == 0x05EB, "menu result address drift");
    Ok(())
}

pub(super) fn validate_item_eligibility_case(rom: &Rom) -> Result<()> {
    let requirement = fixed_slice(rom, 0xD6C3, 1)?[0];
    let flags = fixed_slice(rom, 0xD9D3, 1)?[0];
    let allowed_classes = switchable_slice(rom, 0x06, 0xA3FE, 5)?;

    ensure!(
        requirement == 0x01,
        "representative item 11 weapon-level requirement changed"
    );
    ensure!(
        flags == 0x0C,
        "representative item 11 eligibility flags changed"
    );
    ensure!(
        allowed_classes == [0x0B, 0x0C, 0x0E, 0x0F, 0xEF],
        "representative item 11 allowed-class list changed"
    );
    Ok(())
}

pub(super) fn bind_source_region(rom: &Rom, spec: SourceRegionSpec) -> Result<SourceRegionBinding> {
    let bytes = switchable_slice(rom, spec.prg_bank, spec.cpu_address, spec.byte_count)?;
    let actual_sha1 = sha1_hex(bytes);
    ensure!(
        actual_sha1 == spec.expected_sha1,
        "{} code changed: expected {}, found {}",
        spec.role,
        spec.expected_sha1,
        actual_sha1
    );
    let file_offset = switchable_file_offset(spec.prg_bank, spec.cpu_address)?;

    Ok(SourceRegionBinding {
        role: spec.role,
        prg_bank: spec.prg_bank,
        prg_bank_hex: format!("0x{:02X}", spec.prg_bank),
        cpu_address: spec.cpu_address,
        cpu_address_hex: hex_u16(spec.cpu_address),
        file_offset,
        file_offset_hex: format!("0x{file_offset:05X}"),
        byte_count: spec.byte_count,
        source_sha1: actual_sha1,
    })
}

pub(super) fn switchable_slice(
    rom: &Rom,
    prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
) -> Result<&[u8]> {
    let file_offset = switchable_file_offset(prg_bank, cpu_address)?;
    let end = file_offset
        .checked_add(byte_count)
        .context("shop-flow code range overflow")?;
    rom.data()
        .get(file_offset..end)
        .with_context(|| format!("shop-flow code range exceeds ROM at {file_offset:05X}"))
}

pub(super) fn fixed_slice(rom: &Rom, cpu_address: u16, byte_count: usize) -> Result<&[u8]> {
    let file_offset = fixed_file_offset(cpu_address)?;
    let end = file_offset
        .checked_add(byte_count)
        .context("shop-flow fixed source range overflow")?;
    rom.data()
        .get(file_offset..end)
        .with_context(|| format!("shop-flow fixed source range exceeds ROM at {file_offset:05X}"))
}

pub(super) fn fixed_file_offset(cpu_address: u16) -> Result<usize> {
    ensure!(
        cpu_address >= FIXED_CPU_START,
        "fixed CPU address {cpu_address:04X} is below the fixed window"
    );
    Ok(HEADER_SIZE + FIXED_PRG_BANK * PRG_BANK_SIZE + usize::from(cpu_address - FIXED_CPU_START))
}

pub(super) fn switchable_file_offset(prg_bank: u8, cpu_address: u16) -> Result<usize> {
    ensure!(prg_bank < 0x0F, "shop-flow code uses unavailable PRG bank");
    ensure!(
        (SWITCHABLE_CPU_START..SWITCHABLE_CPU_END_EXCLUSIVE).contains(&cpu_address),
        "shop-flow CPU address {cpu_address:04X} is outside the switchable window"
    );
    Ok(HEADER_SIZE
        + usize::from(prg_bank) * PRG_BANK_SIZE
        + usize::from(cpu_address - SWITCHABLE_CPU_START))
}

pub(super) fn location(prg_bank: u8, cpu_address: u16) -> CodeLocation {
    CodeLocation {
        prg_bank,
        prg_bank_hex: format!("0x{prg_bank:02X}"),
        cpu_address,
        cpu_address_hex: hex_u16(cpu_address),
    }
}

pub(super) fn hex_u16(value: u16) -> String {
    format!("0x{value:04X}")
}
