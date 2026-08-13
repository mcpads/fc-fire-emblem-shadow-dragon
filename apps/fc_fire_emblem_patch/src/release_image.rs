//! 배포 이미지의 헤더와 CHR 정렬을 하드웨어 구성에 맞춘다.
//!
//! 누적 빌드가 만드는 이미지는 iNES 1.0이고 CHR ROM 크기가 2의 거듭제곱이 아니다.
//! iNES 1.0에는 «CHR ROM이 있으면서 CHR RAM도 따로 있다»를 적을 자리가 없어서,
//! 지금은 실행 환경이 매퍼 165를 알아서 맞춰 주기를 기대하는 상태다. 에뮬레이터는
//! 맞춰 주지만 그 사실이 실기에서도 맞는다는 증거는 아니다. 의사결정 62번을 따른다.
//!
//! 이 단계는 PRG와 CHR 내용을 바꾸지 않는다. 헤더를 NES 2.0으로 다시 쓰고 CHR ROM을
//! 매퍼 상한과 같은 256 KiB까지 0으로 채운다. 채우는 자리는 아직 아무도 고르지 않는
//! 물리 페이지이므로 뱅크 번호는 그대로다.

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    mapper165::MAXIMUM_CHR_PAGE_COUNT,
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
};

const EXPECTED_MAPPER: u16 = 165;
const PRG_BANK_SIZE: usize = 16 * 1024;
const CHR_BANK_SIZE: usize = 8 * 1024;
const CHR_PAGE_SIZE: usize = 4 * 1024;

/// 매퍼 165의 CHR 뱅크 레지스터는 1 KiB 단위 8비트다. `page × 4`가 255를 넘을 수
/// 없으므로 4 KiB 페이지 64장, 256 KiB가 하드웨어 상한이다.
const RELEASE_CHR_SIZE: usize = MAXIMUM_CHR_PAGE_COUNT as usize * CHR_PAGE_SIZE;
/// 매퍼 165의 PRG는 MMC3식 8 KiB 단위라 512 KiB가 끝이다. 현재 이미지가 상한에
/// 정확히 닿아 있으므로 늘릴 여지가 없다.
const RELEASE_PRG_SIZE: usize = 512 * 1024;
/// 보드에 있는 CHR RAM이다. 레지스터 값 0으로 고른다.
const CHR_RAM_BYTE_COUNT: usize = 4 * 1024;
/// 배터리로 유지되는 작업 RAM이다. 원본 저장 구조를 그대로 쓴다.
const BATTERY_WORK_RAM_BYTE_COUNT: usize = 8 * 1024;

#[derive(Debug, Serialize)]
pub(crate) struct ReleaseImagePlan {
    pub(crate) schema: u8,
    pub(crate) input_sha1: String,
    pub(crate) output_sha1: String,
    pub(crate) mapper: u16,
    pub(crate) header_format: &'static str,
    pub(crate) prg_byte_count: usize,
    pub(crate) input_chr_byte_count: usize,
    pub(crate) output_chr_byte_count: usize,
    pub(crate) appended_zero_chr_page_count: usize,
    pub(crate) chr_ram_byte_count: usize,
    pub(crate) battery_work_ram_byte_count: usize,
    pub(crate) prg_bytes_unchanged: bool,
    pub(crate) existing_chr_bytes_unchanged: bool,
    pub(crate) chr_size_is_mapper_maximum: bool,
    pub(crate) prg_size_is_mapper_maximum: bool,
    pub(crate) header_declares_chr_ram: bool,
}

/// NES 2.0 헤더를 만든다. 바이트마다 무엇을 선언하는지는 아래 주석을 따른다.
fn release_header(prg_byte_count: usize, chr_byte_count: usize) -> Result<[u8; HEADER_SIZE]> {
    let prg_banks = prg_byte_count / PRG_BANK_SIZE;
    let chr_banks = chr_byte_count / CHR_BANK_SIZE;
    ensure!(
        prg_banks <= 0xFF && chr_banks <= 0xFF,
        "release image sizes need the NES 2.0 exponent form, which this mapper does not require"
    );

    let mut header = [0u8; HEADER_SIZE];
    header[..4].copy_from_slice(b"NES\x1A");
    header[4] = prg_banks as u8;
    header[5] = chr_banks as u8;
    // 하위 니블은 매퍼 165의 아래 네 비트, 비트 1은 배터리, 비트 0은 세로 미러링이다.
    // 원본 변환이 쓰던 값을 그대로 유지한다.
    header[6] = 0x52;
    // 상위 니블은 매퍼 165의 위 네 비트, 비트 3~2 `10`이 NES 2.0 표식이다.
    header[7] = 0xA8;
    // 매퍼 상위 비트와 서브매퍼. 둘 다 없다.
    header[8] = 0x00;
    // PRG·CHR 크기의 상위 니블. 두 크기 모두 바이트 4·5 안에 들어간다.
    header[9] = 0x00;
    // PRG RAM. 상위 니블이 배터리 유지 분량의 shift 값이고 `64 << n` 바이트다.
    header[10] = shift_nibble(BATTERY_WORK_RAM_BYTE_COUNT)? << 4;
    // CHR RAM. 하위 니블이 배터리 없는 분량의 shift 값이다.
    header[11] = shift_nibble(CHR_RAM_BYTE_COUNT)?;
    // 타이밍. 원본이 일본판이므로 NTSC다.
    header[12] = 0x00;
    // 확장 콘솔 종류. 일반 패미컴이다.
    header[13] = 0x00;
    // 별도 ROM 개수. 없다.
    header[14] = 0x00;
    // 기본 입력 장치. 표준 컨트롤러다.
    header[15] = 0x01;
    Ok(header)
}

/// NES 2.0은 RAM 크기를 `64 << n` 꼴의 shift 값으로 적는다.
fn shift_nibble(byte_count: usize) -> Result<u8> {
    ensure!(
        byte_count.is_power_of_two() && byte_count >= 64,
        "NES 2.0 RAM size {byte_count} is not a shift of 64"
    );
    let shift = byte_count.trailing_zeros() - 6;
    ensure!(shift <= 0x0F, "NES 2.0 RAM size {byte_count} does not fit a nibble");
    Ok(shift as u8)
}

pub(crate) fn build_release_image(cumulative: &Rom) -> Result<(Vec<u8>, ReleaseImagePlan)> {
    ensure!(
        cumulative.mapper() == EXPECTED_MAPPER,
        "release packaging expects the mapper 165 cumulative image"
    );
    let prg = cumulative.prg();
    let chr = cumulative.chr();
    ensure!(
        prg.len() == RELEASE_PRG_SIZE,
        "release packaging expects the {RELEASE_PRG_SIZE}-byte PRG, found {}",
        prg.len()
    );
    ensure!(
        chr.len() <= RELEASE_CHR_SIZE,
        "cumulative CHR {} exceeds the mapper 165 maximum {RELEASE_CHR_SIZE}",
        chr.len()
    );
    ensure!(
        chr.len() % CHR_PAGE_SIZE == 0,
        "cumulative CHR is not a whole number of 4 KiB pages"
    );

    let header = release_header(prg.len(), RELEASE_CHR_SIZE)?;
    let mut output = Vec::with_capacity(HEADER_SIZE + prg.len() + RELEASE_CHR_SIZE);
    output.extend_from_slice(&header);
    output.extend_from_slice(prg);
    output.extend_from_slice(chr);
    output.resize(HEADER_SIZE + prg.len() + RELEASE_CHR_SIZE, 0);

    let rebuilt = Rom::parse(output.clone()).context("release image does not parse")?;
    ensure!(
        rebuilt.mapper() == EXPECTED_MAPPER,
        "release header lost the mapper number"
    );
    ensure!(
        rebuilt.prg() == prg,
        "release packaging changed a PRG byte"
    );
    ensure!(
        &rebuilt.chr()[..chr.len()] == chr
            && rebuilt.chr()[chr.len()..].iter().all(|byte| *byte == 0),
        "release packaging changed an existing CHR byte or padded with non-zero"
    );

    let plan = ReleaseImagePlan {
        schema: 1,
        input_sha1: sha1_hex(cumulative.data()),
        output_sha1: sha1_hex(&output),
        mapper: EXPECTED_MAPPER,
        header_format: "NES 2.0",
        prg_byte_count: prg.len(),
        input_chr_byte_count: chr.len(),
        output_chr_byte_count: RELEASE_CHR_SIZE,
        appended_zero_chr_page_count: (RELEASE_CHR_SIZE - chr.len()) / CHR_PAGE_SIZE,
        chr_ram_byte_count: CHR_RAM_BYTE_COUNT,
        battery_work_ram_byte_count: BATTERY_WORK_RAM_BYTE_COUNT,
        prg_bytes_unchanged: true,
        existing_chr_bytes_unchanged: true,
        chr_size_is_mapper_maximum: true,
        prg_size_is_mapper_maximum: prg.len() == RELEASE_PRG_SIZE,
        header_declares_chr_ram: true,
    };
    Ok((output, plan))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_header_declares_the_board_ram_the_mapper_actually_has() {
        let header = release_header(RELEASE_PRG_SIZE, RELEASE_CHR_SIZE).unwrap();

        assert_eq!(&header[..4], b"NES\x1A");
        assert_eq!(header[7] & 0x0C, 0x08, "NES 2.0 marker");
        assert_eq!(
            ((header[6] >> 4) as u16) | ((header[7] & 0xF0) as u16),
            EXPECTED_MAPPER
        );
        assert_eq!(header[6] & 0x02, 0x02, "battery");
        // 4 KiB CHR RAM = 64 << 6, 8 KiB 배터리 작업 RAM = 64 << 7.
        assert_eq!(header[11] & 0x0F, 6);
        assert_eq!(header[10] >> 4, 7);
    }

    #[test]
    fn shift_nibble_rejects_sizes_the_format_cannot_express() {
        assert_eq!(shift_nibble(64).unwrap(), 0);
        assert_eq!(shift_nibble(4 * 1024).unwrap(), 6);
        assert!(shift_nibble(96).is_err());
        assert!(shift_nibble(32).is_err());
    }

    /// 매퍼가 주소로 닿을 수 있는 크기를 넘기면 배포 이미지를 만들지 않는다.
    #[test]
    fn release_sizes_stay_inside_what_the_mapper_can_address() {
        assert_eq!(RELEASE_CHR_SIZE, 256 * 1024);
        assert_eq!(RELEASE_PRG_SIZE, 512 * 1024);
        assert_eq!(RELEASE_CHR_SIZE / CHR_PAGE_SIZE, 64);
    }
}
