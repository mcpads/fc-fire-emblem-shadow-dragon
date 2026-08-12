use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{
    class_profile::ClassProfilePlan,
    font_slots::{FONT_PAGE_SIZE, active_hangul_codes},
    japanese_encoding::is_japanese_text_code,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
};

use super::{
    dialogue_probe_font::{assign_glyph_codes_excluding, build_font_page},
    encode_chr_page_register,
};

pub(super) const PROFILE_PAGE_SELECTOR_ADDRESS: u16 = 0xBE3C;
pub(super) const PROFILE_PAGE_SELECTOR_CAVE_END: u16 = 0xBE70;
pub(super) const TITLE_COMPOSER_HOOK_ADDRESS: u16 = 0x82ED;
pub(super) const SOURCE_TITLE_COMPOSER_PREFIX: [u8; 4] = [0xA9, 0xA8, 0x85, 0x02];
pub(super) const PROFILE_PAGE_SPLIT_INDEX: usize = 11;

const FIRST_PAGE_LOAD_ADDRESS: u16 = 0xBE47;
const WRITE_PAGES_ADDRESS: u16 = 0xBE49;
const SOURCE_FONT_PHYSICAL_PAGE: usize = 0;
const PHYSICAL_NAMETABLE_SIZE: usize = 1024;
const TILE_BYTES_PER_NAMETABLE: usize = 30 * 32;
const NAMETABLE_MEMORY_SIZE: usize = 2 * PHYSICAL_NAMETABLE_SIZE;
const MINIMUM_TEMPORAL_SAMPLE_COUNT: usize = 5;

pub(super) struct ClassProfilePagePlan {
    pub(super) assignments: [BTreeMap<char, u8>; 2],
    pub(super) page_pack: Vec<u8>,
    pub(super) page_sha1s: [String; 2],
    pub(super) physical_pages: [u8; 2],
    pub(super) mapper_registers: [u8; 2],
    pub(super) evidence_manifest_sha1: String,
    pub(super) temporal_sample_count: usize,
    pub(super) unique_image_count: usize,
    pub(super) visible_code_count: usize,
    pub(super) preserved_active_code_count: usize,
}

impl ClassProfilePagePlan {
    pub(super) fn assignments_for_profile(&self, profile_index: usize) -> &BTreeMap<char, u8> {
        &self.assignments[usize::from(profile_index >= PROFILE_PAGE_SPLIT_INDEX)]
    }
}

pub(super) fn plan_class_profile_pages(
    cumulative_rom: &Rom,
    source_rom: &Rom,
    profiles: &ClassProfilePlan,
    evidence_path: &Path,
    first_physical_page: u8,
) -> Result<ClassProfilePagePlan> {
    source_rom.verify_supported_japanese()?;
    ensure!(
        profiles.entries.len() == 22
            && profiles
                .entries
                .iter()
                .enumerate()
                .all(|(index, entry)| entry.profile_index == index),
        "class-profile page plan lost the ordered twenty-two-profile sequence"
    );
    ensure!(
        cumulative_rom.chr().len().is_multiple_of(FONT_PAGE_SIZE)
            && cumulative_rom.chr().len() / FONT_PAGE_SIZE == usize::from(first_physical_page),
        "class-profile pages no longer follow the cumulative CHR base"
    );
    ensure!(
        first_physical_page.is_multiple_of(2),
        "class-profile pages must begin at an 8 KiB CHR boundary"
    );
    let second_physical_page = first_physical_page
        .checked_add(1)
        .context("class-profile second physical page overflow")?;

    let evidence = load_screen_evidence(evidence_path)?;
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let mut preserved_active_codes = evidence
        .visible_codes
        .iter()
        .copied()
        .filter(|code| !is_japanese_text_code(*code) && active_codes.contains(code))
        .collect::<BTreeSet<_>>();
    preserved_active_codes.extend(
        profiles
            .encoded_codes()
            .intersection(&active_codes)
            .copied(),
    );

    let first_glyphs = profiles.entries[..PROFILE_PAGE_SPLIT_INDEX]
        .iter()
        .flat_map(|entry| entry.unique_glyphs())
        .collect::<BTreeSet<_>>();
    let second_glyphs = profiles.entries[PROFILE_PAGE_SPLIT_INDEX..]
        .iter()
        .flat_map(|entry| entry.unique_glyphs())
        .collect::<BTreeSet<_>>();
    let assignments = [
        assign_glyph_codes_excluding(&first_glyphs, &preserved_active_codes)?,
        assign_glyph_codes_excluding(&second_glyphs, &preserved_active_codes)?,
    ];

    let source_page = source_rom
        .chr()
        .get(
            SOURCE_FONT_PHYSICAL_PAGE * FONT_PAGE_SIZE
                ..(SOURCE_FONT_PHYSICAL_PAGE + 1) * FONT_PAGE_SIZE,
        )
        .context("class-profile source font page is outside CHR")?;
    let first_page = build_font_page(source_page, &assignments[0])?;
    let second_page = build_font_page(source_page, &assignments[1])?;
    let mut page_pack = first_page.clone();
    page_pack.extend_from_slice(&second_page);

    Ok(ClassProfilePagePlan {
        assignments,
        page_pack,
        page_sha1s: [sha1_hex(&first_page), sha1_hex(&second_page)],
        physical_pages: [first_physical_page, second_physical_page],
        mapper_registers: [
            encode_chr_page_register(first_physical_page)?,
            encode_chr_page_register(second_physical_page)?,
        ],
        evidence_manifest_sha1: evidence.manifest_sha1,
        temporal_sample_count: evidence.temporal_sample_count,
        unique_image_count: evidence.unique_image_count,
        visible_code_count: evidence.visible_codes.len(),
        preserved_active_code_count: preserved_active_codes.len(),
    })
}

pub(super) fn build_profile_page_selector(mapper_registers: [u8; 2]) -> Result<Vec<u8>> {
    ensure!(
        mapper_registers.iter().all(|register| *register != 0),
        "class-profile page register cannot select CHR RAM"
    );
    let selector = assemble_at(
        PROFILE_PAGE_SELECTOR_ADDRESS,
        &[
            Instruction::LdaAbsolute(0x0559),
            Instruction::CmpImmediate(u8::try_from(PROFILE_PAGE_SPLIT_INDEX)?),
            Instruction::BccAbsolute(FIRST_PAGE_LOAD_ADDRESS),
            Instruction::LdaImmediate(mapper_registers[1]),
            Instruction::BneAbsolute(WRITE_PAGES_ADDRESS),
            Instruction::LdaImmediate(mapper_registers[0]),
            Instruction::Pha,
            Instruction::LdaImmediate(2),
            Instruction::StaAbsolute(0x8000),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Pha,
            Instruction::LdaImmediate(4),
            Instruction::StaAbsolute(0x8000),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::LdaImmediate(0xA8),
            Instruction::StaZeroPage(0x02),
            Instruction::Rts,
        ],
    )?;
    ensure!(
        usize::from(PROFILE_PAGE_SELECTOR_ADDRESS) + selector.len()
            <= usize::from(PROFILE_PAGE_SELECTOR_CAVE_END),
        "class-profile selector exceeds its source-bound cave"
    );
    Ok(selector)
}

pub(super) fn build_title_composer_hook() -> Result<Vec<u8>> {
    let hook = assemble_at(
        TITLE_COMPOSER_HOOK_ADDRESS,
        &[
            Instruction::JsrAbsolute(PROFILE_PAGE_SELECTOR_ADDRESS),
            Instruction::Nop,
        ],
    )?;
    ensure!(
        hook.len() == SOURCE_TITLE_COMPOSER_PREFIX.len(),
        "class-profile title hook changed the consumer prefix size"
    );
    Ok(hook)
}

struct LoadedEvidence {
    manifest_sha1: String,
    temporal_sample_count: usize,
    unique_image_count: usize,
    visible_codes: BTreeSet<u8>,
}

#[derive(Debug, Deserialize)]
struct EvidenceManifest {
    format_version: u8,
    screen_role: String,
    source_sha1: String,
    source_dump_directory: String,
    source_nametable_sha1: String,
    source_state_sha1: String,
    temporal_samples: Vec<TemporalSample>,
}

#[derive(Debug, Deserialize)]
struct TemporalSample {
    frame_count: u64,
    image: String,
    image_sha1: String,
}

fn load_screen_evidence(manifest_path: &Path) -> Result<LoadedEvidence> {
    let manifest_bytes = fs::read(manifest_path)
        .with_context(|| format!("read class-profile evidence {}", manifest_path.display()))?;
    let manifest: EvidenceManifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parse class-profile evidence {}", manifest_path.display()))?;
    ensure!(
        manifest.format_version == 1
            && manifest.screen_role == "automatic_class_profile"
            && manifest.source_sha1 == EXPECTED_SOURCE_SHA1,
        "class-profile evidence scope changed"
    );
    ensure!(
        manifest.temporal_samples.len() >= MINIMUM_TEMPORAL_SAMPLE_COUNT,
        "class-profile evidence needs at least {MINIMUM_TEMPORAL_SAMPLE_COUNT} irregular samples"
    );
    let parent = manifest_path
        .parent()
        .context("class-profile evidence has no parent directory")?;
    let dump = resolve_below(parent, &manifest.source_dump_directory)?;
    let nametable = read_bound_file(
        &dump.join("nametable.bin"),
        &manifest.source_nametable_sha1,
        "source nametable",
    )?;
    ensure!(
        nametable.len() == NAMETABLE_MEMORY_SIZE,
        "class-profile source nametable must be exactly 2 KiB"
    );
    let state = read_bound_file(
        &dump.join("state.json"),
        &manifest.source_state_sha1,
        "source state",
    )?;
    let state: serde_json::Value =
        serde_json::from_slice(&state).context("parse class-profile source state")?;
    ensure!(
        state
            .get("ppu.control.backgroundPatternAddr")
            .and_then(serde_json::Value::as_u64)
            == Some(0x1000)
            && state
                .get("ppu.control.spritePatternAddr")
                .and_then(serde_json::Value::as_u64)
                == Some(0)
            && state
                .get("mapper.prgPage")
                .and_then(serde_json::Value::as_u64)
                == Some(0x0D)
            && state
                .get("mapper.rightChrPage[0]")
                .and_then(serde_json::Value::as_u64)
                == Some(0)
            && state
                .get("mapper.rightChrPage[1]")
                .and_then(serde_json::Value::as_u64)
                == Some(0),
        "class-profile source dump no longer binds the automatic screen font lifetime"
    );

    let mut frames = BTreeSet::new();
    let mut image_hashes = BTreeSet::new();
    let mut intervals = BTreeSet::new();
    let mut previous_frame = None;
    for sample in &manifest.temporal_samples {
        ensure!(
            frames.insert(sample.frame_count),
            "class-profile evidence repeats a frame"
        );
        if let Some(previous) = previous_frame {
            intervals.insert(sample.frame_count - previous);
        }
        previous_frame = Some(sample.frame_count);
        let image_path = resolve_below(parent, &sample.image)?;
        let image = read_bound_file(&image_path, &sample.image_sha1, "temporal image")?;
        ensure!(!image.is_empty(), "class-profile temporal image is empty");
        image_hashes.insert(sample.image_sha1.clone());
    }
    ensure!(
        intervals.len() >= 3 && image_hashes.len() >= 3,
        "class-profile samples do not cover irregular animated frames"
    );

    let mut visible_codes = BTreeSet::new();
    for physical_table in 0..2 {
        let start = physical_table * PHYSICAL_NAMETABLE_SIZE;
        visible_codes.extend(
            nametable[start..start + TILE_BYTES_PER_NAMETABLE]
                .iter()
                .copied(),
        );
    }
    Ok(LoadedEvidence {
        manifest_sha1: sha1_hex(&manifest_bytes),
        temporal_sample_count: manifest.temporal_samples.len(),
        unique_image_count: image_hashes.len(),
        visible_codes,
    })
}

fn resolve_below(parent: &Path, relative: &str) -> Result<std::path::PathBuf> {
    let relative = Path::new(relative);
    ensure!(
        !relative.is_absolute()
            && relative
                .components()
                .all(|component| !matches!(component, Component::ParentDir)),
        "class-profile evidence path escapes its manifest directory"
    );
    Ok(parent.join(relative))
}

fn read_bound_file(path: &Path, expected_sha1: &str, role: &str) -> Result<Vec<u8>> {
    let bytes =
        fs::read(path).with_context(|| format!("read class-profile {role} {}", path.display()))?;
    ensure!(
        sha1_hex(&bytes) == expected_sha1,
        "class-profile {role} SHA-1 changed"
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{class_profile::plan_class_profiles, font_slots::ACTIVE_HANGUL_SLOT_COUNT};

    #[test]
    fn twenty_two_profiles_split_into_two_screen_safe_pages() {
        let source_path = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
        ));
        let workspace = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/translation/class-profiles.ko.json"
        ));
        let evidence = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../evidence/private/class-profile-manifest.json"
        ));
        if !source_path.exists() || !evidence.exists() {
            return;
        }
        let source = Rom::from_path(source_path).unwrap();
        let profiles = plan_class_profiles(&source, workspace).unwrap();
        let page = plan_class_profile_pages(&source, &source, &profiles, evidence, 32).unwrap();

        // 페이지별 글리프 수는 확정한 표기에 따라 달라지므로 고정하지 않는다.
        // 지켜야 하는 것은 각 페이지가 활성 슬롯 예산 안에 들면서, 두 페이지의 합집합은
        // 한 페이지를 넘어 실제로 분할이 필요하다는 점이다.
        for assignment in &page.assignments {
            assert!(!assignment.is_empty());
            assert!(assignment.len() <= ACTIVE_HANGUL_SLOT_COUNT);
        }
        let combined: BTreeSet<char> = page
            .assignments
            .iter()
            .flat_map(|assignment| assignment.keys().copied())
            .collect();
        assert!(combined.len() > ACTIVE_HANGUL_SLOT_COUNT);
        assert_eq!(page.preserved_active_code_count, 12);
        assert_eq!(page.page_pack.len(), 2 * FONT_PAGE_SIZE);
    }

    #[test]
    fn title_hook_selects_both_latches_and_restores_the_replaced_prefix_effect() {
        let selector = build_profile_page_selector([0xB8, 0xBC]).unwrap();
        let hook = build_title_composer_hook().unwrap();

        assert_eq!(hook, [0x20, 0x3C, 0xBE, 0xEA]);
        assert!(
            selector
                .windows(5)
                .any(|bytes| bytes == [0xA9, 0x02, 0x8D, 0x00, 0x80])
        );
        assert!(
            selector
                .windows(5)
                .any(|bytes| bytes == [0xA9, 0x04, 0x8D, 0x00, 0x80])
        );
        assert_eq!(
            &selector[selector.len() - 5..],
            &[0xA9, 0xA8, 0x85, 0x02, 0x60]
        );
    }
}
