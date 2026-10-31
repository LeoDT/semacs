pub const NAMES: [&str; 6] = [
    "mixed",
    "short-ascii",
    "short-unicode",
    "long-ascii",
    "single-ascii",
    "single-unicode",
];

const MIB8: usize = 8 * 1024 * 1024;

pub fn build(name: &str, mixed: &str) -> String {
    match name {
        "mixed" => mixed.to_owned(),
        "short-ascii" => format!("{}\n", "x".repeat(127)).repeat(65_536),
        "short-unicode" => format!("{}z\n", "λ界🙂".repeat(14)).repeat(65_536),
        "long-ascii" => format!("{}\n", "x".repeat(19_455)).repeat(MIB8.div_ceil(19_456)),
        "single-ascii" => "x".repeat(MIB8),
        "single-unicode" => "e\u{301}🙂x".repeat(MIB8 / 8),
        _ => panic!("unknown sample {name}; expected one of {NAMES:?}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn deterministic_shapes() {
        for name in super::NAMES {
            let text = super::build(name, "mixed\n");
            if name == "mixed" {
                assert_eq!(text, "mixed\n");
                continue;
            }
            assert!(text.len() >= super::MIB8);
            assert_eq!(text.contains('\n'), !name.starts_with("single-"));
        }
    }
}
