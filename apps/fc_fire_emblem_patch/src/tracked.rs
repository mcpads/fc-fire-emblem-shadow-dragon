use std::ops::Range;

use anyhow::{Result, ensure};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteReport {
    pub label: String,
    pub offset: usize,
    pub len: usize,
}

#[derive(Debug, Clone)]
pub struct TrackedImage {
    data: Vec<u8>,
    writes: Vec<WriteReport>,
    ranges: Vec<Range<usize>>,
}

impl TrackedImage {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            writes: Vec::new(),
            ranges: Vec::new(),
        }
    }

    pub fn write_expected(
        &mut self,
        label: impl Into<String>,
        offset: usize,
        expected: &[u8],
        replacement: &[u8],
    ) -> Result<()> {
        ensure!(
            expected.len() == replacement.len(),
            "expected write length mismatch"
        );
        let end = offset
            .checked_add(expected.len())
            .ok_or_else(|| anyhow::anyhow!("expected write range overflow"))?;
        ensure!(
            end <= self.data.len(),
            "expected write is outside the image"
        );
        ensure!(
            self.data[offset..end] == *expected,
            "expected write precondition failed at {offset:#X}: expected {}, found {}",
            hex(expected),
            hex(&self.data[offset..end])
        );
        ensure!(
            self.ranges
                .iter()
                .all(|range| end <= range.start || offset >= range.end),
            "expected write overlaps a previous tracked write"
        );

        self.data[offset..end].copy_from_slice(replacement);
        self.writes.push(WriteReport {
            label: label.into(),
            offset,
            len: expected.len(),
        });
        self.ranges.push(offset..end);
        Ok(())
    }

    pub fn verify_all_changes_tracked(&self, source: &[u8]) -> Result<()> {
        ensure!(self.data.len() == source.len(), "source size mismatch");
        for (offset, (before, after)) in source.iter().zip(&self.data).enumerate() {
            if before != after {
                ensure!(
                    self.ranges.iter().any(|range| range.contains(&offset)),
                    "untracked write at {offset:#X}: {before:02X} -> {after:02X}"
                );
            }
        }
        Ok(())
    }

    pub fn writes(&self) -> &[WriteReport] {
        &self.writes
    }

    pub fn into_data(self) -> Vec<u8> {
        self.data
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_change_only_after_its_original_bytes_match() {
        let source = vec![0_u8; 8];
        let mut image = TrackedImage::new(source.clone());
        image
            .write_expected("option text", 2, &[0, 0], &[1, 2])
            .unwrap();

        image.verify_all_changes_tracked(&source).unwrap();
        assert_eq!(image.writes()[0].label, "option text");
    }

    #[test]
    fn rejects_a_wrong_original_or_overlapping_write() {
        let mut image = TrackedImage::new(vec![0_u8; 8]);
        assert!(
            image
                .write_expected("wrong original", 0, &[1], &[2])
                .is_err()
        );
        image
            .write_expected("first range", 1, &[0, 0], &[1, 1])
            .unwrap();
        assert!(image.write_expected("overlap", 2, &[1], &[2]).is_err());
    }
}
