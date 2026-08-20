pub(crate) const SOURCE_PRG_BANK: u8 = 0x0B;
pub(crate) const POINTER_TABLE_ADDRESS: u16 = 0x8FC2;
pub(crate) const POINTER_LOAD_ADDRESS: u16 = 0x8EF0;
pub(crate) const POINTER_LOAD_BYTES: [u8; 10] =
    [0xB9, 0xC2, 0x8F, 0x85, 0x00, 0xB9, 0xC3, 0x8F, 0x85, 0x01];
pub(crate) const CHOICE_LABEL_COMPOSITE_STATE: u8 = 0x0C;
pub(super) const SHOP_CHOICE_COMPOSER_ADDRESS: u16 = 0x87A5;
pub(super) const SHOP_CHOICE_COMPOSER_BYTES: [u8; 10] =
    [0xA9, 0x22, 0x20, 0xEE, 0x8E, 0xA9, 0x23, 0x20, 0xEE, 0x8E];

pub(super) struct ChoiceLabelSpec {
    pub(super) id: &'static str,
    pub(super) index: u8,
    pub(super) pointer: u16,
    pub(super) expected: &'static [u8],
}

pub(super) const LABEL_SPECS: &[ChoiceLabelSpec] = &[
    ChoiceLabelSpec {
        id: "choice-label:yes",
        index: 0x22,
        pointer: 0x9146,
        expected: &[0x1A, 0x01, 0xED],
    },
    ChoiceLabelSpec {
        id: "choice-label:no",
        index: 0x23,
        pointer: 0x9149,
        expected: &[0x01, 0x01, 0x03, 0xED],
    },
];
