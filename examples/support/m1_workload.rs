pub const CHAIN: &[(&str, &str, usize)] = &[
    ("enter", "\n", 1),
    ("ctrl-b", "\n", 0),
    ("tab", "\t\n", 1),
    ("ctrl-/", "\n", 0),
    ("a", "a\n", 1),
    ("b", "ab\n", 2),
    ("c", "abc\n", 3),
    ("alt-b", "abc\n", 0),
    ("alt-f", "abc\n", 3),
    ("ctrl-a", "abc\n", 0),
    ("ctrl-space", "abc\n", 0),
    ("ctrl-f", "abc\n", 1),
    ("ctrl-f", "abc\n", 2),
    ("alt-w", "abc\n", 2),
    ("ctrl-w", "c\n", 0),
    ("ctrl-y", "abc\n", 2),
    ("ctrl-/", "c\n", 0),
    ("ctrl-?", "abc\n", 0),
    ("ctrl-e", "abc\n", 3),
    ("ctrl-k", "abc", 3),
    ("ctrl-/", "abc\n", 3),
    ("alt-<", "abc\n", 0),
    ("ctrl-k", "\n", 0),
    ("ctrl-k", "", 0),
    ("ctrl-y", "abc\n", 4),
    ("ctrl-/", "", 0),
    ("ctrl-?", "abc\n", 0),
    ("ctrl-/", "", 0),
    ("ctrl-g", "", 0),
];

pub fn pending_bytes(sent: usize) -> usize {
    let remainder = sent % 64;
    if remainder <= 32 {
        remainder
    } else {
        64 - remainder
    }
}

pub fn stream_key(sent: usize) -> &'static str {
    if sent % 64 < 32 { "x" } else { "backspace" }
}
