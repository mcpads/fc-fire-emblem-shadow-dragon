use std::collections::BTreeSet;

pub(crate) const FONT_PAGE_SIZE: usize = 4 * 1024;
pub(crate) const FONT_TILE_SIZE: usize = 16;
pub(crate) const FONT_CODE_COUNT: usize = FONT_PAGE_SIZE / FONT_TILE_SIZE;

pub(crate) const PRESERVED_DISPLAY_CODES: [u8; 38] = [
    0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F,
    0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x7B, 0x7C, 0x7D, 0x7E, 0x7F,
    0x80, 0x81, 0x82, 0x83, 0x8D, 0x9B,
];
pub(crate) const TEXT_CONTROL_CODES: [u8; 2] = [0xED, 0xEF];
pub(crate) const LATCH_TRIGGER_CODES: [u8; 2] = [0xFD, 0xFE];
pub(crate) const LAYOUT_RESERVED_CODES: [u8; 3] = [0x0F, 0x1F, 0xFF];
pub(crate) const ACTIVE_HANGUL_SLOT_COUNT: usize = FONT_CODE_COUNT
    - PRESERVED_DISPLAY_CODES.len()
    - TEXT_CONTROL_CODES.len()
    - LATCH_TRIGGER_CODES.len()
    - LAYOUT_RESERVED_CODES.len();

pub(crate) fn protected_original_codes() -> BTreeSet<u8> {
    PRESERVED_DISPLAY_CODES
        .into_iter()
        .chain(TEXT_CONTROL_CODES)
        .chain(LATCH_TRIGGER_CODES)
        .collect()
}

pub(crate) fn reserved_font_codes() -> BTreeSet<u8> {
    protected_original_codes()
        .into_iter()
        .chain(LAYOUT_RESERVED_CODES)
        .collect()
}

pub(crate) fn active_hangul_codes() -> Vec<u8> {
    let reserved = reserved_font_codes();
    (u8::MIN..=u8::MAX)
        .filter(|code| !reserved.contains(code))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_partition_is_complete_and_disjoint() {
        let protected = protected_original_codes();
        let reserved = reserved_font_codes();
        let active = active_hangul_codes();

        assert_eq!(protected.len(), 42);
        assert_eq!(reserved.len(), 45);
        assert_eq!(active.len(), ACTIVE_HANGUL_SLOT_COUNT);
        assert_eq!(ACTIVE_HANGUL_SLOT_COUNT, 211);
        assert!(active.iter().all(|code| !reserved.contains(code)));
        assert_eq!(reserved.len() + active.len(), FONT_CODE_COUNT);
    }
}
