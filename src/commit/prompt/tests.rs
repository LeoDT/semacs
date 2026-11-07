use super::*;

#[test]
fn known_non_secret_prompts_match_upstream_token_spellings() {
    for (input, yes, no) in [
        ("Continue? [Y/n] ", "y", "n"),
        ("Continue (yes/no)? ", "yes", "no"),
        ("Continue [Yes|No]:", "yes", "no"),
        ("Trust host (yes/no/[fingerprint])? ", "yes", "no"),
        ("Continue [Y/No] ?", "y", "no"),
    ] {
        let Some(Prompt::YesNo {
            question,
            yes: actual_yes,
            no: actual_no,
        }) = detect(input.as_bytes())
        else {
            panic!("{input}");
        };
        assert!(!question.is_empty());
        assert_eq!(actual_yes, yes);
        assert_eq!(actual_no, no);
    }
    assert_eq!(
        detect(b"Username for 'https://example.invalid':"),
        Some(Prompt::Username(
            "Username for 'https://example.invalid': ".into()
        ))
    );
    for input in [
        "Password: ",
        "Passphrase for key: ",
        "Token: ",
        "Enter PIN for card: ",
        "Continue [YES/NO]",
        "Continue [y/n] later",
        "Username: ",
        "unrecognized: ",
    ] {
        assert_eq!(detect(input.as_bytes()), None, "{input}");
    }
}

#[test]
fn chunks_utf8_crlf_and_late_prompt_suffix_do_not_duplicate_reads() {
    let prompt = "log\r\n\u{4e2d}\u{6587} [y/n]";
    for split in 0..=prompt.len() {
        let mut scanner = Scanner::default();
        assert_eq!(
            scanner.scan(&prompt.as_bytes()[..split]).is_some(),
            split == prompt.len()
        );
        let result = scanner.scan(prompt.as_bytes());
        assert_eq!(result.is_some(), split != prompt.len());
        let extended = format!("{prompt}: ");
        assert_eq!(scanner.scan(extended.as_bytes()), None);
        scanner.answered(extended.as_bytes());
        let again = format!("{extended}Again [yes/no]");
        assert!(
            matches!(scanner.scan(again.as_bytes()), Some(Prompt::YesNo { question, .. }) if question == "Again")
        );
    }
    for prompt in ["Continue [y/n]\r\n", "Username for 'host': \n"] {
        for split in 0..=prompt.len() {
            let mut scanner = Scanner::default();
            let first = scanner.scan(&prompt.as_bytes()[..split]);
            let second = scanner.scan(prompt.as_bytes());
            assert_eq!(
                usize::from(first.is_some()) + usize::from(second.is_some()),
                1
            );
        }
    }
    let output = b"progress\rUsername for 'host': ";
    assert!(matches!(
        Scanner::default().scan(output),
        Some(Prompt::Username(_))
    ));
}

#[test]
fn ordinary_output_and_newlines_do_not_reuse_an_old_prompt() {
    let mut scanner = Scanner::default();
    assert!(scanner.scan(b"Continue [y/n]").is_some());
    assert_eq!(scanner.scan(b"Continue [y/n]\nnormal output\n"), None);
    assert!(
        scanner
            .scan(b"Continue [y/n]\nnormal output\nUsername for 'host': ")
            .is_some()
    );
}
