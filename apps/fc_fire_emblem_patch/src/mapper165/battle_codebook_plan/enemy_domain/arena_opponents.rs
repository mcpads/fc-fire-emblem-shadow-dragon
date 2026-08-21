use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

use super::ParticipantCandidate;

const PRG_BANK_SIZE: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const ARENA_GENERATOR_PRG_BANK: u8 = 0x0B;
const ARENA_GENERATOR_ADDRESS: u16 = 0xA32D;
const ARENA_GENERATOR_BYTE_COUNT: usize = 0xF1;
const ARENA_GENERATOR_SHA1: &str = "2bc5a2ba661bab173eac2517f72eac5a55fdf34a";

const ARENA_ENEMY_IDENTITY: u8 = 0xAC;
const CHAPTER_CLASS_POINTER_TABLE_ADDRESS: u16 = 0xA434;
const CHAPTER_CLASS_POINTER_COUNT: usize = 22;
const CHAPTER_CLASS_POINTER_TABLE_SHA1: &str = "21ba017bc1870e887984accbd346c30816f90663";
const CHAPTER_CLASS_CHOICE_TABLE_ADDRESS: u16 = 0xA460;
const CHAPTER_CLASS_CHOICE_COUNT: usize = CHAPTER_CLASS_POINTER_COUNT * 4;
const CHAPTER_CLASS_CHOICE_TABLE_SHA1: &str = "3c95eae48ca719b0f9c81f6a1b992955cfb20bd7";
const CLASS_ITEM_TABLE_ADDRESS: u16 = 0xA4B8;
const CLASS_ITEM_TABLE_BYTE_COUNT: usize = 23;
const CLASS_ITEM_TABLE_SHA1: &str = "eed1cb59f37aab0ef3917a533df7fabe07fd644e";
const ALTERNATE_CLASS_TABLE_ADDRESS: u16 = 0xA4F3;
const ALTERNATE_CLASS_COUNT: usize = 6;
const ALTERNATE_CLASS_TABLE_SHA1: &str = "3070166a6d9d7cc1169129b045bd6fadb400f8e8";
const ALTERNATE_ITEM_BASE_TABLE_ADDRESS: u16 = 0xA4F9;
const ALTERNATE_ITEM_BASE_TABLE_SHA1: &str = "10c0dfd95282cd40012feaa0e8835f5183181d69";
const ALTERNATE_ITEM_VARIANT_COUNT: u8 = 4;

pub(super) struct ArenaOpponentDomain {
    pub(super) candidates: BTreeSet<ParticipantCandidate>,
    pub(super) binding: ArenaOpponentDomainBinding,
}

#[derive(Debug, Serialize)]
pub(super) struct ArenaOpponentDomainBinding {
    generator_prg_bank: u8,
    generator_cpu_address_hex: String,
    generator_byte_count: usize,
    generator_sha1: String,
    generator_typed_instruction_count: usize,
    fixed_enemy_identity_hex: String,
    chapter_class_pointer_count: usize,
    chapter_class_choice_count: usize,
    primary_class_item_pair_count: usize,
    alternate_class_count: usize,
    alternate_item_variants_per_class: usize,
    alternate_class_item_pair_count: usize,
    combined_class_item_pair_count: usize,
    candidate_sha1: String,
    chapter_pointer_targets_bound: bool,
    primary_class_item_lookup_bound: bool,
    alternate_class_item_generation_bound: bool,
    generated_record_identity_bound: bool,
    output_domain_is_necessary_condition_superset: bool,
    exact_random_reachability_proven: bool,
}

pub(super) fn bind_arena_opponent_domain(rom: &Rom) -> Result<ArenaOpponentDomain> {
    let generator = bank_slice(
        rom,
        ARENA_GENERATOR_ADDRESS,
        ARENA_GENERATOR_BYTE_COUNT,
        "arena opponent generator",
    )?;
    ensure_sha1(generator, ARENA_GENERATOR_SHA1, "arena opponent generator")?;
    let generator_instructions = decode_rp2a03_sequence(
        generator,
        ARENA_GENERATOR_ADDRESS,
        "generate_arena_opponent",
    )?;

    let pointer_bytes = bank_slice(
        rom,
        CHAPTER_CLASS_POINTER_TABLE_ADDRESS,
        CHAPTER_CLASS_POINTER_COUNT * 2,
        "arena chapter class pointer table",
    )?;
    ensure_sha1(
        pointer_bytes,
        CHAPTER_CLASS_POINTER_TABLE_SHA1,
        "arena chapter class pointer table",
    )?;
    let class_choices = bank_slice(
        rom,
        CHAPTER_CLASS_CHOICE_TABLE_ADDRESS,
        CHAPTER_CLASS_CHOICE_COUNT,
        "arena chapter class choices",
    )?;
    ensure_sha1(
        class_choices,
        CHAPTER_CLASS_CHOICE_TABLE_SHA1,
        "arena chapter class choices",
    )?;
    for (index, pointer) in pointer_bytes.chunks_exact(2).enumerate() {
        let actual = u16::from_le_bytes([pointer[0], pointer[1]]);
        let expected = CHAPTER_CLASS_CHOICE_TABLE_ADDRESS
            .checked_add(u16::try_from(index * 4)?)
            .context("arena chapter class pointer overflow")?;
        ensure!(
            actual == expected,
            "arena chapter class pointer {index} changed: expected {expected:04X}, found {actual:04X}"
        );
    }

    let class_items = bank_slice(
        rom,
        CLASS_ITEM_TABLE_ADDRESS,
        CLASS_ITEM_TABLE_BYTE_COUNT,
        "arena class item table",
    )?;
    ensure_sha1(class_items, CLASS_ITEM_TABLE_SHA1, "arena class item table")?;
    let primary_pairs = primary_class_item_pairs(class_choices, class_items)?;

    let alternate_classes = bank_slice(
        rom,
        ALTERNATE_CLASS_TABLE_ADDRESS,
        ALTERNATE_CLASS_COUNT,
        "arena alternate class table",
    )?;
    ensure_sha1(
        alternate_classes,
        ALTERNATE_CLASS_TABLE_SHA1,
        "arena alternate class table",
    )?;
    let alternate_item_bases = bank_slice(
        rom,
        ALTERNATE_ITEM_BASE_TABLE_ADDRESS,
        ALTERNATE_CLASS_COUNT,
        "arena alternate item-base table",
    )?;
    ensure_sha1(
        alternate_item_bases,
        ALTERNATE_ITEM_BASE_TABLE_SHA1,
        "arena alternate item-base table",
    )?;
    let alternate_pairs = alternate_class_item_pairs(alternate_classes, alternate_item_bases)?;

    let class_item_pairs = primary_pairs
        .union(&alternate_pairs)
        .copied()
        .collect::<BTreeSet<_>>();
    let candidates = class_item_pairs
        .iter()
        .map(|(class_id, item_id)| ParticipantCandidate {
            identity: ARENA_ENEMY_IDENTITY,
            class_id: *class_id,
            item_id: *item_id,
        })
        .collect::<BTreeSet<_>>();
    let candidate_bytes = candidates
        .iter()
        .flat_map(|candidate| [candidate.identity, candidate.class_id, candidate.item_id])
        .collect::<Vec<_>>();

    Ok(ArenaOpponentDomain {
        candidates,
        binding: ArenaOpponentDomainBinding {
            generator_prg_bank: ARENA_GENERATOR_PRG_BANK,
            generator_cpu_address_hex: format!("0x{ARENA_GENERATOR_ADDRESS:04X}"),
            generator_byte_count: ARENA_GENERATOR_BYTE_COUNT,
            generator_sha1: sha1_hex(generator),
            generator_typed_instruction_count: generator_instructions.len(),
            fixed_enemy_identity_hex: format!("0x{ARENA_ENEMY_IDENTITY:02X}"),
            chapter_class_pointer_count: CHAPTER_CLASS_POINTER_COUNT,
            chapter_class_choice_count: CHAPTER_CLASS_CHOICE_COUNT,
            primary_class_item_pair_count: primary_pairs.len(),
            alternate_class_count: ALTERNATE_CLASS_COUNT,
            alternate_item_variants_per_class: usize::from(ALTERNATE_ITEM_VARIANT_COUNT),
            alternate_class_item_pair_count: alternate_pairs.len(),
            combined_class_item_pair_count: class_item_pairs.len(),
            candidate_sha1: sha1_hex(&candidate_bytes),
            chapter_pointer_targets_bound: true,
            primary_class_item_lookup_bound: true,
            alternate_class_item_generation_bound: true,
            generated_record_identity_bound: true,
            output_domain_is_necessary_condition_superset: true,
            exact_random_reachability_proven: false,
        },
    })
}

fn primary_class_item_pairs(
    class_choices: &[u8],
    class_items: &[u8],
) -> Result<BTreeSet<(u8, u8)>> {
    class_choices
        .iter()
        .map(|class_id| {
            ensure!(*class_id != 0, "arena primary class is zero");
            let item_id = class_items
                .get(usize::from(*class_id))
                .copied()
                .with_context(|| format!("arena primary class {class_id:02X} has no item entry"))?;
            Ok((*class_id, item_id))
        })
        .collect()
}

fn alternate_class_item_pairs(classes: &[u8], item_bases: &[u8]) -> Result<BTreeSet<(u8, u8)>> {
    ensure!(
        classes.len() == item_bases.len(),
        "arena alternate class and item tables differ in length"
    );
    let mut pairs = BTreeSet::new();
    for (class_id, item_base) in classes.iter().zip(item_bases) {
        ensure!(*class_id != 0, "arena alternate class is zero");
        for variant in 0..ALTERNATE_ITEM_VARIANT_COUNT {
            let mut item_id = item_base
                .checked_add(variant)
                .context("arena alternate item identity overflow")?;
            if item_id == 5 {
                item_id = 2;
            }
            pairs.insert((*class_id, item_id));
        }
    }
    Ok(pairs)
}

fn bank_slice<'a>(rom: &'a Rom, address: u16, byte_count: usize, role: &str) -> Result<&'a [u8]> {
    ensure!(
        (SWITCHABLE_CPU_START..0xC000).contains(&address),
        "{role} address is outside switchable PRG"
    );
    let offset = HEADER_SIZE
        + usize::from(ARENA_GENERATOR_PRG_BANK) * PRG_BANK_SIZE
        + usize::from(address - SWITCHABLE_CPU_START);
    rom.data()
        .get(offset..offset + byte_count)
        .with_context(|| format!("{role} is outside the ROM"))
}

fn ensure_sha1(bytes: &[u8], expected: &str, role: &str) -> Result<()> {
    let actual = sha1_hex(bytes);
    ensure!(
        actual == expected,
        "{role} changed: expected {expected}, found {actual}"
    );
    Ok(())
}

#[cfg(test)]
pub(super) fn test_binding() -> ArenaOpponentDomainBinding {
    ArenaOpponentDomainBinding {
        generator_prg_bank: ARENA_GENERATOR_PRG_BANK,
        generator_cpu_address_hex: format!("0x{ARENA_GENERATOR_ADDRESS:04X}"),
        generator_byte_count: ARENA_GENERATOR_BYTE_COUNT,
        generator_sha1: "generator".to_owned(),
        generator_typed_instruction_count: 1,
        fixed_enemy_identity_hex: format!("0x{ARENA_ENEMY_IDENTITY:02X}"),
        chapter_class_pointer_count: CHAPTER_CLASS_POINTER_COUNT,
        chapter_class_choice_count: CHAPTER_CLASS_CHOICE_COUNT,
        primary_class_item_pair_count: 1,
        alternate_class_count: ALTERNATE_CLASS_COUNT,
        alternate_item_variants_per_class: usize::from(ALTERNATE_ITEM_VARIANT_COUNT),
        alternate_class_item_pair_count: 1,
        combined_class_item_pair_count: 2,
        candidate_sha1: "candidates".to_owned(),
        chapter_pointer_targets_bound: true,
        primary_class_item_lookup_bound: true,
        alternate_class_item_generation_bound: true,
        generated_record_identity_bound: true,
        output_domain_is_necessary_condition_superset: true,
        exact_random_reachability_proven: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_tables_cover_primary_and_alternate_generated_loadouts() {
        let primary = primary_class_item_pairs(&[1, 2, 3], &[0, 0x0C, 0x1B, 0]).unwrap();
        assert_eq!(
            primary,
            [(1, 0x0C), (2, 0x1B), (3, 0)].into_iter().collect()
        );

        let alternate = alternate_class_item_pairs(&[0x0B, 0x12], &[2, 0x2B]).unwrap();
        assert!(alternate.contains(&(0x0B, 2)));
        assert!(alternate.contains(&(0x0B, 3)));
        assert!(alternate.contains(&(0x0B, 4)));
        assert!(!alternate.contains(&(0x0B, 5)));
        assert!(alternate.contains(&(0x12, 0x2E)));
        assert_eq!(alternate.len(), 7);
    }
}
