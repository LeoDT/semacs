use super::FileDiff;

impl FileDiff {
    pub(crate) fn hunk_patch(&self, index: usize) -> Result<String, String> {
        let hunk = self.hunks.get(index).ok_or("No text hunk selected")?;
        if self.summary.is_some() || !matches!(self.status, 'A' | 'D' | 'M') {
            return Err("This change has no supported text hunk".into());
        }
        let mut patch: String = self
            .patch_header
            .split_inclusive('\n')
            .filter(|line| !line.starts_with("old mode ") && !line.starts_with("new mode "))
            .collect();
        let fields: Vec<_> = hunk.header.splitn(4, ' ').collect();
        let new = fields.get(2).ok_or("Malformed unified hunk")?;
        let count = new.split_once(',').map_or("", |(_, count)| count);
        patch.push_str(&format!(
            "{} {} +{}{} {}\n",
            fields[0],
            fields[1],
            hunk.old_start,
            if count.is_empty() {
                String::new()
            } else {
                format!(",{count}")
            },
            fields.get(3).ok_or("Malformed unified hunk")?,
        ));
        for line in &hunk.lines {
            patch.push_str(&line.text);
        }
        Ok(patch)
    }
}
