use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{
    font_slots::{FONT_PAGE_SIZE, active_hangul_codes},
    japanese_encoding::is_japanese_text_code,
    rom::Rom,
    sha1_hex,
};

const SOURCE_FONT_PAGE_INDEX: usize = 0;
const SOURCE_FONT_PAGE_SHA1: &str = "1860feeb0b0b216abb79bf7917bde8b51734a980";

pub(super) struct BattleBackgroundCodeOwnership {
    active_codes: BTreeSet<u8>,
    japanese_text_codes: BTreeSet<u8>,
    preserved_non_japanese_codes: BTreeSet<u8>,
}

pub(super) struct ObservedBattleBackgroundCodes {
    pub(super) japanese_text_codes: BTreeSet<u8>,
    pub(super) preserved_non_japanese_codes: BTreeSet<u8>,
}

pub(super) fn bind_battle_background_code_ownership(
    rom: &Rom,
) -> Result<BattleBackgroundCodeOwnership> {
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
