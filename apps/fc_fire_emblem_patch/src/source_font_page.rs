use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{
    font_slots::{FONT_PAGE_SIZE, active_hangul_codes},
    japanese_encoding::is_japanese_text_code,
    rom::Rom,
    sha1_hex,
};

const SOURCE_FONT_PAGE_INDEX: usize = 0;
pub(crate) const SOURCE_FONT_PAGE_SHA1: &str = "1860feeb0b0b216abb79bf7917bde8b51734a980";

pub(crate) struct SourceFontPageOwnership {
    active_codes: BTreeSet<u8>,
    japanese_text_codes: BTreeSet<u8>,
    preserved_non_japanese_codes: BTreeSet<u8>,
}

pub(crate) fn bind_source_font_page_ownership(rom: &Rom) -> Result<SourceFontPageOwnership> {
    rom.verify_supported_japanese()?;
    let page_start = SOURCE_FONT_PAGE_INDEX
        .checked_mul(FONT_PAGE_SIZE)
        .context("source-font page offset overflow")?;
    let page = rom
        .chr()
        .get(page_start..page_start + FONT_PAGE_SIZE)
        .context("source-font page is outside CHR")?;
    ensure!(
        sha1_hex(page) == SOURCE_FONT_PAGE_SHA1,
        "source-font page changed"
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
        japanese_text_codes.len() == 111
            && preserved_non_japanese_codes.len() == 99
            && japanese_text_codes.is_disjoint(&preserved_non_japanese_codes)
            && japanese_text_codes
                .union(&preserved_non_japanese_codes)
                .copied()
                .collect::<BTreeSet<_>>()
                == active_codes,
        "source-font active-code ownership changed"
    );
    Ok(SourceFontPageOwnership {
        active_codes,
        japanese_text_codes,
        preserved_non_japanese_codes,
    })
}

impl SourceFontPageOwnership {
    pub(crate) fn page_sha1(&self) -> &'static str {
        SOURCE_FONT_PAGE_SHA1
    }

    pub(crate) fn active_codes(&self) -> &BTreeSet<u8> {
        &self.active_codes
    }

    pub(crate) fn japanese_text_codes(&self) -> &BTreeSet<u8> {
        &self.japanese_text_codes
    }

    pub(crate) fn preserved_non_japanese_codes(&self) -> &BTreeSet<u8> {
        &self.preserved_non_japanese_codes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_source_codes_partition_into_text_and_preserved_ownership() {
        let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
        let japanese = active_codes
            .iter()
            .copied()
            .filter(|code| is_japanese_text_code(*code))
            .collect::<BTreeSet<_>>();
        let preserved = active_codes
            .difference(&japanese)
            .copied()
            .collect::<BTreeSet<_>>();

        assert_eq!(japanese.len(), 111);
        assert_eq!(preserved.len(), 99);
        assert!(japanese.is_disjoint(&preserved));
    }
}
