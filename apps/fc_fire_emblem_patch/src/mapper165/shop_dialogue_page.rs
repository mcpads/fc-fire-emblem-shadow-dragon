use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use anyhow::{Context, Result, ensure};

use crate::{
    font_slots::{FONT_PAGE_SIZE, active_hangul_codes},
    rom::Rom,
    sha1_hex,
};

use super::{
    dialogue_probe_font::{assign_glyph_codes_excluding, build_font_page},
    encode_chr_page_register,
};

mod evidence;
mod selector;

use evidence::load_shop_screen_codes;
pub(super) use selector::{
    PAGE_ROUTINE_ADDRESS, PAGE_ROUTINE_CAVE_END, PAGE_ROUTINE_END, build_page_selector,
};

pub(super) const SCREEN_ROLE: &str = "weapon_shop_dialogue_lifetime";
pub(super) const RECORD_IDS: [&str; 8] = [
    "shop-and-item-dialogue:000",
    "shop-and-item-dialogue:001",
    "shop-and-item-dialogue:002",
    "shop-and-item-dialogue:003",
    "shop-and-item-dialogue:004",
    "shop-and-item-dialogue:005",
    "shop-and-item-dialogue:006",
    "shop-and-item-dialogue:054",
];

const SOURCE_FONT_PHYSICAL_PAGE: usize = 2;

#[derive(Clone)]
pub(super) struct ShopDialoguePagePlan {
    pub(super) assignments: BTreeMap<char, u8>,
    pub(super) page_pack: Vec<u8>,
    pub(super) manifest_sha1: String,
    pub(super) sample_count: usize,
    pub(super) unique_nametable_count: usize,
    pub(super) preserved_screen_active_code_count: usize,
    pub(super) preserved_source_active_code_count: usize,
    pub(super) preserved_active_code_count: usize,
    pub(super) preserved_active_codes: BTreeSet<u8>,
    pub(super) page_sha1: String,
    pub(super) physical_chr_page: u8,
    pub(super) mapper_register: u8,
}

pub(super) fn plan_shop_dialogue_page(
    parity_rom: &Rom,
    manifest_path: &Path,
    glyphs: &BTreeSet<char>,
    preserved_source_codes: &BTreeSet<u8>,
    physical_chr_page: u8,
) -> Result<ShopDialoguePagePlan> {
    let evidence = load_shop_screen_codes(manifest_path)?;
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let preserved_screen_active_codes = evidence
        .screen_codes
        .intersection(&active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let preserved_source_active_codes = preserved_source_codes
        .intersection(&active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let preserved_active_codes = preserved_screen_active_codes
        .union(&preserved_source_active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let assignments = assign_glyph_codes_excluding(glyphs, &preserved_active_codes)?;

    ensure!(
        parity_rom.chr().len().is_multiple_of(FONT_PAGE_SIZE),
        "weapon-shop parity CHR is not 4 KiB page aligned"
    );
    ensure!(
        parity_rom.chr().len() / FONT_PAGE_SIZE == usize::from(physical_chr_page),
        "weapon-shop extension page does not follow the cumulative base"
    );
    let source_start = SOURCE_FONT_PHYSICAL_PAGE * FONT_PAGE_SIZE;
    let source_end = source_start + 2 * FONT_PAGE_SIZE;
    let source_pair = parity_rom
        .chr()
        .get(source_start..source_end)
        .context("weapon-shop source font pair is outside parity CHR")?;
    let mut page_pack = build_font_page(&source_pair[..FONT_PAGE_SIZE], &assignments)?;
    page_pack.extend_from_slice(&source_pair[FONT_PAGE_SIZE..]);
    let mapper_register = encode_chr_page_register(physical_chr_page)?;

    Ok(ShopDialoguePagePlan {
        assignments,
        page_sha1: sha1_hex(&page_pack[..FONT_PAGE_SIZE]),
        page_pack,
        manifest_sha1: evidence.manifest_sha1,
        sample_count: evidence.sample_count,
        unique_nametable_count: evidence.unique_nametable_count,
        preserved_screen_active_code_count: preserved_screen_active_codes.len(),
        preserved_source_active_code_count: preserved_source_active_codes.len(),
        preserved_active_code_count: preserved_active_codes.len(),
        preserved_active_codes,
        physical_chr_page,
        mapper_register,
    })
}

pub(super) fn extend_shop_dialogue_page(
    page: &ShopDialoguePagePlan,
    requested_glyphs: &BTreeSet<char>,
) -> Result<ShopDialoguePagePlan> {
    let additional_glyphs = requested_glyphs
        .iter()
        .filter(|glyph| !page.assignments.contains_key(glyph))
        .copied()
        .collect::<BTreeSet<_>>();
    let mut unavailable_codes = page.preserved_active_codes.clone();
    unavailable_codes.extend(page.assignments.values().copied());
    let additions = assign_glyph_codes_excluding(&additional_glyphs, &unavailable_codes)?;
    ensure!(
        additions
            .values()
            .all(|code| !unavailable_codes.contains(code)),
        "weapon-shop shared-text page reused an occupied code"
    );

    let mut assignments = page.assignments.clone();
    for (glyph, code) in &additions {
        ensure!(
            assignments.insert(*glyph, *code).is_none(),
            "weapon-shop shared-text glyph was assigned twice"
        );
    }
    let mut page_pack = build_font_page(&page.page_pack[..FONT_PAGE_SIZE], &additions)?;
    page_pack.extend_from_slice(&page.page_pack[FONT_PAGE_SIZE..]);
    ensure!(
        page_pack.len() == page.page_pack.len()
            && page_pack[FONT_PAGE_SIZE..] == page.page_pack[FONT_PAGE_SIZE..],
        "weapon-shop shared-text extension changed the companion page"
    );

    Ok(ShopDialoguePagePlan {
        assignments,
        page_sha1: sha1_hex(&page_pack[..FONT_PAGE_SIZE]),
        page_pack,
        manifest_sha1: page.manifest_sha1.clone(),
        sample_count: page.sample_count,
        unique_nametable_count: page.unique_nametable_count,
        preserved_screen_active_code_count: page.preserved_screen_active_code_count,
        preserved_source_active_code_count: page.preserved_source_active_code_count,
        preserved_active_code_count: page.preserved_active_code_count,
        preserved_active_codes: page.preserved_active_codes.clone(),
        physical_chr_page: page.physical_chr_page,
        mapper_register: page.mapper_register,
    })
}
