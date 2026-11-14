//! Script properties used by shaping (`unicode_properties.g.ts` / `script_detection.ts`),
//! as code point ranges.

fn in_ranges(cp: u32, ranges: &[(u32, u32)]) -> bool {
    ranges.iter().any(|&(a, b)| (a..=b).contains(&cp))
}

/// JS `/\s/u`.
pub fn is_whitespace(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// `codePointAllowsIdeographicBreaking`.
pub fn allows_ideographic_breaking(c: char) -> bool {
    const R: &[(u32, u32)] = &[
        (0x02EA, 0x02EB), (0x2E80, 0x2FDF), (0x2FF0, 0x303F), (0x3041, 0x3096), (0x309D, 0x309F),
        (0x30A1, 0x30FA), (0x30FD, 0x30FF), (0x3105, 0x312F), (0x31A0, 0x4DBF), (0x4E00, 0xA48C),
        (0xA490, 0xA4C6), (0xF900, 0xFA6D), (0xFA70, 0xFAD9), (0xFE10, 0xFE1F), (0xFE30, 0xFE4F),
        (0xFF00, 0xFFEF), (0x16FE0, 0x18CD5), (0x1AFF0, 0x1B2FB), (0x1F200, 0x1F2FF), (0x20000, 0x323AF),
    ];
    in_ranges(c as u32, R)
}

/// `codePointHasUprightVerticalOrientation`.
pub fn has_upright_vertical_orientation(c: char) -> bool {
    const R: &[(u32, u32)] = &[
        (0x02EA, 0x02EB), (0x1100, 0x11FF), (0x1401, 0x167F), (0x18B0, 0x18FF), (0x2065, 0x2065),
        (0x20DD, 0x20E0), (0x20E2, 0x20E4), (0x2B97, 0x2B97), (0x2BF0, 0x2BFF), (0x2E50, 0x2E51),
        (0x2E80, 0x3007), (0x3012, 0x3013), (0x3020, 0x302F), (0x3031, 0x30FB), (0x30FD, 0xA4CF),
        (0xA960, 0xA97F), (0xAC00, 0xD7FF), (0xF900, 0xFAFF), (0xFE10, 0xFE1F), (0xFE30, 0xFE48),
        (0xFE50, 0xFE57), (0xFE5F, 0xFE62), (0xFE67, 0xFE6F), (0xFF00, 0xFF07), (0xFF0A, 0xFF0C),
        (0xFF0E, 0xFF19), (0xFF1F, 0xFF3A), (0xFF3C, 0xFF3C), (0xFF3E, 0xFF3E), (0xFF40, 0xFF5A),
        (0xFFE0, 0xFFE2), (0xFFE4, 0xFFE7), (0xFFF0, 0xFFF8), (0x1F000, 0x1FAFF), (0x20000, 0x3FFFD),
    ];
    in_ranges(c as u32, R)
}

/// `codePointIsInCursiveScript` (letter spacing is not applied to such text).
pub fn is_cursive(c: char) -> bool {
    const R: &[(u32, u32)] = &[
        (0x0600, 0x0604), (0x0606, 0x060B), (0x060D, 0x061A), (0x061C, 0x061E), (0x0620, 0x063F),
        (0x0641, 0x064A), (0x0656, 0x066F), (0x0671, 0x06DC), (0x06DE, 0x070D), (0x070F, 0x074A),
        (0x074D, 0x077F), (0x07C0, 0x07FA), (0x07FD, 0x07FF), (0x0840, 0x085B), (0x085E, 0x085E),
        (0x0860, 0x086A), (0x0870, 0x0891), (0x0897, 0x08E1), (0x08E3, 0x08FF), (0x1800, 0x1801),
        (0x1804, 0x1804), (0x1806, 0x1819), (0x1820, 0x1878), (0x1880, 0x18AA), (0xA840, 0xA877),
        (0xFB50, 0xFD3D), (0xFD40, 0xFDCF), (0xFDF0, 0xFDFF), (0xFE70, 0xFE74), (0xFE76, 0xFEFC),
    ];
    in_ranges(c as u32, R)
}

pub fn allows_letter_spacing(text: &str) -> bool {
    !text.chars().any(is_cursive)
}

pub fn allows_vertical_writing_mode(text: &str) -> bool {
    text.chars().any(has_upright_vertical_orientation)
}

/// `codePointHasNeutralVerticalOrientation`.
pub fn has_neutral_vertical_orientation(c: char) -> bool {
    const R: &[(u32, u32)] = &[
        (0xA7, 0xA7), (0xA9, 0xA9), (0xAE, 0xAE), (0xB1, 0xB1), (0xBC, 0xBE), (0xD7, 0xD7), (0xF7, 0xF7),
        (0x2016, 0x2016), (0x2020, 0x2021), (0x2030, 0x2031), (0x203B, 0x203C), (0x2042, 0x2042),
        (0x2047, 0x2049), (0x2051, 0x2051), (0x2100, 0x218F), (0x221E, 0x221E), (0x2234, 0x2235),
        (0x2300, 0x2307), (0x230C, 0x231F), (0x2324, 0x2328), (0x232B, 0x232B), (0x237D, 0x239A),
        (0x23BE, 0x23CD), (0x23CF, 0x23CF), (0x23D1, 0x23DB), (0x23E2, 0x2422), (0x2424, 0x24FF),
        (0x25A0, 0x2619), (0x2620, 0x2767), (0x2776, 0x2793), (0x2B12, 0x2B2F), (0x2B50, 0x2B59),
        (0x2BB8, 0x2BEB), (0x3000, 0x303F), (0x30A0, 0x30FF), (0xE000, 0xF8FF), (0xFE30, 0xFE6F),
        (0xFF00, 0xFFEF), (0xFFFC, 0xFFFD), (0xF0000, 0x10FFFF),
    ];
    in_ranges(c as u32, R)
}

/// `charHasRotatedVerticalOrientation`.
pub fn has_rotated_vertical_orientation(c: char) -> bool {
    !(has_upright_vertical_orientation(c) || has_neutral_vertical_orientation(c))
}

/// `\p{sc=Arab}` (complex shaping for vertical placement).
pub fn is_arabic(c: char) -> bool {
    in_ranges(c as u32, &[(0x0600, 0x06FF), (0x0750, 0x077F), (0x0870, 0x08FF), (0xFB50, 0xFDFF), (0xFE70, 0xFEFF)])
}

/// `[\p{P}\p{S}]`, approximated as anything that is not a letter, number, mark, space or control.
pub fn is_symbol_or_punctuation(c: char) -> bool {
    !c.is_alphanumeric() && !c.is_whitespace() && !c.is_control() && !unicode_normalization::char::is_combining_mark(c)
}

/// `\p{Nd}` (approximated by numeric characters with a decimal value or in digit blocks).
pub fn is_decimal_digit(c: char) -> bool {
    c.is_ascii_digit() || ('\u{ff10}'..='\u{ff19}').contains(&c) || (c.is_numeric() && !matches!(c as u32, 0xB2 | 0xB3 | 0xB9 | 0xBC..=0xBE | 0x2150..=0x218F | 0x2460..=0x24FF | 0x3007 | 0x3021..=0x3029 | 0x4E00..=0x9FFF))
}

/// `\p{Lu}`.
pub fn is_uppercase_letter(c: char) -> bool {
    c.is_uppercase() && c.is_alphabetic()
}

/// `verticalizedCharacterMap`.
pub fn verticalized(c: char) -> Option<char> {
    const MAP: &[(char, char)] = &[
        ('!', '︕'), ('#', '＃'), ('$', '＄'), ('%', '％'), ('&', '＆'), ('(', '︵'), (')', '︶'), ('*', '＊'), ('+', '＋'),
        (',', '︐'), ('-', '︲'), ('.', '・'), ('/', '／'), (':', '︓'), (';', '︔'), ('<', '︿'), ('=', '＝'), ('>', '﹀'),
        ('?', '︖'), ('@', '＠'), ('[', '﹇'), ('\\', '＼'), (']', '﹈'), ('^', '＾'), ('_', '︳'), ('`', '｀'), ('{', '︷'),
        ('|', '―'), ('}', '︸'), ('~', '～'), ('¢', '￠'), ('£', '￡'), ('¥', '￥'), ('¦', '￤'), ('¬', '￢'), ('¯', '￣'),
        ('–', '︲'), ('—', '︱'), ('‘', '﹃'), ('’', '﹄'), ('“', '﹁'), ('”', '﹂'), ('…', '︙'), ('⋯', '︙'), ('‧', '・'),
        ('₩', '￦'), ('、', '︑'), ('。', '︒'), ('〈', '︿'), ('〉', '﹀'), ('《', '︽'), ('》', '︾'), ('「', '﹁'),
        ('」', '﹂'), ('『', '﹃'), ('』', '﹄'), ('【', '︻'), ('】', '︼'), ('〔', '︹'), ('〕', '︺'), ('〖', '︗'),
        ('〗', '︘'), ('！', '︕'), ('（', '︵'), ('）', '︶'), ('，', '︐'), ('－', '︲'), ('．', '・'), ('：', '︓'),
        ('；', '︔'), ('＜', '︿'), ('＞', '﹀'), ('？', '︖'), ('［', '﹇'), ('］', '﹈'), ('＿', '︳'), ('｛', '︷'),
        ('｜', '―'), ('｝', '︸'), ('｟', '︵'), ('｠', '︶'), ('｡', '︒'), ('｢', '﹁'), ('｣', '﹂'),
    ];
    MAP.iter().find(|m| m.0 == c).map(|m| m.1)
}

/// `verticalizePunctuation`: punctuation between upright characters takes its vertical form.
pub fn verticalize_punctuation(chars: &[char]) -> Vec<char> {
    let keeps = |c: Option<char>| c.is_none_or(|c| !has_rotated_vertical_orientation(c) || verticalized(c).is_some());
    (0..chars.len())
        .map(|i| {
            let (prev, next) = (i.checked_sub(1).map(|j| chars[j]), chars.get(i + 1).copied());
            match verticalized(chars[i]) {
                Some(v) if keeps(prev) && keeps(next) => v,
                _ => chars[i],
            }
        })
        .collect()
}
