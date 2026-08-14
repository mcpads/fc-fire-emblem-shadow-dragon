//! 테스트가 외부 ROM 없이 계약 경계를 구성하는 데 쓰는 합성 이미지다.

use crate::rom::{HEADER_SIZE, Rom};

const SYNTHETIC_PRG_BANK_COUNT: u8 = 32;
const SYNTHETIC_PRG_BYTE_COUNT: usize = SYNTHETIC_PRG_BANK_COUNT as usize * 16 * 1024;

/// 512 KiB PRG와 CHR ROM이 없는 mapper 165 NES 2.0 이미지를 만든다.
pub(crate) fn synthetic_mapper165_rom_bytes(prg_fill: u8) -> Vec<u8> {
    let mut bytes = vec![prg_fill; HEADER_SIZE + SYNTHETIC_PRG_BYTE_COUNT];
    bytes[..HEADER_SIZE].fill(0);
    bytes[..4].copy_from_slice(b"NES\x1A");
    bytes[4] = SYNTHETIC_PRG_BANK_COUNT;
    bytes[6] = 0x50;
    bytes[7] = 0xA8;
    bytes
}

/// 합성 이미지에서 마지막 16 KiB 고정 뱅크의 CPU 주소를 파일 오프셋으로 바꾼다.
pub(crate) fn synthetic_fixed_bank_file_offset(address: u16) -> usize {
    assert!(address >= 0xC000, "fixed-bank address is below C000");
    HEADER_SIZE + SYNTHETIC_PRG_BYTE_COUNT - 16 * 1024 + usize::from(address - 0xC000)
}

/// 변경할 필요가 없는 시험에서 쓸 균일한 합성 mapper 165 ROM을 파싱한다.
pub(crate) fn synthetic_mapper165_rom(prg_fill: u8) -> Rom {
    Rom::parse(synthetic_mapper165_rom_bytes(prg_fill)).expect("synthetic mapper 165 ROM parses")
}
