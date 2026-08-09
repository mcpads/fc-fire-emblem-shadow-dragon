use super::*;

pub(super) const CHR_PAGE_SIZE: usize = 4 * 1024;
pub(super) const TILE_SIZE: usize = 16;
pub(super) const TILES_PER_PAGE: usize = CHR_PAGE_SIZE / TILE_SIZE;
pub(super) const FONT_PAGE_INDEX: usize = 0;
pub(super) const STATUS_LABELS_OFFSET: usize = 0x3447B;
pub(super) const SOURCE_STATUS_LABELS: [u8; 32] = [
    0x7C, 0x7D, 0x7B, 0x8D, // STR:
    0x7C, 0x74, 0x72, 0x8D, // SKI:
    0x80, 0x75, 0x7F, 0x8D, // WLV:
    0x6A, 0x70, 0x72, 0x8D, // AGI:
    0x6D, 0x6E, 0x6F, 0x8D, // DEF:
    0x76, 0x78, 0x7F, 0x8D, // MOV:
    0x71, 0x9B, 0x79, 0x8D, // H.P:
    0x6E, 0x81, 0x79, 0x8D, // EXP:
];

pub(super) const ENTRY_SEPARATOR: u8 = 0xED;
pub(super) const TABLE_TERMINATOR: u8 = 0xEF;
pub(super) const PRG_BANK_SIZE: usize = 16 * 1024;

pub(super) struct Mmc4ControlRoutine {
    pub(super) role: &'static str,
    pub(super) cpu_address: u16,
    pub(super) expected: &'static [u8],
}

pub(super) const MMC4_CONTROL_ROUTINES: [Mmc4ControlRoutine; 3] = [
    Mmc4ControlRoutine {
        role: "select_prg_bank_and_update_shadows",
        cpu_address: 0xC9A6,
        expected: &[0x85, 0x29, 0x85, 0x51, 0x8D, 0x00, 0xA0, 0x60],
    },
    Mmc4ControlRoutine {
        role: "set_mirroring_bit_1",
        cpu_address: 0xC9CE,
        expected: &[0xA9, 0x01, 0x85, 0xC8, 0x8D, 0x00, 0xF0, 0x60],
    },
    Mmc4ControlRoutine {
        role: "set_mirroring_bit_0",
        cpu_address: 0xC9D6,
        expected: &[0xA9, 0x00, 0x85, 0xC8, 0x8D, 0x00, 0xF0, 0x60],
    },
];

pub(super) const MMC4_REGISTER_SPECS: [(u16, &str); 6] = [
    (0xA000, "select_16k_prg_bank"),
    (0xB000, "select_ppu_0000_fd_chr_bank"),
    (0xC000, "select_ppu_0000_fe_chr_bank"),
    (0xD000, "select_ppu_1000_fd_chr_bank"),
    (0xE000, "select_ppu_1000_fe_chr_bank"),
    (0xF000, "select_nametable_mirroring"),
];

pub(super) struct Mmc4ChrWriter {
    pub(super) cpu_address: u16,
    pub(super) shadow_address: u8,
    pub(super) hardware_register: u16,
    pub(super) latch_domain: &'static str,
    pub(super) expected: [u8; 8],
}

pub(super) const MMC4_CHR_WRITERS: [Mmc4ChrWriter; 4] = [
    Mmc4ChrWriter {
        cpu_address: 0xC9AE,
        shadow_address: 0x59,
        hardware_register: 0xB000,
        latch_domain: "ppu_0000_fd",
        expected: [0x85, 0x59, 0x05, 0x52, 0x8D, 0x00, 0xB0, 0x60],
    },
    Mmc4ChrWriter {
        cpu_address: 0xC9B6,
        shadow_address: 0x5A,
        hardware_register: 0xC000,
        latch_domain: "ppu_0000_fe",
        expected: [0x85, 0x5A, 0x05, 0x52, 0x8D, 0x00, 0xC0, 0x60],
    },
    Mmc4ChrWriter {
        cpu_address: 0xC9BE,
        shadow_address: 0x5B,
        hardware_register: 0xD000,
        latch_domain: "ppu_1000_fd",
        expected: [0x85, 0x5B, 0x05, 0x52, 0x8D, 0x00, 0xD0, 0x60],
    },
    Mmc4ChrWriter {
        cpu_address: 0xC9C6,
        shadow_address: 0x5C,
        hardware_register: 0xE000,
        latch_domain: "ppu_1000_fe",
        expected: [0x85, 0x5C, 0x05, 0x52, 0x8D, 0x00, 0xE0, 0x60],
    },
];

pub(super) const HEX_GLYPHS: [[u8; 5]; 16] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b110, 0b001, 0b010, 0b100, 0b111],
    [0b110, 0b001, 0b010, 0b001, 0b110],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b110, 0b001, 0b110],
    [0b011, 0b100, 0b110, 0b101, 0b010],
    [0b111, 0b001, 0b010, 0b010, 0b010],
    [0b010, 0b101, 0b010, 0b101, 0b010],
    [0b010, 0b101, 0b011, 0b001, 0b110],
    [0b010, 0b101, 0b111, 0b101, 0b101],
    [0b110, 0b101, 0b110, 0b101, 0b110],
    [0b011, 0b100, 0b100, 0b100, 0b011],
    [0b110, 0b101, 0b101, 0b101, 0b110],
    [0b111, 0b100, 0b110, 0b100, 0b111],
    [0b111, 0b100, 0b110, 0b100, 0b100],
];

pub(super) struct KnownReference {
    pub(super) id: &'static str,
    pub(super) file_offset: usize,
    pub(super) expected: &'static [u8],
    pub(super) displayed_text: &'static str,
    pub(super) consumer: &'static str,
    pub(super) scope: ReferenceScope,
    pub(super) evidence: &'static str,
}

pub(super) const KNOWN_REFERENCES: [KnownReference; 2] = [
    KnownReference {
        id: "options-label-table",
        file_offset: OPTIONS_TABLE_OFFSET,
        expected: &SOURCE_OPTIONS_TABLE,
        displayed_text: "サウンド / アニメーション / ウエイトタイマー",
        consumer: "options labels",
        scope: ReferenceScope::TranslatedJapanese,
        evidence: "confirmed static consumer and runtime display",
    },
    KnownReference {
        id: "status-label-table",
        file_offset: STATUS_LABELS_OFFSET,
        expected: &SOURCE_STATUS_LABELS,
        displayed_text: "STR: / SKI: / WLV: / AGI: / DEF: / MOV: / H.P: / EXP:",
        consumer: "status labels",
        scope: ReferenceScope::PreservedOriginal,
        evidence: "confirmed table bytes and runtime display",
    },
];
