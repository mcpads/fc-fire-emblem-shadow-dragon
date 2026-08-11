use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::{FONT_PAGE_SIZE, active_hangul_codes},
    japanese_encoding::is_japanese_text_code,
    rom::Rom,
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

const SOURCE_FONT_PAGE_INDEX: usize = 0;
const SOURCE_FONT_PAGE_SHA1: &str = "1860feeb0b0b216abb79bf7917bde8b51734a980";
const PRG_BANK_SIZE: usize = 16 * 1024;
const PRIMARY_PHASE_POINTERS: [u16; 32] = [
    0x82C7, 0x8830, 0x8C5D, 0x8830, 0x8C5D, 0x881C, 0x8827, 0x8CD3, 0x9304, 0x82F1, 0x8505, 0x8522,
    0x85DE, 0x8341, 0x83FD, 0x8522, 0x83D3, 0x8467, 0x8475, 0x8353, 0x84D5, 0x8522, 0x837F, 0x8341,
    0x86E1, 0x8725, 0x8250, 0x829C, 0x82E9, 0x835B, 0x8368, 0x81A9,
];
const UNIT_PANEL_PHASE_POINTERS: [u16; 12] = [
    0x884E, 0x8874, 0x8863, 0x8946, 0x89AA, 0x89D7, 0x8A39, 0x8A64, 0x8A94, 0x8AD8, 0x8BA5, 0x8852,
];
const ANIMATION_PHASE_POINTERS: [u16; 41] = [
    0x97A1, 0x936C, 0x93E9, 0x9435, 0x943C, 0x945D, 0x9495, 0x94C4, 0x98D5, 0x98D9, 0x97CF, 0x97E8,
    0x95C3, 0x9603, 0x962D, 0x963F, 0x9648, 0x96A3, 0x96C0, 0x950B, 0x958D, 0x9596, 0x9620, 0x9717,
    0x972A, 0x99B7, 0x9801, 0x97EF, 0x984D, 0x98D5, 0x98D9, 0x9829, 0x97EF, 0xA059, 0x98D5, 0xAE70,
    0xAE87, 0xAED2, 0x98D9, 0xAEEB, 0x8830,
];
const BATTLE_BANK_PUBLISH_SITES: [(u8, u16, u8); 17] = [
    (0x05, 0x83F7, 0x85),
    (0x05, 0x84CD, 0x85),
    (0x05, 0x8562, 0x85),
    (0x05, 0x88A3, 0x85),
    (0x05, 0x8A33, 0x85),
    (0x05, 0x8AB4, 0x86),
    (0x05, 0x8B9F, 0x85),
    (0x05, 0x8C15, 0x85),
    (0x05, 0x8CEC, 0x85),
    (0x05, 0x952F, 0x85),
    (0x05, 0x968A, 0x86),
    (0x05, 0x96B0, 0x85),
    (0x05, 0x9C2E, 0x85),
    (0x05, 0xAE39, 0x85),
    (0x05, 0xAFE4, 0x85),
    (0x05, 0xB013, 0x86),
    (0x07, 0x803D, 0x85),
];
const QUEUE_CONSUMER_ADDRESS: u16 = 0xC3A5;
const QUEUE_CONSUMER_BYTES: [u8; 26] = [
    0xA5, 0x21, 0xF0, 0x15, 0xA9, 0x81, 0x85, 0x00, 0xA9, 0x07, 0x85, 0x01, 0x20, 0xE7, 0xC3, 0xA9,
    0x00, 0x8D, 0x80, 0x07, 0x8D, 0x81, 0x07, 0x85, 0x21, 0x60,
];

pub(super) struct BattleBackgroundCodeOwnership {
    active_codes: BTreeSet<u8>,
    japanese_text_codes: BTreeSet<u8>,
    preserved_non_japanese_codes: BTreeSet<u8>,
    producer_topology: BattleBackgroundProducerTopology,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct BattleBackgroundProducerTopology {
    pub(super) primary_phase_count: usize,
    pub(super) primary_distinct_handler_count: usize,
    pub(super) unit_panel_phase_count: usize,
    pub(super) animation_phase_count: usize,
    pub(super) battle_switchable_bank_count: usize,
    pub(super) queue_publish_site_count: usize,
    pub(super) queue_publish_sites_sha1: String,
    pub(super) direct_ppu_data_store_count: usize,
    pub(super) queue_ready_address_hex: &'static str,
    pub(super) queue_buffer_address_hex: &'static str,
    pub(super) queue_consumer_address_hex: String,
    pub(super) every_primary_phase_source_bound: bool,
    pub(super) every_nested_phase_source_bound: bool,
    pub(super) every_battle_bank_queue_publisher_classified: bool,
    pub(super) battle_banks_have_no_direct_ppu_data_stores: bool,
    pub(super) producer_topology_complete: bool,
    pub(super) simultaneous_preserved_code_demand_complete: bool,
}

pub(super) struct ObservedBattleBackgroundCodes {
    pub(super) japanese_text_codes: BTreeSet<u8>,
    pub(super) preserved_non_japanese_codes: BTreeSet<u8>,
}

pub(super) fn bind_battle_background_code_ownership(
    rom: &Rom,
) -> Result<BattleBackgroundCodeOwnership> {
    let producer_topology = bind_battle_background_producer_topology(rom)?;
    let page_start = SOURCE_FONT_PAGE_INDEX
        .checked_mul(FONT_PAGE_SIZE)
        .context("battle source-font page offset overflow")?;
    let page = rom
        .chr()
        .get(page_start..page_start + FONT_PAGE_SIZE)
        .context("battle source-font page is outside CHR")?;
    ensure!(
        sha1_hex(page) == SOURCE_FONT_PAGE_SHA1,
        "battle source-font page changed"
    );

    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let japanese_text_codes = active_codes
        .iter()
        .copied()
        .filter(|code| is_japanese_text_code(*code))
        .collect::<BTreeSet<_>>();
    let preserved_non_japanese_codes = active_codes
        .difference(&japanese_text_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    ensure!(
        japanese_text_codes.is_disjoint(&preserved_non_japanese_codes),
        "battle background code ownership overlaps"
    );
    ensure!(
        japanese_text_codes
            .union(&preserved_non_japanese_codes)
            .copied()
            .collect::<BTreeSet<_>>()
            == active_codes,
        "battle background code ownership does not cover every active code"
    );

    Ok(BattleBackgroundCodeOwnership {
        active_codes,
        japanese_text_codes,
        preserved_non_japanese_codes,
        producer_topology,
    })
}

impl BattleBackgroundCodeOwnership {
    pub(super) fn source_font_page_sha1(&self) -> &'static str {
        SOURCE_FONT_PAGE_SHA1
    }

    pub(super) fn japanese_text_active_code_count(&self) -> usize {
        self.japanese_text_codes.len()
    }

    pub(super) fn preserved_non_japanese_active_code_count(&self) -> usize {
        self.preserved_non_japanese_codes.len()
    }

    pub(super) fn producer_topology(&self) -> BattleBackgroundProducerTopology {
        self.producer_topology.clone()
    }

    pub(super) fn partition_observed(
        &self,
        observed_active_codes: &BTreeSet<u8>,
    ) -> Result<ObservedBattleBackgroundCodes> {
        ensure!(
            observed_active_codes.is_subset(&self.active_codes),
            "observed battle background contains a reserved code in its active-code set"
        );
        let japanese_text_codes = observed_active_codes
            .intersection(&self.japanese_text_codes)
            .copied()
            .collect::<BTreeSet<_>>();
        let preserved_non_japanese_codes = observed_active_codes
            .intersection(&self.preserved_non_japanese_codes)
            .copied()
            .collect::<BTreeSet<_>>();
        ensure!(
            japanese_text_codes
                .union(&preserved_non_japanese_codes)
                .copied()
                .collect::<BTreeSet<_>>()
                == *observed_active_codes,
            "observed battle background ownership lost active codes"
        );
        Ok(ObservedBattleBackgroundCodes {
            japanese_text_codes,
            preserved_non_japanese_codes,
        })
    }
}

fn bind_battle_background_producer_topology(rom: &Rom) -> Result<BattleBackgroundProducerTopology> {
    bind_dispatcher(
        rom,
        0x05,
        0x81EC,
        &[0xAD, 0x7C, 0x04, 0x20, 0x4C, 0xC3],
        0x81F2,
        &PRIMARY_PHASE_POINTERS,
        "shared battle primary phase",
    )?;
    bind_dispatcher(
        rom,
        0x05,
        0x8830,
        &[0xAD, 0x76, 0x03, 0x20, 0x4C, 0xC3],
        0x8836,
        &UNIT_PANEL_PHASE_POINTERS,
        "battle unit-panel phase",
    )?;
    bind_dispatcher(
        rom,
        0x05,
        0x9314,
        &[0xAD, 0xBF, 0x03, 0x20, 0x4C, 0xC3],
        0x931A,
        &ANIMATION_PHASE_POINTERS,
        "battle animation phase",
    )?;

    let mut actual_publish_sites = Vec::new();
    let mut direct_ppu_data_store_count = 0;
    for bank in [0x05, 0x07] {
        let bytes = prg_bank(rom, bank)?;
        for offset in 0..bytes.len().saturating_sub(1) {
            if matches!(bytes[offset], 0x84..=0x86) && bytes[offset + 1] == 0x21 {
                actual_publish_sites.push((bank, 0x8000 + offset as u16, bytes[offset]));
            }
        }
        direct_ppu_data_store_count += bytes
            .windows(3)
            .filter(|bytes| *bytes == [0x8D, 0x07, 0x20])
            .count();
    }
    ensure!(
        actual_publish_sites == BATTLE_BANK_PUBLISH_SITES,
        "battle background queue publisher population changed"
    );
    ensure!(
        direct_ppu_data_store_count == 0,
        "battle switchable banks gained a direct PPU data store"
    );
    for (bank, address, opcode) in BATTLE_BANK_PUBLISH_SITES {
        let instruction = source_bytes(rom, bank, address, 2)?;
        ensure!(
            instruction == [opcode, 0x21],
            "battle queue publisher {bank:02X}:${address:04X} changed"
        );
        decode_rp2a03_sequence(instruction, address, "battle queue publisher")?;
    }

    let queue_consumer = source_bytes(
        rom,
        0x0F,
        QUEUE_CONSUMER_ADDRESS,
        QUEUE_CONSUMER_BYTES.len(),
    )?;
    ensure!(
        queue_consumer == QUEUE_CONSUMER_BYTES,
        "battle background queue consumer changed"
    );
    decode_rp2a03_sequence(
        queue_consumer,
        QUEUE_CONSUMER_ADDRESS,
        "consume published PPU queue",
    )?;

    let mut publish_catalog_bytes = Vec::with_capacity(BATTLE_BANK_PUBLISH_SITES.len() * 4);
    for (bank, address, opcode) in BATTLE_BANK_PUBLISH_SITES {
        publish_catalog_bytes.push(bank);
        publish_catalog_bytes.extend_from_slice(&address.to_le_bytes());
        publish_catalog_bytes.push(opcode);
    }
    Ok(BattleBackgroundProducerTopology {
        primary_phase_count: PRIMARY_PHASE_POINTERS.len(),
        primary_distinct_handler_count: PRIMARY_PHASE_POINTERS
            .into_iter()
            .collect::<BTreeSet<_>>()
            .len(),
        unit_panel_phase_count: UNIT_PANEL_PHASE_POINTERS.len(),
        animation_phase_count: ANIMATION_PHASE_POINTERS.len(),
        battle_switchable_bank_count: 2,
        queue_publish_site_count: BATTLE_BANK_PUBLISH_SITES.len(),
        queue_publish_sites_sha1: sha1_hex(&publish_catalog_bytes),
        direct_ppu_data_store_count,
        queue_ready_address_hex: "0x0021",
        queue_buffer_address_hex: "0x0781",
        queue_consumer_address_hex: format!("0x{QUEUE_CONSUMER_ADDRESS:04X}"),
        every_primary_phase_source_bound: true,
        every_nested_phase_source_bound: true,
        every_battle_bank_queue_publisher_classified: true,
        battle_banks_have_no_direct_ppu_data_stores: true,
        producer_topology_complete: true,
        simultaneous_preserved_code_demand_complete: false,
    })
}

fn bind_dispatcher(
    rom: &Rom,
    bank: u8,
    dispatcher_address: u16,
    expected_dispatcher: &[u8],
    table_address: u16,
    expected_pointers: &[u16],
    role: &str,
) -> Result<()> {
    let dispatcher = source_bytes(rom, bank, dispatcher_address, expected_dispatcher.len())?;
    ensure!(
        dispatcher == expected_dispatcher,
        "{role} dispatcher changed"
    );
    decode_rp2a03_sequence(dispatcher, dispatcher_address, role)?;
    let table = source_bytes(rom, bank, table_address, expected_pointers.len() * 2)?;
    let pointers = table
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        pointers == expected_pointers,
        "{role} pointer table changed"
    );
    ensure!(
        pointers
            .iter()
            .all(|address| (0x8000..0xC000).contains(address)),
        "{role} contains a handler outside its switchable bank"
    );
    Ok(())
}

fn source_bytes(rom: &Rom, bank: u8, address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(
        address >= 0x8000,
        "source address ${address:04X} is below the PRG window"
    );
    let bank_offset = if bank == 0x0F {
        usize::from(
            address
                .checked_sub(0xC000)
                .context("fixed-bank source address is below the fixed CPU window")?,
        )
    } else {
        usize::from(address - 0x8000)
    };
    let start = usize::from(bank)
        .checked_mul(PRG_BANK_SIZE)
        .and_then(|offset| offset.checked_add(bank_offset))
        .context("source PRG offset overflow")?;
    rom.prg()
        .get(start..start + byte_count)
        .with_context(|| format!("source {bank:02X}:${address:04X} is outside PRG"))
}

fn prg_bank(rom: &Rom, bank: u8) -> Result<&[u8]> {
    let start = usize::from(bank)
        .checked_mul(PRG_BANK_SIZE)
        .context("source PRG bank offset overflow")?;
    rom.prg()
        .get(start..start + PRG_BANK_SIZE)
        .with_context(|| format!("source PRG bank {bank:02X} is absent"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_japanese_glyphs_are_translation_owned_not_graphics() {
        let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
        let japanese_text_codes = active_codes
            .iter()
            .copied()
            .filter(|code| is_japanese_text_code(*code))
            .collect::<BTreeSet<_>>();
        let ownership = BattleBackgroundCodeOwnership {
            active_codes,
            japanese_text_codes: japanese_text_codes.clone(),
            preserved_non_japanese_codes: active_hangul_codes()
                .into_iter()
                .filter(|code| !is_japanese_text_code(*code))
                .collect(),
            producer_topology: BattleBackgroundProducerTopology {
                primary_phase_count: 0,
                primary_distinct_handler_count: 0,
                unit_panel_phase_count: 0,
                animation_phase_count: 0,
                battle_switchable_bank_count: 0,
                queue_publish_site_count: 0,
                queue_publish_sites_sha1: String::new(),
                direct_ppu_data_store_count: 0,
                queue_ready_address_hex: "0x0021",
                queue_buffer_address_hex: "0x0781",
                queue_consumer_address_hex: "0xC3A5".to_owned(),
                every_primary_phase_source_bound: false,
                every_nested_phase_source_bound: false,
                every_battle_bank_queue_publisher_classified: false,
                battle_banks_have_no_direct_ppu_data_stores: false,
                producer_topology_complete: false,
                simultaneous_preserved_code_demand_complete: false,
            },
        };
        let observed = BTreeSet::from([0x00, 0x5F, 0x8C, 0xA6, 0xB0]);
        let partition = ownership.partition_observed(&observed).unwrap();

        assert_eq!(
            partition.japanese_text_codes,
            BTreeSet::from([0x00, 0x5F, 0xA6])
        );
        assert_eq!(
            partition.preserved_non_japanese_codes,
            BTreeSet::from([0x8C, 0xB0])
        );
    }
}
