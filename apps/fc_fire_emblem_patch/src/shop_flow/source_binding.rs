use super::*;

const SHOP_ITEM_COMPOSITION_STATE: u8 = 0x03;
pub(crate) const SHOP_ITEM_COMPOSITE_STATE: u8 = 0x15;
const WEAPON_SHOP_FACILITY_SELECTOR: u8 = 0x01;
const SHOP_DIALOGUE_DIRECTORY_SELECTOR: u8 = 0xB1;
const SHOP_ITEM_COMPOSITION_HANDLER: u16 = 0x99FB;
const SHOP_ITEM_COMPOSITION_BYTES: [u8; 19] = [
    0x20, 0x5A, 0x9C, 0x20, 0x5C, 0xE6, 0xA9, 0x00, 0x85, 0x26, 0xA9, 0x15, 0x20, 0x90, 0xE6, 0xEE,
    0xDB, 0x05, 0x60,
];
const SHOP_ITEM_LIST_HANDLER: u16 = 0xA2F2;
const SHOP_ITEM_LIST_HANDLER_BYTES: [u8; 37] = [
    0xA0, 0x03, 0xB1, 0x04, 0x0A, 0xA8, 0xB9, 0xC2, 0xA6, 0x85, 0x04, 0xB9, 0xC3, 0xA6, 0x85, 0x05,
    0xA2, 0x00, 0xA0, 0x00, 0x8E, 0xDC, 0x77, 0xB1, 0x04, 0x9D, 0xD2, 0x77, 0xC9, 0xF0, 0xF0, 0x04,
    0xC8, 0xE8, 0xD0, 0xF3, 0x60,
];
const SHOP_STOCK_POINTER_DIRECTORY_ADDRESS: u16 = 0xA6C2;
const SHOP_STOCK_GROUP_COUNT: usize = 20;
const SHOP_STOCK_DATA_ADDRESS: u16 = 0xA6EA;
const SHOP_STOCK_DATA_END: u16 = 0xA766;
const SHOP_STOCK_TERMINATOR: u8 = 0xF0;

#[derive(Clone, Debug)]
pub(crate) struct ShopItemCompositionSource {
    outer_state_address: u16,
    composition_state: u8,
    composite_state: u8,
    selected_facility_address: u16,
    dialogue_directory_address: u16,
    dialogue_directory_selector: u8,
    selling_facilities: [u8; 3],
    non_selling_facilities: [u8; 2],
    stock_group_ids: BTreeSet<u8>,
    item_source_indices: BTreeSet<usize>,
}

impl ShopItemCompositionSource {
    pub(crate) fn outer_state_address(&self) -> u16 {
        self.outer_state_address
    }

    pub(crate) fn composition_state(&self) -> u8 {
        self.composition_state
    }

    pub(crate) fn composite_state(&self) -> u8 {
        self.composite_state
    }

    pub(crate) fn selected_facility_address(&self) -> u16 {
        self.selected_facility_address
    }

    pub(crate) fn dialogue_directory_address(&self) -> u16 {
        self.dialogue_directory_address
    }

    pub(crate) fn dialogue_directory_selector(&self) -> u8 {
        self.dialogue_directory_selector
    }

    pub(crate) fn selling_facilities(&self) -> [u8; 3] {
        self.selling_facilities
    }

    pub(crate) fn non_selling_facilities(&self) -> [u8; 2] {
        self.non_selling_facilities
    }

    pub(crate) fn stock_group_ids(&self) -> &BTreeSet<u8> {
        &self.stock_group_ids
    }

    pub(crate) fn item_source_indices(&self) -> &BTreeSet<usize> {
        &self.item_source_indices
    }
}

/// Binds the source state which composes the visible shop item rows to the three map-facility
/// selectors that share the weapon-shop handler.  The settled screen is state 4, but the item
/// appender runs while state 3 calls composite state 0x15 and then advances the outer state.
pub(crate) fn bind_shop_item_composition_source(rom: &Rom) -> Result<ShopItemCompositionSource> {
    validate_shop_lifetime_source(rom)?;
    let map_facilities = bind_map_facility_dispatch_source(rom)?;
    let shop_handler = map_facilities
        .handler_target(WEAPON_SHOP_FACILITY_SELECTOR)
        .context("weapon-shop facility selector lost its handler")?;
    let selling_facilities = map_facilities
        .produced_selectors()
        .iter()
        .copied()
        .filter(|selector| map_facilities.handler_target(*selector) == Some(shop_handler))
        .collect::<Vec<_>>();
    let selling_facilities: [u8; 3] = selling_facilities.try_into().map_err(|facilities: Vec<u8>| {
        anyhow::anyhow!(
            "shop handler no longer owns exactly three produced facility selectors: {facilities:02X?}"
        )
    })?;
    ensure!(
        selling_facilities == [0x01, 0x02, 0x05],
        "item-selling facility selectors changed: {selling_facilities:02X?}"
    );
    let non_selling_facilities = map_facilities
        .produced_selectors()
        .iter()
        .copied()
        .filter(|selector| !selling_facilities.contains(selector))
        .collect::<Vec<_>>();
    let non_selling_facilities: [u8; 2] = non_selling_facilities.try_into().map_err(
        |facilities: Vec<u8>| {
            anyhow::anyhow!(
                "shop lifetime no longer has exactly two non-selling facility selectors: {facilities:02X?}"
            )
        },
    )?;
    ensure!(
        non_selling_facilities == [0x03, 0x04],
        "non-selling facility selectors changed: {non_selling_facilities:02X?}"
    );
    ensure!(
        shop_handler == SHOP_ITEM_LIST_HANDLER,
        "item-selling facilities no longer enter the owned stock-list handler"
    );
    let (stock_group_ids, item_source_indices) =
        bind_shop_stock_sources(rom, &map_facilities, &selling_facilities)?;
    ensure!(
        SHOP_STATE_HANDLERS.get(usize::from(SHOP_ITEM_COMPOSITION_STATE))
            == Some(&SHOP_ITEM_COMPOSITION_HANDLER),
        "shop item-composition state no longer selects its owned handler"
    );
    let composition = switchable_slice(
        rom,
        0x06,
        SHOP_ITEM_COMPOSITION_HANDLER,
        SHOP_ITEM_COMPOSITION_BYTES.len(),
    )?;
    ensure!(
        composition == SHOP_ITEM_COMPOSITION_BYTES,
        "shop item-composition handler changed"
    );
    decode_rp2a03_sequence(
        composition,
        SHOP_ITEM_COMPOSITION_HANDLER,
        "shop item-composition handler",
    )?;

    Ok(ShopItemCompositionSource {
        outer_state_address: SHOP_OUTER_STATE_ADDRESS,
        composition_state: SHOP_ITEM_COMPOSITION_STATE,
        composite_state: SHOP_ITEM_COMPOSITE_STATE,
        selected_facility_address: SELECTED_FACILITY_ADDRESS,
        dialogue_directory_address: DIALOGUE_DIRECTORY_SELECTOR_ADDRESS,
        dialogue_directory_selector: SHOP_DIALOGUE_DIRECTORY_SELECTOR,
        selling_facilities,
        non_selling_facilities,
        stock_group_ids,
        item_source_indices,
    })
}

fn bind_shop_stock_sources(
    rom: &Rom,
    map_facilities: &crate::unit_ui_text::MapFacilityDispatchSource,
    selling_facilities: &[u8; 3],
) -> Result<(BTreeSet<u8>, BTreeSet<usize>)> {
    let handler = switchable_slice(
        rom,
        0x0B,
        SHOP_ITEM_LIST_HANDLER,
        SHOP_ITEM_LIST_HANDLER_BYTES.len(),
    )?;
    ensure!(
        handler == SHOP_ITEM_LIST_HANDLER_BYTES,
        "map facility stock-list handler changed"
    );
    decode_rp2a03_sequence(
        handler,
        SHOP_ITEM_LIST_HANDLER,
        "map facility stock-list handler",
    )?;

    let pointer_bytes = switchable_slice(
        rom,
        0x0B,
        SHOP_STOCK_POINTER_DIRECTORY_ADDRESS,
        SHOP_STOCK_GROUP_COUNT * 2,
    )?;
    let pointers = pointer_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        pointers.len() == SHOP_STOCK_GROUP_COUNT
            && pointers.first() == Some(&SHOP_STOCK_DATA_ADDRESS)
            && pointers.windows(2).all(|pair| pair[0] < pair[1])
            && pointers
                .last()
                .is_some_and(|pointer| *pointer < SHOP_STOCK_DATA_END),
        "shop stock pointer directory changed"
    );

    let stock_groups = pointers
        .iter()
        .enumerate()
        .map(|(index, start)| {
            let end = pointers
                .get(index + 1)
                .copied()
                .unwrap_or(SHOP_STOCK_DATA_END);
            let bytes = switchable_slice(
                rom,
                0x0B,
                *start,
                usize::from(
                    end.checked_sub(*start)
                        .context("shop stock range underflow")?,
                ),
            )?;
            ensure!(
                bytes.len() >= 2
                    && bytes.last() == Some(&SHOP_STOCK_TERMINATOR)
                    && !bytes[..bytes.len() - 1].contains(&SHOP_STOCK_TERMINATOR),
                "shop stock group {index} lost its exclusive terminator"
            );
            ensure!(
                bytes[..bytes.len() - 1]
                    .iter()
                    .all(|item_id| (1..=SHOP_ITEM_ENTRY_COUNT as u8).contains(item_id)),
                "shop stock group {index} selects outside the item-name population"
            );
            Ok(bytes[..bytes.len() - 1].to_vec())
        })
        .collect::<Result<Vec<_>>>()?;

    let stock_group_ids = map_facilities
        .records()
        .iter()
        .filter(|record| selling_facilities.contains(&record.facility_selector()))
        .map(|record| record.payload())
        .collect::<BTreeSet<_>>();
    let expected_stock_group_ids = (0_u8..=7).chain(9..=19).collect::<BTreeSet<_>>();
    ensure!(
        stock_group_ids == expected_stock_group_ids
            && stock_group_ids
                .iter()
                .all(|group| usize::from(*group) < stock_groups.len()),
        "selling facilities no longer cover the 19 owned stock groups: {stock_group_ids:02X?}"
    );
    let item_source_indices = stock_group_ids
        .iter()
        .flat_map(|group| stock_groups[usize::from(*group)].iter().copied())
        .map(|item_id| usize::from(item_id - 1))
        .collect::<BTreeSet<_>>();
    ensure!(
        item_source_indices.len() == 52,
        "selling facility stock domain changed from 52 item names to {}",
        item_source_indices.len()
    );
    Ok((stock_group_ids, item_source_indices))
}

#[derive(Debug)]
pub(crate) struct SharedMenuControllerSource {
    dispatch_call: u16,
    state_address: u16,
    handler_targets: [u16; 7],
}

impl SharedMenuControllerSource {
    pub(crate) fn dispatch_call(&self) -> u16 {
        self.dispatch_call
    }

    pub(crate) fn state_address(&self) -> u16 {
        self.state_address
    }

    pub(crate) fn handler_target(&self, state: u8) -> Option<u16> {
        self.handler_targets.get(usize::from(state)).copied()
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
        state_address: MENU_CONTROLLER_STATE_ADDRESS,
        handler_targets: MENU_CONTROLLER_HANDLERS,
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
