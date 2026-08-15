use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{
    font_slots::FONT_PAGE_SIZE,
    rp2a03::{Instruction, assemble_at},
    screen_contracts::{OBSERVED_CHR_PAIRS, PatternWindow},
};

use super::selector_safety::select_register_instruction;

const FD_TILE_HIGH_PLANE_OFFSET: usize = 0x0FD8;
const FD_TILE_HIGH_PLANE_BYTE_COUNT: usize = 8;
const MAPPER_REGISTER_PAGE_MASK: u8 = 0xFC;
pub(crate) const TRANSLATED_FE_PAGE_FLAG: u8 = 0x01;

/// A에 든 CHR 페이지 값을 X가 가리키는 MMC3 레지스터에 NMI-safe하게 쓴다.
///
/// 누적 패치와 전체 런타임이 함께 쓰는 가장 작은 공통 writer다. `$FA58`을 거치므로
/// 선택 레지스터 그림자와 NMI 복원 계약을 우회하지 않는다.
pub(crate) const WRITE_TRANSLATED_CHR_PAGE_ADDRESS: u16 = 0xF386;
pub(crate) const WRITE_TRANSLATED_CHR_PAGE_END: u16 = 0xF390;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TranslatedFePageSelection {
    PreserveSource,
    UseTranslatedPage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RightFontPageProjection {
    source_fd_page: u8,
    required_fd_trigger_high_plane: [u8; FD_TILE_HIGH_PLANE_BYTE_COUNT],
    fe_selection: TranslatedFePageSelection,
}

impl RightFontPageProjection {
    pub(crate) fn for_screen_roles(
        source_chr: &[u8],
        screen_roles: &[&str],
        source_fd_page: u8,
    ) -> Result<Self> {
        ensure!(
            source_chr.len().is_multiple_of(FONT_PAGE_SIZE),
            "font-pair source CHR is not 4 KiB page aligned"
        );
        ensure!(
            !screen_roles.is_empty(),
            "font-pair projection has no screen role"
        );
        ensure!(
            screen_roles.iter().copied().collect::<BTreeSet<_>>().len() == screen_roles.len(),
            "font-pair projection repeats a screen role"
        );

        let mut fe_pages = BTreeSet::new();
        for role in screen_roles {
            let pairs = OBSERVED_CHR_PAIRS
                .iter()
                .filter(|pair| {
                    pair.screen_role == *role && pair.pattern_window == PatternWindow::Right
                })
                .collect::<Vec<_>>();
            ensure!(
                !pairs.is_empty(),
                "screen role {role} has no observed right-window CHR pair"
            );
            for pair in pairs {
                ensure!(
                    pair.fd_source_page == source_fd_page,
                    "screen role {role} uses right FD page {:02X}, expected {:02X}",
                    pair.fd_source_page,
                    source_fd_page
                );
                fe_pages.insert(pair.fe_source_page);
            }
        }

        let mirrors_translated_page = fe_pages.iter().all(|page| *page == source_fd_page);
        let preserves_source_page = fe_pages.iter().all(|page| *page != source_fd_page);
        ensure!(
            mirrors_translated_page || preserves_source_page,
            "screen-role family mixes translated-FE and preserved-FE lifetimes"
        );

        let required_planes = fe_pages
            .iter()
            .map(|page| fd_tile_high_plane(source_chr, *page))
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(
            required_planes.len() == 1,
            "screen-role family needs multiple tile-FD trigger planes"
        );

        Ok(Self {
            source_fd_page,
            required_fd_trigger_high_plane: *required_planes
                .first()
                .context("screen-role family has no FE trigger plane")?,
            fe_selection: if mirrors_translated_page {
                TranslatedFePageSelection::UseTranslatedPage
            } else {
                TranslatedFePageSelection::PreserveSource
            },
        })
    }

    pub(crate) fn apply_to_page(self, page: &mut [u8]) -> Result<()> {
        ensure!(
            page.len() == FONT_PAGE_SIZE,
            "translated font page is not 4 KiB"
        );
        page[FD_TILE_HIGH_PLANE_OFFSET..FD_TILE_HIGH_PLANE_OFFSET + FD_TILE_HIGH_PLANE_BYTE_COUNT]
            .copy_from_slice(&self.required_fd_trigger_high_plane);
        Ok(())
    }

    pub(crate) fn encode_mapper_route(self, mapper_register: u8) -> Result<u8> {
        ensure!(
            mapper_register != 0 && mapper_register & !MAPPER_REGISTER_PAGE_MASK == 0,
            "translated font page is not an encoded 4 KiB CHR-ROM page"
        );
        Ok(mapper_register
            | match self.fe_selection {
                TranslatedFePageSelection::PreserveSource => 0,
                TranslatedFePageSelection::UseTranslatedPage => TRANSLATED_FE_PAGE_FLAG,
            })
    }

    pub(crate) fn source_fd_page(self) -> u8 {
        self.source_fd_page
    }

    pub(crate) fn fe_selection(self) -> TranslatedFePageSelection {
        self.fe_selection
    }
}

pub(crate) fn mapper_register_from_route(route: u8) -> u8 {
    route & MAPPER_REGISTER_PAGE_MASK
}

pub(crate) fn build_translated_chr_page_writer() -> Result<Vec<u8>> {
    let bytes = assemble_at(
        WRITE_TRANSLATED_CHR_PAGE_ADDRESS,
        &[
            Instruction::Pha,
            Instruction::Txa,
            select_register_instruction(),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Rts,
        ],
    )?;
    ensure!(
        usize::from(WRITE_TRANSLATED_CHR_PAGE_ADDRESS) + bytes.len()
            == usize::from(WRITE_TRANSLATED_CHR_PAGE_END),
        "translated CHR page writer no longer owns its exact fixed-bank gap"
    );
    Ok(bytes)
}

fn fd_tile_high_plane(chr: &[u8], page: u8) -> Result<[u8; FD_TILE_HIGH_PLANE_BYTE_COUNT]> {
    let start = usize::from(page)
        .checked_mul(FONT_PAGE_SIZE)
        .and_then(|offset| offset.checked_add(FD_TILE_HIGH_PLANE_OFFSET))
        .context("tile-FD trigger-plane offset overflow")?;
    let end = start + FD_TILE_HIGH_PLANE_BYTE_COUNT;
    chr.get(start..end)
        .context("tile-FD trigger plane is outside source CHR")?
        .try_into()
        .context("tile-FD trigger plane has the wrong length")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_chr() -> Vec<u8> {
        let mut chr = vec![0; 32 * FONT_PAGE_SIZE];
        for (page, plane) in [
            (0x00, [0x20; 8]),
            (0x0E, [0x00; 8]),
            (0x0F, [0x20; 8]),
            (0x12, [0x20; 8]),
            (0x13, [0x20; 8]),
        ] as [(u8, [u8; 8]); 5]
        {
            let start = usize::from(page) * FONT_PAGE_SIZE + FD_TILE_HIGH_PLANE_OFFSET;
            chr[start..start + 8].copy_from_slice(&plane);
        }
        chr
    }

    #[test]
    fn front_end_and_ending_roles_map_the_translated_page_to_both_latches() {
        for roles in [
            &["new_game_choice", "save_slot_selection"][..],
            &["ending_chapter_record_scroll"][..],
        ] {
            let projection =
                RightFontPageProjection::for_screen_roles(&source_chr(), roles, 0).unwrap();
            assert_eq!(
                projection.fe_selection(),
                TranslatedFePageSelection::UseTranslatedPage
            );
            assert_eq!(projection.encode_mapper_route(0xA8).unwrap(), 0xA9);
        }
    }

    #[test]
    fn class_profile_keeps_fe_page_and_adopts_its_trigger_plane() {
        let projection =
            RightFontPageProjection::for_screen_roles(&source_chr(), &["class_profile"], 0)
                .unwrap();
        let mut page = vec![0xFF; FONT_PAGE_SIZE];
        projection.apply_to_page(&mut page).unwrap();

        assert_eq!(projection.source_fd_page(), 0);
        assert_eq!(
            projection.fe_selection(),
            TranslatedFePageSelection::PreserveSource
        );
        assert_eq!(projection.encode_mapper_route(0xB0).unwrap(), 0xB0);
        assert_eq!(
            &page[FD_TILE_HIGH_PLANE_OFFSET..FD_TILE_HIGH_PLANE_OFFSET + 8],
            &[0; 8]
        );
    }

    #[test]
    fn unit_ui_fe_variants_share_one_preserved_backdrop_trigger_plane() {
        let projection =
            RightFontPageProjection::for_screen_roles(&source_chr(), &["unit_command_menu"], 0)
                .unwrap();
        assert_eq!(
            projection.fe_selection(),
            TranslatedFePageSelection::PreserveSource
        );
        assert_eq!(projection.encode_mapper_route(0xCC).unwrap(), 0xCC);
    }

    #[test]
    fn mixed_fe_ownership_and_unknown_roles_fail_closed() {
        assert!(
            RightFontPageProjection::for_screen_roles(
                &source_chr(),
                &["new_game_choice", "class_profile"],
                0,
            )
            .unwrap_err()
            .to_string()
            .contains("mixes translated-FE")
        );
        assert!(
            RightFontPageProjection::for_screen_roles(&source_chr(), &["missing"], 0)
                .unwrap_err()
                .to_string()
                .contains("no observed")
        );
    }

    #[test]
    fn shared_writer_uses_the_nmi_safe_selected_register_writer() {
        assert_eq!(
            build_translated_chr_page_writer().unwrap(),
            [0x48, 0x8A, 0x20, 0x58, 0xFA, 0x68, 0x8D, 0x01, 0x80, 0x60]
        );
    }
}
