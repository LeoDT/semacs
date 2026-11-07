#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    YesNo {
        question: String,
        yes: String,
        no: String,
    },
    Username(String),
}

#[derive(Default)]
pub struct Scanner {
    seen: usize,
    line_start: usize,
    recognized: bool,
}

impl Scanner {
    pub fn scan(&mut self, output: &[u8]) -> Option<Prompt> {
        let mut prompt = None;
        for (index, byte) in output[self.seen..].iter().enumerate() {
            if matches!(byte, b'\r' | b'\n') {
                let end = self.seen + index;
                if !self.recognized && prompt.is_none() {
                    prompt = detect(&output[self.line_start..end]);
                }
                self.line_start = end + 1;
                self.recognized = false;
            }
        }
        self.seen = output.len();
        if !self.recognized
            && let Some(tail) = detect(&output[self.line_start..])
        {
            self.recognized = true;
            prompt = prompt.or(Some(tail));
        }
        prompt
    }

    pub fn answered(&mut self, output: &[u8]) {
        self.seen = output.len();
        self.line_start = self.seen;
        self.recognized = false;
    }
}

fn detect(line: &[u8]) -> Option<Prompt> {
    let line = line.strip_suffix(b" ").unwrap_or(line);
    let suffix = line
        .strip_suffix(b"?")
        .or_else(|| line.strip_suffix(b":"))
        .unwrap_or(line);
    let suffix = suffix.strip_suffix(b" ").unwrap_or(suffix);
    if let Some(suffix) = suffix
        .strip_suffix(b"]")
        .or_else(|| suffix.strip_suffix(b")"))
    {
        let suffix = suffix.strip_suffix(b"/[fingerprint]").unwrap_or(suffix);
        for yes in ["Y", "y", "Yes", "yes"] {
            for no in ["N", "n", "No", "no"] {
                for separator in ["/", "|"] {
                    if let Some(prefix) = suffix
                        .strip_suffix(no.as_bytes())
                        .and_then(|prefix| prefix.strip_suffix(separator.as_bytes()))
                        .and_then(|prefix| prefix.strip_suffix(yes.as_bytes()))
                        && let Some(question) = prefix
                            .strip_suffix(b" [")
                            .or_else(|| prefix.strip_suffix(b" ("))
                    {
                        return Some(Prompt::YesNo {
                            question: String::from_utf8_lossy(question).into_owned(),
                            yes: yes.to_ascii_lowercase(),
                            no: no.to_ascii_lowercase(),
                        });
                    }
                }
            }
        }
    }
    if line.starts_with(b"Username for '") && line.ends_with(b"':") {
        return Some(Prompt::Username(format!(
            "{} ",
            String::from_utf8_lossy(line)
        )));
    }
    None
}
