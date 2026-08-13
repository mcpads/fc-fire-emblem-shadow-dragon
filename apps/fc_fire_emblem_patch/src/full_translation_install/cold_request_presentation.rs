//! 냉간 대사 페이지를 합성하는 동안 화면에 내놓는 CHR-ROM 페이지다.
//!
//! 대사 네임테이블에는 직전 한글 페이지의 코드가 남아 있을 수 있다. 그 상태에서
//! 원본 FD 페이지를 다시 고르면 같은 코드가 일본어 글리프나 다른 원본 타일로
//! 해석되어 깨진 글자가 보인다. 반대로 미완성 CHR RAM을 고르면 부분 합성 페이지가
//! 보인다. 그래서 원본 FD 페이지를 복제하되 한글에 할당 가능한 슬롯만 빈 타일로
//! 만든 별도 표시 페이지를 쓴다.
//!
//! 보호된 영어·숫자·제어·래치 타일과 레이아웃 예약 타일은 원본과 바이트 단위로
//! 같다. 따라서 냉간 전송 중에는 직전 한글 본문만 사라지고, 원본 영어와 UI는
//! 그대로 남는다.

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE, active_hangul_codes, reserved_font_codes},
    mapper165::encode_chr_page_register,
    sha1_hex,
};

#[derive(Serialize)]
pub(super) struct ColdRequestPresentationPage {
    pub(super) physical_page: u8,
    pub(super) mapper_register: u8,
    pub(super) bytes: Vec<u8>,
    pub(super) blanked_code_count: usize,
    pub(super) sha1: String,
}

pub(super) fn plan_cold_request_presentation_page(
    source_dialogue_page: &[u8],
    physical_page: u8,
) -> Result<ColdRequestPresentationPage> {
    ensure!(
        source_dialogue_page.len() == FONT_PAGE_SIZE,
        "cold-request presentation source is not one 4 KiB font page"
    );

    let writable_codes = active_hangul_codes();
    let reserved_codes = reserved_font_codes();
    ensure!(
        writable_codes.len() + reserved_codes.len() == FONT_PAGE_SIZE / FONT_TILE_SIZE,
        "font-slot partition does not cover the cold-request presentation page"
    );

    let mut bytes = source_dialogue_page.to_vec();
    for code in &writable_codes {
        let start = usize::from(*code)
            .checked_mul(FONT_TILE_SIZE)
            .context("cold-request tile offset overflow")?;
        bytes[start..start + FONT_TILE_SIZE].fill(0);
    }
    for code in reserved_codes {
        let start = usize::from(code) * FONT_TILE_SIZE;
        ensure!(
            bytes[start..start + FONT_TILE_SIZE]
                == source_dialogue_page[start..start + FONT_TILE_SIZE],
            "cold-request presentation changed protected code {code:02X}"
        );
    }

    let mapper_register = encode_chr_page_register(physical_page)?;
    ensure!(
        mapper_register != 0,
        "cold-request presentation page must select CHR ROM, not CHR RAM"
    );
    let sha1 = sha1_hex(&bytes);
    Ok(ColdRequestPresentationPage {
        physical_page,
        mapper_register,
        bytes,
        blanked_code_count: writable_codes.len(),
        sha1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distinct_source_page() -> Vec<u8> {
        (0..FONT_PAGE_SIZE)
            .map(|offset| (offset as u8).wrapping_mul(37).wrapping_add(11))
            .collect()
    }

    #[test]
    fn writable_hangul_slots_are_blank_during_a_cold_request() {
        let source = distinct_source_page();
        let page = plan_cold_request_presentation_page(&source, 50).unwrap();

        for code in active_hangul_codes() {
            let start = usize::from(code) * FONT_TILE_SIZE;
            assert_eq!(
                &page.bytes[start..start + FONT_TILE_SIZE],
                &[0; FONT_TILE_SIZE]
            );
        }
        assert_eq!(page.blanked_code_count, 210);
    }

    #[test]
    fn protected_and_layout_reserved_tiles_remain_source_exact() {
        let source = distinct_source_page();
        let page = plan_cold_request_presentation_page(&source, 50).unwrap();

        for code in reserved_font_codes() {
            let start = usize::from(code) * FONT_TILE_SIZE;
            assert_eq!(
                &page.bytes[start..start + FONT_TILE_SIZE],
                &source[start..start + FONT_TILE_SIZE]
            );
        }
    }

    #[test]
    fn presentation_uses_the_reserved_chr_rom_page() {
        let page = plan_cold_request_presentation_page(&distinct_source_page(), 50).unwrap();

        assert_eq!(page.physical_page, 50);
        assert_eq!(page.mapper_register, 0xC8);
        assert_ne!(page.mapper_register, 0);
    }
}
