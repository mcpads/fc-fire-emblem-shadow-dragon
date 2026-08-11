use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use serde::Serialize;

#[cfg(test)]
use crate::{font_slots::active_hangul_codes, japanese_encoding::is_japanese_text_code};
use crate::{
    rom::Rom, sha1_hex, source_font_page::bind_source_font_page_ownership,
    typed_source::decode_rp2a03_sequence,
};

use super::{
    background_payloads::{
        BATTLE_BANK_PUBLISH_SITES, BattleBackgroundPayloadModel, bind_battle_background_payloads,
    },
    phase_cooccurrence::{
        ANIMATION_PHASE_POINTERS, PRIMARY_PHASE_POINTERS, UNIT_PANEL_PHASE_POINTERS,
    },
    source_window::{prg_bank, source_bytes},
};

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
    payload_model: BattleBackgroundPayloadModel,
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
    pub(super) every_publisher_payload_source_bound: bool,
    pub(super) conservative_global_preserved_code_union_complete: bool,
    pub(super) simultaneous_preserved_code_demand_complete: bool,
}

pub(super) struct ObservedBattleBackgroundCodes {
    pub(super) japanese_text_codes: BTreeSet<u8>,
    pub(super) preserved_non_japanese_codes: BTreeSet<u8>,
}

pub(super) fn bind_battle_background_code_ownership(
    rom: &Rom,
) -> Result<BattleBackgroundCodeOwnership> {
    let payload_model = bind_battle_background_payloads(rom)?;
    let producer_topology = bind_battle_background_producer_topology(rom, &payload_model)?;
    let source_page = bind_source_font_page_ownership(rom)?;
    let active_codes = source_page.active_codes().clone();
    let japanese_text_codes = source_page.japanese_text_codes().clone();
    let preserved_non_japanese_codes = source_page.preserved_non_japanese_codes().clone();

    Ok(BattleBackgroundCodeOwnership {
        active_codes,
        japanese_text_codes,
        preserved_non_japanese_codes,
        producer_topology,
        payload_model,
    })
}

impl BattleBackgroundCodeOwnership {
    pub(super) fn source_font_page_sha1(&self) -> &'static str {
        crate::source_font_page::SOURCE_FONT_PAGE_SHA1
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

    pub(super) fn payload_model(&self) -> BattleBackgroundPayloadModel {
        self.payload_model.clone()
    }

    pub(super) fn conservative_global_preserved_active_codes(&self) -> BTreeSet<u8> {
        self.payload_model.conservative_preserved_active_codes()
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

fn bind_battle_background_producer_topology(
    rom: &Rom,
    payload_model: &BattleBackgroundPayloadModel,
) -> Result<BattleBackgroundProducerTopology> {
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
        every_publisher_payload_source_bound: payload_model.every_publisher_payload_source_bound(),
        conservative_global_preserved_code_union_complete: true,
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
            payload_model: BattleBackgroundPayloadModel::test_model(),
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
                every_publisher_payload_source_bound: false,
                conservative_global_preserved_code_union_complete: false,
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
