use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    text_inventory::FixedTextPlan,
    typed_source::decode_rp2a03_sequence,
};

use super::{enemy_domain::EnemyParticipantInput, item_domain::PlayerParticipantInput};

const PRG_BANK_SIZE: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const SWITCHABLE_CPU_END_EXCLUSIVE: u16 = 0xC000;
const SOUND_TEST_PRG_BANK: u8 = 0x07;

const INITIALIZE_PAIRING_CPU_ADDRESS: u16 = 0xAA5F;
const INITIALIZE_PAIRING_BYTE_COUNT: usize = 0x23;
const INITIALIZE_PAIRING_SHA1: &str = "ec74a0ef9c91d41ee9eab5884b5fea87bac026f5";
const RANDOM_INPUT_CPU_ADDRESS: u16 = 0xAA89;
const RANDOM_INPUT_BYTE_COUNT: usize = 0x84;
const RANDOM_INPUT_SHA1: &str = "5aaf964a733ca52b296bfe441dbff15c3f019038";
const SELECT_PAIRING_CPU_ADDRESS: u16 = 0xAB0D;
const SELECT_PAIRING_BYTE_COUNT: usize = 0x94;
const SELECT_PAIRING_SHA1: &str = "85f547dbf304136c6156478c5da45ea293cd67dc";
const REPLAY_PAIRING_CPU_ADDRESS: u16 = 0xABB8;
const REPLAY_PAIRING_BYTE_COUNT: usize = 0x52;
const REPLAY_PAIRING_SHA1: &str = "41f213966ffb1e4de01c840b2560b6f24702ae2f";

const CLASS_ITEM_POINTER_TABLE_CPU_ADDRESS: u16 = 0xAC44;
const CLASS_ITEM_POINTER_COUNT: usize = 24;
const CLASS_ITEM_POINTER_TABLE_SHA1: &str = "f44372aeff22f096149bab376b9e7655ea3b7a16";
const CLASS_ITEM_LIST_STORAGE_CPU_ADDRESS: u16 = 0xAC74;
const CLASS_ITEM_LIST_STORAGE_BYTE_COUNT: usize = 0xD8;
const CLASS_ITEM_LIST_STORAGE_SHA1: &str = "cf5552cb31cb408ba7b7addc1afd35133fde428f";

const UNIT_IDENTITY_MINIMUM: u8 = 1;
const UNIT_IDENTITY_MAXIMUM: u8 = 16;
const ENEMY_IDENTITY_MINIMUM: u8 = 1;
const ENEMY_IDENTITY_MAXIMUM: u8 = 8;
const CLASS_ID_MINIMUM: u8 = 1;
const CLASS_ID_MAXIMUM: u8 = 23;
const TERRAIN_SOURCE_INDEX_MINIMUM: u8 = 0;
const TERRAIN_SOURCE_INDEX_MAXIMUM: u8 = 15;
const ITEM_ENTRY_COUNT: usize = 91;
const CLASS_ITEM_PAIR_COUNT: usize = 193;
const CLASS_ITEM_PAIR_SHA1: &str = "36b2894df25b2a47333e12228b3e6804e7d30007";
const UNIQUE_ITEM_SOURCE_INDEX_COUNT: usize = 55;
const PLAYER_PARTICIPANT_COUNT: usize = CLASS_ITEM_PAIR_COUNT * 16;
const ENEMY_PARTICIPANT_COUNT: usize = CLASS_ITEM_PAIR_COUNT * 8;

const RANDOM_NAME_FRAGMENT: &[u8] = &[
    0x20, 0x4E, 0xC0, 0x29, 0x0F, 0x18, 0x69, 0x01, 0x8D, 0x04, 0x03, 0x20, 0x4E, 0xC0, 0x29, 0x07,
    0x18, 0x69, 0x01, 0x8D, 0x05, 0x03,
];
const RANDOM_TERRAIN_FRAGMENT: &[u8] = &[
    0xA9, 0x00, 0x8D, 0x02, 0x03, 0x20, 0x4E, 0xC0, 0x48, 0x29, 0x0F, 0x8D, 0x22, 0x03, 0x68, 0x29,
    0xF0, 0x4A, 0x4A, 0x4A, 0x4A, 0x8D, 0x23, 0x03,
];
const FIRST_PAIRING_FRAGMENT: &[u8] = &[
    0xAD, 0x31, 0x77, 0x8D, 0x06, 0x03, 0x0A, 0xAA, 0xBD, 0x44, 0xAC, 0x85, 0x00, 0xBD, 0x45, 0xAC,
    0x85, 0x01, 0xAC, 0x33, 0x77, 0xB1, 0x00,
];
const SECOND_PAIRING_FRAGMENT: &[u8] = &[
    0xAD, 0x32, 0x77, 0x8D, 0x07, 0x03, 0x0A, 0xAA, 0xBD, 0x44, 0xAC, 0x85, 0x00, 0xBD, 0x45, 0xAC,
    0x85, 0x01, 0xAC, 0x34, 0x77, 0xB1, 0x00,
];
const REPLAY_SELECTED_PAIRING_FRAGMENT: &[u8] = &[
    0x20, 0x89, 0xAA, 0xAD, 0x35, 0x77, 0x8D, 0x20, 0x03, 0xAD, 0x36, 0x77, 0x8D, 0x21, 0x03, 0xAD,
    0x37, 0x77, 0x8D, 0x06, 0x03, 0xAD, 0x38, 0x77, 0x8D, 0x07, 0x03,
];

pub(super) struct SoundTestBattleDomain {
    pub(super) player_participant_glyph_sets: Vec<BTreeSet<char>>,
    pub(super) player_participant_inputs: Vec<PlayerParticipantInput>,
    pub(super) enemy_participant_glyph_sets: Vec<BTreeSet<char>>,
    pub(super) enemy_participant_inputs: Vec<EnemyParticipantInput>,
    pub(super) enemy_name_source_indices: BTreeSet<usize>,
    pub(super) item_source_indices: BTreeSet<usize>,
    pub(super) binding: SoundTestBattleDomainBinding,
}

#[derive(Debug, Serialize)]
pub(super) struct SoundTestBattleDomainBinding {
    initialize_pairing: SourceRoutineBinding,
    random_input: SourceRoutineBinding,
    select_pairing: SourceRoutineBinding,
    replay_pairing: SourceRoutineBinding,
    class_item_pointer_table: SourceTableBinding,
    class_item_list_storage: SourceTableBinding,
    unit_identity_minimum: u8,
    unit_identity_maximum: u8,
    enemy_identity_minimum: u8,
    enemy_identity_maximum: u8,
    enemy_identity_high_bit_required: bool,
    class_id_minimum: u8,
    class_id_maximum: u8,
    terrain_source_index_minimum: u8,
    terrain_source_index_maximum: u8,
    class_item_pair_count: usize,
    class_item_pair_sha1: String,
    unique_item_source_index_count: usize,
    player_participant_candidate_count: usize,
    enemy_participant_candidate_count: usize,
    class_items_are_direct_name_source_indices: bool,
    random_names_replayed_with_selected_class_items: bool,
    participant_role_is_owned_by_staging_position: bool,
    source_domain_complete: bool,
}

#[derive(Debug, Serialize)]
struct SourceRoutineBinding {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
    source_sha1: String,
    typed_instruction_count: usize,
}

#[derive(Debug, Serialize)]
struct SourceTableBinding {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
    source_sha1: String,
}

#[derive(Clone, Copy)]
struct RoutineSpec {
    role: &'static str,
    cpu_address: u16,
    byte_count: usize,
    expected_sha1: &'static str,
}

pub(super) fn bind_sound_test_battle_domain(
    rom: &Rom,
    fixed: &FixedTextPlan,
) -> Result<SoundTestBattleDomain> {
    let initialize_pairing = bind_routine(
        rom,
        RoutineSpec {
            role: "initialize_sound_test_battle_pairing",
            cpu_address: INITIALIZE_PAIRING_CPU_ADDRESS,
            byte_count: INITIALIZE_PAIRING_BYTE_COUNT,
            expected_sha1: INITIALIZE_PAIRING_SHA1,
        },
    )?;
    let random_input = bind_routine(
        rom,
        RoutineSpec {
            role: "compose_random_sound_test_battle_inputs",
            cpu_address: RANDOM_INPUT_CPU_ADDRESS,
            byte_count: RANDOM_INPUT_BYTE_COUNT,
            expected_sha1: RANDOM_INPUT_SHA1,
        },
    )?;
    let select_pairing = bind_routine(
        rom,
        RoutineSpec {
            role: "select_sound_test_class_item_pairing",
            cpu_address: SELECT_PAIRING_CPU_ADDRESS,
            byte_count: SELECT_PAIRING_BYTE_COUNT,
            expected_sha1: SELECT_PAIRING_SHA1,
        },
    )?;
    let replay_pairing = bind_routine(
        rom,
        RoutineSpec {
            role: "replay_sound_test_class_item_pairing",
            cpu_address: REPLAY_PAIRING_CPU_ADDRESS,
            byte_count: REPLAY_PAIRING_BYTE_COUNT,
            expected_sha1: REPLAY_PAIRING_SHA1,
        },
    )?;

    let random_source = source_slice(rom, RANDOM_INPUT_CPU_ADDRESS, RANDOM_INPUT_BYTE_COUNT)?;
    ensure!(
        contains_fragment(random_source, RANDOM_NAME_FRAGMENT),
        "sound-test random input no longer produces the two participant-name ranges"
    );
    ensure!(
        contains_fragment(random_source, RANDOM_TERRAIN_FRAGMENT),
        "sound-test random input no longer produces both terrain-name ranges"
    );
    let pairing_source = source_slice(rom, SELECT_PAIRING_CPU_ADDRESS, SELECT_PAIRING_BYTE_COUNT)?;
    ensure!(
        contains_fragment(pairing_source, FIRST_PAIRING_FRAGMENT),
        "sound-test pairing no longer projects the first class and item"
    );
    ensure!(
        contains_fragment(pairing_source, SECOND_PAIRING_FRAGMENT),
        "sound-test pairing no longer projects the second class and item"
    );
    let replay_source = source_slice(rom, REPLAY_PAIRING_CPU_ADDRESS, REPLAY_PAIRING_BYTE_COUNT)?;
    ensure!(
        contains_fragment(replay_source, REPLAY_SELECTED_PAIRING_FRAGMENT),
        "sound-test replay no longer combines random names with the selected class-item pair"
    );

    let pointer_bytes = source_slice(
        rom,
        CLASS_ITEM_POINTER_TABLE_CPU_ADDRESS,
        CLASS_ITEM_POINTER_COUNT * 2,
    )?;
    let class_item_pointer_table = bind_table(
        pointer_bytes,
        "sound_test_class_item_pointers",
        CLASS_ITEM_POINTER_TABLE_CPU_ADDRESS,
        CLASS_ITEM_POINTER_TABLE_SHA1,
    )?;
    let list_bytes = source_slice(
        rom,
        CLASS_ITEM_LIST_STORAGE_CPU_ADDRESS,
        CLASS_ITEM_LIST_STORAGE_BYTE_COUNT,
    )?;
    let class_item_list_storage = bind_table(
        list_bytes,
        "sound_test_class_item_lists",
        CLASS_ITEM_LIST_STORAGE_CPU_ADDRESS,
        CLASS_ITEM_LIST_STORAGE_SHA1,
    )?;
    let class_items = parse_class_item_lists(pointer_bytes, list_bytes)?;
    let class_item_bytes = class_items
        .iter()
        .flat_map(|(class_id, items)| {
            std::iter::once(*class_id)
                .chain(std::iter::once(
                    u8::try_from(items.len()).expect("sound-test item list length fits one byte"),
                ))
                .chain(items.iter().copied())
        })
        .collect::<Vec<_>>();
    let class_item_pair_count = class_items
        .iter()
        .map(|(_, items)| items.len())
        .sum::<usize>();
    ensure!(
        class_item_pair_count == CLASS_ITEM_PAIR_COUNT,
        "sound-test class-item pair count changed: expected {CLASS_ITEM_PAIR_COUNT}, found {class_item_pair_count}"
    );
    let class_item_sha1 = sha1_hex(&class_item_bytes);
    ensure!(
        class_item_sha1 == CLASS_ITEM_PAIR_SHA1,
        "sound-test class-item domain changed: expected {CLASS_ITEM_PAIR_SHA1}, found {class_item_sha1}"
    );
    let item_source_indices = class_items
        .iter()
        .flat_map(|(_, items)| items.iter().copied().map(usize::from))
        .collect::<BTreeSet<_>>();
    ensure!(
        item_source_indices.len() == UNIQUE_ITEM_SOURCE_INDEX_COUNT,
        "sound-test unique item source-index count changed"
    );

    let mut player_participant_glyph_sets = Vec::with_capacity(PLAYER_PARTICIPANT_COUNT);
    let mut player_participant_inputs = Vec::with_capacity(PLAYER_PARTICIPANT_COUNT);
    let mut enemy_participant_glyph_sets = Vec::with_capacity(ENEMY_PARTICIPANT_COUNT);
    let mut enemy_participant_inputs = Vec::with_capacity(ENEMY_PARTICIPANT_COUNT);
    for (class_id, items) in &class_items {
        let class_source_index = usize::from(*class_id - 1);
        for item_source_index in items {
            let mut class_item_glyphs = entry_glyphs(fixed, "class-names", class_source_index)?;
            class_item_glyphs.extend(entry_glyphs(
                fixed,
                "item-names",
                usize::from(*item_source_index),
            )?);
            for identity in UNIT_IDENTITY_MINIMUM..=UNIT_IDENTITY_MAXIMUM {
                let mut glyphs = entry_glyphs(fixed, "unit-names", usize::from(identity - 1))?;
                glyphs.extend(&class_item_glyphs);
                player_participant_glyph_sets.push(glyphs);
                player_participant_inputs.push(PlayerParticipantInput {
                    identity,
                    class_id: *class_id,
                    item_source_index: *item_source_index,
                });
            }
            for identity in ENEMY_IDENTITY_MINIMUM..=ENEMY_IDENTITY_MAXIMUM {
                let mut glyphs = entry_glyphs(fixed, "enemy-names", usize::from(identity - 1))?;
                glyphs.extend(&class_item_glyphs);
                enemy_participant_glyph_sets.push(glyphs);
                enemy_participant_inputs.push(EnemyParticipantInput {
                    identity,
                    class_id: *class_id,
                    item_source_index: *item_source_index,
                });
            }
        }
    }
    ensure!(
        player_participant_glyph_sets.len() == PLAYER_PARTICIPANT_COUNT
            && player_participant_inputs.len() == PLAYER_PARTICIPANT_COUNT,
        "sound-test player participant domain changed"
    );
    ensure!(
        enemy_participant_glyph_sets.len() == ENEMY_PARTICIPANT_COUNT
            && enemy_participant_inputs.len() == ENEMY_PARTICIPANT_COUNT,
        "sound-test enemy participant domain changed"
    );

    Ok(SoundTestBattleDomain {
        player_participant_glyph_sets,
        player_participant_inputs,
        enemy_participant_glyph_sets,
        enemy_participant_inputs,
        enemy_name_source_indices: (0..usize::from(ENEMY_IDENTITY_MAXIMUM)).collect(),
        item_source_indices,
        binding: SoundTestBattleDomainBinding {
            initialize_pairing,
            random_input,
            select_pairing,
            replay_pairing,
            class_item_pointer_table,
            class_item_list_storage,
            unit_identity_minimum: UNIT_IDENTITY_MINIMUM,
            unit_identity_maximum: UNIT_IDENTITY_MAXIMUM,
            enemy_identity_minimum: ENEMY_IDENTITY_MINIMUM,
            enemy_identity_maximum: ENEMY_IDENTITY_MAXIMUM,
            enemy_identity_high_bit_required: false,
            class_id_minimum: CLASS_ID_MINIMUM,
            class_id_maximum: CLASS_ID_MAXIMUM,
            terrain_source_index_minimum: TERRAIN_SOURCE_INDEX_MINIMUM,
            terrain_source_index_maximum: TERRAIN_SOURCE_INDEX_MAXIMUM,
            class_item_pair_count,
            class_item_pair_sha1: class_item_sha1,
            unique_item_source_index_count: UNIQUE_ITEM_SOURCE_INDEX_COUNT,
            player_participant_candidate_count: PLAYER_PARTICIPANT_COUNT,
            enemy_participant_candidate_count: ENEMY_PARTICIPANT_COUNT,
            class_items_are_direct_name_source_indices: true,
            random_names_replayed_with_selected_class_items: true,
            participant_role_is_owned_by_staging_position: true,
            source_domain_complete: true,
        },
    })
}

fn parse_class_item_lists(pointer_bytes: &[u8], list_storage: &[u8]) -> Result<Vec<(u8, Vec<u8>)>> {
    ensure!(
        pointer_bytes.len() == CLASS_ITEM_POINTER_COUNT * 2,
        "sound-test class-item pointer table length changed"
    );
    let pointers = pointer_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        pointers[0] == pointers[1],
        "sound-test unused class-zero pointer no longer aliases class one"
    );
    let mut class_items = Vec::new();
    for class_id in CLASS_ID_MINIMUM..=CLASS_ID_MAXIMUM {
        let pointer = pointers[usize::from(class_id)];
        let offset = usize::from(
            pointer
                .checked_sub(CLASS_ITEM_LIST_STORAGE_CPU_ADDRESS)
                .with_context(|| {
                    format!("sound-test class {class_id} item pointer precedes list storage")
                })?,
        );
        let tail = list_storage
            .get(offset..)
            .with_context(|| format!("sound-test class {class_id} item pointer exceeds storage"))?;
        let terminator = tail
            .iter()
            .position(|value| value & 0x80 != 0)
            .with_context(|| format!("sound-test class {class_id} item list has no terminator"))?;
        ensure!(
            tail[terminator] == 0xFF,
            "sound-test class {class_id} item-list terminator changed"
        );
        let items = tail[..terminator].to_vec();
        ensure!(
            !items.is_empty(),
            "sound-test class {class_id} has no item choices"
        );
        ensure!(
            items
                .iter()
                .all(|item| usize::from(*item) < ITEM_ENTRY_COUNT),
            "sound-test class {class_id} selects an item outside the name directory"
        );
        ensure!(
            items.iter().copied().collect::<BTreeSet<_>>().len() == items.len(),
            "sound-test class {class_id} repeats an item choice"
        );
        class_items.push((class_id, items));
    }
    Ok(class_items)
}

fn entry_glyphs(
    fixed: &FixedTextPlan,
    table_id: &str,
    source_index: usize,
) -> Result<BTreeSet<char>> {
    fixed
        .entry_for_source_index(table_id, source_index)
        .with_context(|| format!("missing {table_id} source {source_index} for sound-test battle"))
        .map(|entry| entry.unique_glyphs())
}

fn bind_routine(rom: &Rom, spec: RoutineSpec) -> Result<SourceRoutineBinding> {
    let bytes = source_slice(rom, spec.cpu_address, spec.byte_count)?;
    let source_sha1 = sha1_hex(bytes);
    ensure!(
        source_sha1 == spec.expected_sha1,
        "{} source changed: expected {}, found {}",
        spec.role,
        spec.expected_sha1,
        source_sha1
    );
    let typed = decode_rp2a03_sequence(bytes, spec.cpu_address, spec.role)?;
    Ok(SourceRoutineBinding {
        role: spec.role,
        prg_bank: SOUND_TEST_PRG_BANK,
        cpu_address: spec.cpu_address,
        byte_count: spec.byte_count,
        source_sha1,
        typed_instruction_count: typed.len(),
    })
}

fn bind_table(
    bytes: &[u8],
    role: &'static str,
    cpu_address: u16,
    expected_sha1: &str,
) -> Result<SourceTableBinding> {
    let source_sha1 = sha1_hex(bytes);
    ensure!(
        source_sha1 == expected_sha1,
        "{role} source changed: expected {expected_sha1}, found {source_sha1}"
    );
    Ok(SourceTableBinding {
        role,
        prg_bank: SOUND_TEST_PRG_BANK,
        cpu_address,
        byte_count: bytes.len(),
        source_sha1,
    })
}

fn source_slice(rom: &Rom, cpu_address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(
        (SWITCHABLE_CPU_START..SWITCHABLE_CPU_END_EXCLUSIVE).contains(&cpu_address),
        "sound-test battle source is outside the switchable CPU window"
    );
    let file_offset = HEADER_SIZE
        + usize::from(SOUND_TEST_PRG_BANK) * PRG_BANK_SIZE
        + usize::from(cpu_address - SWITCHABLE_CPU_START);
    rom.data()
        .get(file_offset..file_offset + byte_count)
        .context("sound-test battle source is outside the ROM")
}

fn contains_fragment(source: &[u8], fragment: &[u8]) -> bool {
    source
        .windows(fragment.len())
        .any(|window| window == fragment)
}

#[cfg(test)]
pub(super) fn test_binding() -> SoundTestBattleDomainBinding {
    fn routine(role: &'static str) -> SourceRoutineBinding {
        SourceRoutineBinding {
            role,
            prg_bank: SOUND_TEST_PRG_BANK,
            cpu_address: 0x8000,
            byte_count: 1,
            source_sha1: "source".to_owned(),
            typed_instruction_count: 1,
        }
    }
    fn table(role: &'static str) -> SourceTableBinding {
        SourceTableBinding {
            role,
            prg_bank: SOUND_TEST_PRG_BANK,
            cpu_address: 0x8000,
            byte_count: 1,
            source_sha1: "source".to_owned(),
        }
    }
    SoundTestBattleDomainBinding {
        initialize_pairing: routine("initialize pairing"),
        random_input: routine("random input"),
        select_pairing: routine("select pairing"),
        replay_pairing: routine("replay pairing"),
        class_item_pointer_table: table("pointers"),
        class_item_list_storage: table("lists"),
        unit_identity_minimum: UNIT_IDENTITY_MINIMUM,
        unit_identity_maximum: UNIT_IDENTITY_MAXIMUM,
        enemy_identity_minimum: ENEMY_IDENTITY_MINIMUM,
        enemy_identity_maximum: ENEMY_IDENTITY_MAXIMUM,
        enemy_identity_high_bit_required: false,
        class_id_minimum: CLASS_ID_MINIMUM,
        class_id_maximum: CLASS_ID_MAXIMUM,
        terrain_source_index_minimum: TERRAIN_SOURCE_INDEX_MINIMUM,
        terrain_source_index_maximum: TERRAIN_SOURCE_INDEX_MAXIMUM,
        class_item_pair_count: CLASS_ITEM_PAIR_COUNT,
        class_item_pair_sha1: "pairs".to_owned(),
        unique_item_source_index_count: UNIQUE_ITEM_SOURCE_INDEX_COUNT,
        player_participant_candidate_count: PLAYER_PARTICIPANT_COUNT,
        enemy_participant_candidate_count: ENEMY_PARTICIPANT_COUNT,
        class_items_are_direct_name_source_indices: true,
        random_names_replayed_with_selected_class_items: true,
        participant_role_is_owned_by_staging_position: true,
        source_domain_complete: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_uses_class_pointer_and_stops_at_high_bit_terminator() {
        let mut pointers = Vec::new();
        for class_id in 0..CLASS_ITEM_POINTER_COUNT {
            let pointer =
                CLASS_ITEM_LIST_STORAGE_CPU_ADDRESS + u16::try_from(class_id * 2).unwrap();
            pointers.extend_from_slice(&pointer.to_le_bytes());
        }
        pointers[0..2].copy_from_slice(&CLASS_ITEM_LIST_STORAGE_CPU_ADDRESS.to_le_bytes());
        pointers[2..4].copy_from_slice(&CLASS_ITEM_LIST_STORAGE_CPU_ADDRESS.to_le_bytes());
        let mut lists = vec![0xFF; CLASS_ITEM_LIST_STORAGE_BYTE_COUNT];
        for class_id in 0..CLASS_ITEM_POINTER_COUNT {
            lists[class_id * 2] = u8::try_from(class_id).unwrap();
            lists[class_id * 2 + 1] = 0xFF;
        }
        lists[0] = 1;

        let parsed = parse_class_item_lists(&pointers, &lists).unwrap();

        assert_eq!(parsed.first(), Some(&(1, vec![1])));
        assert_eq!(parsed.last(), Some(&(23, vec![23])));
    }
}
