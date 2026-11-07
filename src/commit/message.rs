use std::collections::VecDeque;
use std::ops::Range;

use crate::text::ByteOffset;

const HISTORY_SIZE: usize = 32;

#[derive(Default)]
pub struct MessageRing {
    messages: VecDeque<String>,
}

#[derive(Default)]
pub struct MessagePosition {
    index: Option<usize>,
}

pub struct RecalledMessage {
    pub range: Range<ByteOffset>,
    pub text: String,
    pub number: usize,
}

impl MessageRing {
    pub fn save(&mut self, text: &str, comment_prefix: &str) -> bool {
        let Some(message) = saved_message(text, comment_prefix) else {
            return false;
        };
        if let Some(index) = self.messages.iter().position(|entry| *entry == message) {
            self.messages.remove(index);
        }
        self.insert(message);
        true
    }

    /// Return an edit for the caller's normal buffer history path. Positive
    /// strides visit older entries, negative strides visit newer entries.
    pub fn recall(
        &mut self,
        position: &mut MessagePosition,
        text: &str,
        comment_prefix: &str,
        stride: i64,
    ) -> Option<RecalledMessage> {
        if self.messages.is_empty() {
            return None;
        }
        let mut stride = i128::from(stride);
        if let Some(message) = saved_message(text, comment_prefix)
            && !self.messages.contains(&message)
        {
            self.insert(message);
            stride += 1;
        }
        let offset = match position.index {
            Some(index) => index as i128 + stride,
            None if stride > 0 => stride - 1,
            None => stride,
        };
        let index = offset.rem_euclid(self.messages.len() as i128) as usize;
        position.index = Some(index);
        let end = text
            .split_inclusive('\n')
            .scan(0, |offset, line| {
                let start = *offset;
                *offset += line.len();
                Some((start, line))
            })
            .find_map(|(start, line)| {
                is_comment(line, comment_prefix).then_some(start.saturating_sub(1))
            })
            .unwrap_or(text.len());
        Some(RecalledMessage {
            range: ByteOffset(0)..ByteOffset(end),
            text: self.messages[index].clone(),
            number: index + 1,
        })
    }

    fn insert(&mut self, message: String) {
        self.messages.push_front(message);
        self.messages.truncate(HISTORY_SIZE);
    }
}

fn is_comment(line: &str, prefix: &str) -> bool {
    !prefix.is_empty() && line.starts_with(prefix)
}

fn is_cut_line(line: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return false;
    }
    let Some(rest) = line
        .strip_prefix(prefix)
        .and_then(|line| line.strip_prefix(' '))
    else {
        return false;
    };
    let Some((left, right)) = rest.split_once(" >8 ") else {
        return false;
    };
    [left, right]
        .into_iter()
        .all(|side| !side.is_empty() && side.bytes().all(|byte| byte == b'-'))
}

fn saved_message(text: &str, comment_prefix: &str) -> Option<String> {
    let mut message = String::new();
    for line in text.split_inclusive('\n') {
        if is_cut_line(line.strip_suffix('\n').unwrap_or(line), comment_prefix) {
            break;
        }
        if !is_comment(line, comment_prefix) {
            message.push_str(line);
        }
    }
    if message.bytes().all(|byte| b" \t\n\r".contains(&byte)) {
        return None;
    }
    let body = message.trim_matches('\n');
    let mut saved = String::with_capacity(body.len() + 2);
    if message.starts_with('\n') {
        saved.push('\n');
    }
    saved.push_str(body);
    saved.push('\n');
    Some(saved)
}

#[cfg(test)]
mod tests;
