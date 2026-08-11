use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{font_slots::active_hangul_codes, rom::Rom, sha1_hex};

use super::source_window::source_bytes;

mod hp_bar;
mod queue;
mod source_regions;

use queue::{
    QueueCodeOwnership, add_queue, expected_global_preserved_codes, hex_codes,
    ownership_for_candidates,
};
use source_regions::{SOURCE_REGION_COUNT, bind_source_regions};

pub(super) const BATTLE_BANK_PUBLISH_SITES: [(u8, u16, u8); 17] = [
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

#[derive(Clone, Debug, Serialize)]
pub(super) struct BattleBackgroundPayloadModel {
    publisher_count: usize,
    source_region_count: usize,
    source_region_catalog_sha1: String,
    queue_template_count: usize,
    maximum_published_queue_byte_count: usize,
    hp_bar_queue_bound: hp_bar::BattleHpBarQueueBound,
    publisher_bindings: Vec<QueuePublisherPayloadBinding>,
    conservative_global_preserved_active_codes: Vec<String>,
    conservative_global_preserved_active_code_count: usize,
    conservative_global_preserved_active_codes_sha1: String,
    translation_owned_japanese_queue_codes: Vec<String>,
    translation_owned_japanese_queue_code_count: usize,
    payload_model_sha1: String,
    every_publisher_payload_source_bound: bool,
    global_preserved_code_union_complete: bool,
    exact_simultaneous_preserved_code_demand_proven: bool,
}

#[derive(Clone, Debug, Serialize)]
struct QueuePublisherPayloadBinding {
    bank_hex: String,
    publish_address_hex: String,
    role: &'static str,
    payload_model: &'static str,
    maximum_published_queue_byte_count: usize,
    potential_preserved_active_code_count: usize,
}

pub(super) fn bind_battle_background_payloads(rom: &Rom) -> Result<BattleBackgroundPayloadModel> {
    let source_region_catalog_sha1 = bind_source_regions(rom)?;
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let hp_bar_queue_bound = hp_bar::bind_hp_bar_queue_bound(rom)?;

    let mut exp_label = QueueCodeOwnership::default();
    add_queue(
        &mut exp_label,
        &[0x23, 0x2E, 0x03, 0x6E, 0x81, 0x79, 0x00],
        &active_codes,
    )?;

    let meter = ownership_for_candidates(0xE0..=0xE4, &active_codes);

    let wipe_data = source_bytes(rom, 0x05, 0x8596, 15)?;
    ensure!(
        wipe_data[..3] == [0x00, 0x01, 0x02],
        "battle wipe selector table changed"
    );
    let wipe_candidates = (0..3)
        .flat_map(|entry| {
            let start = 3 + entry * 4;
            [wipe_data[start], wipe_data[start + 1], wipe_data[start + 3]]
        })
        .collect::<Vec<_>>();
    let wipe = ownership_for_candidates(wipe_candidates, &active_codes);

    let unit_templates = source_bytes(rom, 0x05, 0x88BA, 140)?;
    let mut unit_panel = QueueCodeOwnership::default();
    for queue in [
        &unit_templates[0..45],
        &unit_templates[45..90],
        &unit_templates[90..115],
        &unit_templates[115..140],
    ] {
        add_queue(&mut unit_panel, queue, &active_codes)?;
    }

    let unit_stats = ownership_for_candidates(
        [0x75, 0x7F, 0xFF].into_iter().chain(0x60..=0x69),
        &active_codes,
    );
    let unit_marker = ownership_for_candidates([0x8D], &active_codes);
    let hp_bar = ownership_for_candidates(0xCB..=0xCF, &active_codes);

    let attribute_data = source_bytes(rom, 0x05, 0x8CF2, 44)?;
    ensure!(
        attribute_data[0] == 43,
        "battle attribute queue copy length changed"
    );
    let mut attribute_queue = attribute_data[1..].to_vec();
    attribute_queue.push(0);
    let mut attribute_and_latch = QueueCodeOwnership::default();
    add_queue(&mut attribute_and_latch, &attribute_queue, &active_codes)?;

    let animation_data = source_bytes(rom, 0x05, 0x953F, 78)?;
    let mut animation_border = QueueCodeOwnership::default();
    add_queue(&mut animation_border, &animation_data[4..41], &active_codes)?;
    add_queue(
        &mut animation_border,
        &animation_data[41..78],
        &active_codes,
    )?;

    let damage_digits =
        ownership_for_candidates([0xAE, 0xFF].into_iter().chain(0x60..=0x69), &active_codes);

    let mut critical_message = QueueCodeOwnership::default();
    add_queue(
        &mut critical_message,
        source_bytes(rom, 0x05, 0x96B6, 10)?,
        &active_codes,
    )?;

    let clear_data = source_bytes(rom, 0x05, 0xA1D4, 92)?;
    let mut animation_clear = QueueCodeOwnership::default();
    for queue in clear_data[8..].chunks_exact(21) {
        add_queue(&mut animation_clear, queue, &active_codes)?;
    }

    let effect_data = source_bytes(rom, 0x05, 0xAE3C, 52)?;
    let mut effect_overlay = QueueCodeOwnership::default();
    for record in effect_data[8..].chunks_exact(11) {
        ensure!(record[0] == 10, "effect overlay queue copy length changed");
        let mut queue = record[1..].to_vec();
        queue.push(0);
        add_queue(&mut effect_overlay, &queue, &active_codes)?;
    }

    let staggered_clear = ownership_for_candidates([0xFF], &active_codes);
    let mut animation_reset = QueueCodeOwnership::default();
    add_queue(
        &mut animation_reset,
        &[0x20, 0x4B, 0x02, 0xFF, 0x00, 0x00],
        &active_codes,
    )?;

    let dialogue_data = source_bytes(rom, 0x07, 0x804F, 137)?;
    ensure!(
        dialogue_data[8] == 0x20,
        "battle dialogue queue copy length changed"
    );
    let mut dialogue_box = QueueCodeOwnership::default();
    for queue in dialogue_data[9..].chunks_exact(32) {
        add_queue(&mut dialogue_box, queue, &active_codes)?;
    }

    let bindings = vec![
        binding(
            0x05,
            0x83F7,
            "experience_label",
            "literal_queue",
            7,
            &exp_label,
        ),
        binding(
            0x05,
            0x84CD,
            "experience_meter",
            "bounded_meter_helper",
            14,
            &meter,
        ),
        binding(
            0x05,
            0x8562,
            "battle_screen_wipe",
            "source_table_fill",
            23,
            &wipe,
        ),
        binding(
            0x05,
            0x88A3,
            "unit_panel_frame",
            "source_queue_templates",
            45,
            &unit_panel,
        ),
        binding(
            0x05,
            0x8A33,
            "unit_panel_stat_digits",
            "reserved_digit_domain",
            8,
            &unit_stats,
        ),
        binding(
            0x05,
            0x8AB4,
            "unit_panel_marker",
            "reserved_literal",
            5,
            &unit_marker,
        ),
        binding(
            0x05,
            0x8B9F,
            "unit_panel_hp_bar",
            "bounded_literal_domain",
            33,
            &hp_bar,
        ),
        binding(
            0x05,
            0x8C15,
            "unit_panel_meters",
            "bounded_meter_helper",
            40,
            &meter,
        ),
        binding(
            0x05,
            0x8CEC,
            "attribute_and_latch_init",
            "source_queue_template",
            44,
            &attribute_and_latch,
        ),
        binding(
            0x05,
            0x952F,
            "animation_border",
            "source_queue_templates",
            37,
            &animation_border,
        ),
        binding(
            0x05,
            0x968A,
            "damage_digits",
            "reserved_digits_plus_literal",
            7,
            &damage_digits,
        ),
        binding(
            0x05,
            0x96B0,
            "critical_message",
            "translation_owned_queue",
            10,
            &critical_message,
        ),
        binding(
            0x05,
            0x9C2E,
            "animation_clear",
            "source_queue_templates",
            21,
            &animation_clear,
        ),
        binding(
            0x05,
            0xAE39,
            "effect_overlay",
            "source_queue_templates",
            11,
            &effect_overlay,
        ),
        binding(
            0x05,
            0xAFE4,
            "staggered_clear",
            "reserved_fill",
            25,
            &staggered_clear,
        ),
        binding(
            0x05,
            0xB013,
            "animation_reset",
            "literal_queue",
            5,
            &animation_reset,
        ),
        binding(
            0x07,
            0x803D,
            "battle_dialogue_box",
            "source_queue_templates",
            32,
            &dialogue_box,
        ),
    ];
    ensure!(
        bindings.len() == BATTLE_BANK_PUBLISH_SITES.len(),
        "battle payload binding count changed"
    );
    ensure!(
        bindings
            .iter()
            .zip(BATTLE_BANK_PUBLISH_SITES)
            .all(|(binding, site)| {
                binding.bank_hex == format!("0x{:02X}", site.0)
                    && binding.publish_address_hex == format!("0x{:04X}", site.1)
            }),
        "battle payload bindings no longer cover the publisher census in order"
    );
    let maximum_published_queue_byte_count = bindings
        .iter()
        .map(|binding| binding.maximum_published_queue_byte_count)
        .max()
        .context("battle payload binding set is empty")?;
    ensure!(
        maximum_published_queue_byte_count == 45,
        "battle background queue bound changed"
    );

    let ownerships = [
        &exp_label,
        &meter,
        &wipe,
        &unit_panel,
        &unit_stats,
        &unit_marker,
        &hp_bar,
        &attribute_and_latch,
        &animation_border,
        &damage_digits,
        &critical_message,
        &animation_clear,
        &effect_overlay,
        &staggered_clear,
        &animation_reset,
        &dialogue_box,
    ];
    let preserved_active = ownerships
        .iter()
        .flat_map(|ownership| ownership.preserved_active.iter().copied())
        .collect::<BTreeSet<_>>();
    let japanese_active = ownerships
        .iter()
        .flat_map(|ownership| ownership.japanese_active.iter().copied())
        .collect::<BTreeSet<_>>();
    ensure!(
        preserved_active == expected_global_preserved_codes(),
        "battle background preserved-code union changed"
    );
    ensure!(
        preserved_active.is_disjoint(&japanese_active),
        "battle background payload ownership overlaps"
    );

    let preserved_bytes = preserved_active.iter().copied().collect::<Vec<_>>();
    let japanese_bytes = japanese_active.iter().copied().collect::<Vec<_>>();
    let mut model_bytes = Vec::new();
    model_bytes.extend_from_slice(source_region_catalog_sha1.as_bytes());
    for (binding, (_, address, _)) in bindings.iter().zip(BATTLE_BANK_PUBLISH_SITES) {
        model_bytes.extend_from_slice(&address.to_le_bytes());
        model_bytes.extend_from_slice(binding.role.as_bytes());
        model_bytes.push(0);
        model_bytes.extend_from_slice(binding.payload_model.as_bytes());
        model_bytes.push(0);
        model_bytes.extend_from_slice(
            &(binding.potential_preserved_active_code_count as u64).to_le_bytes(),
        );
        model_bytes
            .extend_from_slice(&(binding.maximum_published_queue_byte_count as u64).to_le_bytes());
    }
    model_bytes.extend_from_slice(&preserved_bytes);
    model_bytes.extend_from_slice(&japanese_bytes);

    Ok(BattleBackgroundPayloadModel {
        publisher_count: bindings.len(),
        source_region_count: SOURCE_REGION_COUNT,
        source_region_catalog_sha1,
        queue_template_count: 22,
        maximum_published_queue_byte_count,
        hp_bar_queue_bound,
        publisher_bindings: bindings,
        conservative_global_preserved_active_codes: hex_codes(&preserved_active),
        conservative_global_preserved_active_code_count: preserved_active.len(),
        conservative_global_preserved_active_codes_sha1: sha1_hex(&preserved_bytes),
        translation_owned_japanese_queue_codes: hex_codes(&japanese_active),
        translation_owned_japanese_queue_code_count: japanese_active.len(),
        payload_model_sha1: sha1_hex(&model_bytes),
        every_publisher_payload_source_bound: true,
        global_preserved_code_union_complete: true,
        exact_simultaneous_preserved_code_demand_proven: false,
    })
}

impl BattleBackgroundPayloadModel {
    pub(super) fn conservative_preserved_active_codes(&self) -> BTreeSet<u8> {
        expected_global_preserved_codes()
    }

    pub(super) fn every_publisher_payload_source_bound(&self) -> bool {
        self.every_publisher_payload_source_bound
    }

    pub(super) fn maximum_published_queue_byte_count(&self) -> usize {
        self.maximum_published_queue_byte_count
    }

    #[cfg(test)]
    pub(super) fn test_model() -> Self {
        let preserved = expected_global_preserved_codes();
        let preserved_bytes = preserved.iter().copied().collect::<Vec<_>>();
        Self {
            publisher_count: 0,
            source_region_count: 0,
            source_region_catalog_sha1: String::new(),
            queue_template_count: 0,
            maximum_published_queue_byte_count: 0,
            hp_bar_queue_bound: hp_bar::test_model(),
            publisher_bindings: Vec::new(),
            conservative_global_preserved_active_codes: hex_codes(&preserved),
            conservative_global_preserved_active_code_count: preserved.len(),
            conservative_global_preserved_active_codes_sha1: sha1_hex(&preserved_bytes),
            translation_owned_japanese_queue_codes: Vec::new(),
            translation_owned_japanese_queue_code_count: 0,
            payload_model_sha1: String::new(),
            every_publisher_payload_source_bound: false,
            global_preserved_code_union_complete: false,
            exact_simultaneous_preserved_code_demand_proven: false,
        }
    }
}

fn binding(
    bank: u8,
    address: u16,
    role: &'static str,
    payload_model: &'static str,
    maximum_published_queue_byte_count: usize,
    ownership: &QueueCodeOwnership,
) -> QueuePublisherPayloadBinding {
    QueuePublisherPayloadBinding {
        bank_hex: format!("0x{bank:02X}"),
        publish_address_hex: format!("0x{address:04X}"),
        role,
        payload_model,
        maximum_published_queue_byte_count,
        potential_preserved_active_code_count: ownership.preserved_active.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_decoder_excludes_attribute_bytes_and_reserved_codes() {
        let active = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
        let queue = [
            0x23, 0xBF, 0x03, 0xAE, 0xAF, 0xB0, 0x20, 0x10, 0x42, 0xC0, 0x00,
        ];
        let mut ownership = QueueCodeOwnership::default();
        add_queue(&mut ownership, &queue, &active).unwrap();

        assert_eq!(ownership.preserved_active, BTreeSet::from([0xAE, 0xC0]));
        assert!(ownership.japanese_active.is_empty());
    }

    #[test]
    fn queue_decoder_keeps_japanese_codes_translation_owned() {
        let active = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
        let queue = [0x20, 0x00, 0x03, 0x00, 0x8C, 0xAE, 0x00];
        let mut ownership = QueueCodeOwnership::default();
        add_queue(&mut ownership, &queue, &active).unwrap();

        assert_eq!(ownership.japanese_active, BTreeSet::from([0x00]));
        assert_eq!(ownership.preserved_active, BTreeSet::from([0x8C, 0xAE]));
    }

    #[test]
    fn global_preserved_union_is_smaller_than_the_whole_active_page() {
        let union = expected_global_preserved_codes();

        assert_eq!(union.len(), 39);
        assert!(union.is_subset(&active_hangul_codes().into_iter().collect()));
    }
}
