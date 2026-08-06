pub(crate) const JAPANESE_TEXT_GLYPHS: [&str; 0x60] = [
    "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "゛",
    "た", "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "゜",
    "ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "を", "ん",
    "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "ー",
    "タ", "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "、",
    "マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "ヲ", "ン",
];

pub(crate) const SMALL_KANA_GLYPHS: [&str; 8] = ["ゃ", "っ", "ゅ", "ょ", "ャ", "ッ", "ュ", "ョ"];

pub(crate) fn is_japanese_text_code(code: u8) -> bool {
    japanese_text_glyph(code).is_some()
}

pub(crate) fn japanese_text_glyph(code: u8) -> Option<&'static str> {
    JAPANESE_TEXT_GLYPHS
        .get(usize::from(code))
        .or_else(|| SMALL_KANA_GLYPHS.get(usize::from(code.checked_sub(0x84)?)))
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
        assert!(japanese_text_glyph(0x83).is_none());
    }
}
