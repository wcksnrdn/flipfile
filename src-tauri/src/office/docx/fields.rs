//! Fields and bookmarks, bullet characters and list numbering

/// Word fields can nest (a table of contents is a TOC field full of PAGEREF fields).
#[derive(Default)]
pub(super) struct FieldState {
    pub(super) stack: Vec<FieldFrame>,
}

#[derive(Default)]
pub(super) struct FieldFrame {
    pub(super) instr: String,
    pub(super) in_result: bool,
    pub(super) replaced: bool,
}

/// Bookmark names become Typst labels: keep a safe character set.
pub fn bookmark_label(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

impl FieldState {
    pub(super) fn in_toc(&self) -> bool {
        self.stack.iter().any(|f| f.in_result && field_name(&f.instr) == "TOC")
    }

    /// Text is shown only when it is part of every open field's displayed result and
    /// no open field is being replaced by a live value.
    pub(super) fn visible(&self) -> bool {
        self.stack.iter().all(|f| f.in_result && !f.replaced)
    }
}

/// First word of a field instruction: `PAGEREF _Toc123 \h` -> "PAGEREF"
pub(super) fn field_name(instr: &str) -> String {
    instr.split_whitespace().next().unwrap_or("").to_uppercase()
}

/// Bullets are often stored as private-use characters of the Symbol/Wingdings fonts.
pub(crate) fn bullet_char(text: &str, font: Option<&str>) -> String {
    let font = font.unwrap_or("").to_lowercase();
    let ch = text.chars().next().unwrap_or('•');
    let mapped = match (ch as u32, font.as_str()) {
        (0xF075 | 0x75, "wingdings 3") => '►',
        (0xF07D | 0x7D, "wingdings 3") => '▸',
        (0xF0D8 | 0xD8, "wingdings 3") => '➢',
        (0xF0B7, _) | (0xB7, "symbol") => '•',
        (0xF0A7, _) | (0xA7, "wingdings") => '▪',
        (0xF0D8, _) | (0xD8, "wingdings") => '➢',
        (0xF0FC, _) | (0xFC, "wingdings") => '✓',
        (0xF076, _) | (0x76, "wingdings") => '❖',
        (0xF06E, _) | (0x6E, "wingdings") => '■',
        (0xF0A8, _) => '◆',
        (0xF06C, _) | (0x6C, "wingdings") => '●',
        (0xF071, _) | (0x71, "wingdings") => '❑',
        (0xF0E0, _) | (0xE0, "wingdings") => '➔',
        (_, "courier new") if ch == 'o' => '◦',
        (c, _) if (0xF000..=0xF0FF).contains(&c) => '•',
        _ => ch,
    };
    mapped.to_string()
}

pub(super) fn symbol_char(code: u32) -> char {
    let code = if (0xF000..=0xF0FF).contains(&code) { code - 0xF000 } else { code };
    match code {
        0xB7 => '•',
        0xA7 => '▪',
        0xFC => '✓',
        0xD8 => '➢',
        _ => char::from_u32(code).unwrap_or('•'),
    }
}

pub fn format_number(n: u32, format: &str) -> String {
    match format {
        "lowerLetter" => letters(n, false),
        "upperLetter" => letters(n, true),
        "lowerRoman" => roman(n).to_lowercase(),
        "upperRoman" => roman(n),
        "decimalZero" => format!("{n:02}"),
        "ordinal" => format!("{n}."),
        _ => n.to_string(),
    }
}

pub(super) fn letters(n: u32, upper: bool) -> String {
    // Word repeats letters after z: a..z, aa..zz, aaa…
    let n = n.max(1) - 1;
    let ch = (b'a' + (n % 26) as u8) as char;
    let s: String = std::iter::repeat_n(ch, (n / 26 + 1) as usize).collect();
    if upper { s.to_uppercase() } else { s }
}

fn roman(mut n: u32) -> String {
    if n == 0 || n > 3999 {
        return n.to_string();
    }
    const TABLE: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (v, s) in TABLE {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}
