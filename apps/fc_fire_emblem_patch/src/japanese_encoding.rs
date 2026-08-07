pub(crate) const JAPANESE_TEXT_GLYPHS: [&str; 0x60] = [
    "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "゛",
    "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "゜",
    "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "を", "ん",
    "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "ー",
    "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "、",
    "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "ヲ", "ン",
];

pub(crate) const SMALL_KANA_GLYPHS: [&str; 8] = ["ゃ", "っ", "ゅ", "ょ", "ャ", "ッ", "ュ", "ョ"];
pub(crate) const SMALL_KATAKANA_VOWEL_GLYPHS: [&str; 5] = ["ァ", "ィ", "ゥ", "ェ", "ォ"];

pub(crate) fn is_japanese_text_code(code: u8) -> bool {
    japanese_text_glyph(code).is_some()
}

pub(crate) fn japanese_text_glyph(code: u8) -> Option<&'static str> {
    JAPANESE_TEXT_GLYPHS
        .get(usize::from(code))
        .or_else(|| SMALL_KANA_GLYPHS.get(usize::from(code.checked_sub(0x84)?)))
        .or_else(|| SMALL_KATAKANA_VOWEL_GLYPHS.get(usize::from(code.checked_sub(0xA6)?)))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_known_japanese_option_codes_without_admitting_latin_codes() {
        let sound = [0x3A, 0x32, 0x5F, 0x44, 0x0F]
            .into_iter()
            .map(|code| japanese_text_glyph(code).unwrap())
            .collect::<String>();

        assert_eq!(sound, "サウント゛");
        let animation = [0x30, 0x46, 0x53, 0x3F, 0x3B, 0x8B, 0x5F]
            .into_iter()
            .map(|code| japanese_text_glyph(code).unwrap())
            .collect::<String>();
        assert_eq!(animation, "アニメーション");
        assert!(is_japanese_text_code(0x5F));
        assert!(!is_japanese_text_code(0x60));
        assert!(is_japanese_text_code(0x84));
        assert!(is_japanese_text_code(0x8B));
        assert!(!is_japanese_text_code(0x8C));
        assert_eq!(japanese_text_glyph(0xA6), Some("ァ"));
        assert_eq!(japanese_text_glyph(0xA7), Some("ィ"));
        assert_eq!(japanese_text_glyph(0xA8), Some("ゥ"));
        assert_eq!(japanese_text_glyph(0xA9), Some("ェ"));
        assert_eq!(japanese_text_glyph(0xAA), Some("ォ"));
        assert!(japanese_text_glyph(0xA5).is_none());
        assert!(japanese_text_glyph(0xAB).is_none());
        assert!(japanese_text_glyph(0x83).is_none());
    }

    #[test]
    fn decodes_small_katakana_vowels_used_by_unit_ui_names() {
        let falchion = [0x4C, 0xA6, 0x5A, 0x3B, 0x34, 0x5F]
            .into_iter()
            .map(|code| japanese_text_glyph(code).unwrap())
            .collect::<String>();
        let paladin = [0x4A, 0x1F, 0x58, 0x43, 0x0F, 0xA7, 0x5F]
            .into_iter()
            .map(|code| japanese_text_glyph(code).unwrap())
            .collect::<String>();
        let worm = [0x32, 0xAA, 0x3F, 0x52]
            .into_iter()
            .map(|code| japanese_text_glyph(code).unwrap())
            .collect::<String>();

        assert_eq!(falchion, "ファルシオン");
        assert_eq!(paladin, "ハ゜ラテ゛ィン");
        assert_eq!(worm, "ウォーム");
    }
}
