use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use crate::font_slots::PRESERVED_DISPLAY_CODES;

pub(super) fn select_source_safe_borrowed_codes(
    required_count: usize,
    preserved_literal_codes: &BTreeSet<u8>,
) -> Result<BTreeSet<u8>> {
    let borrowed_codes = PRESERVED_DISPLAY_CODES
        .into_iter()
        .filter(|code| !preserved_literal_codes.contains(code))
        .take(required_count)
        .collect::<BTreeSet<_>>();
    ensure!(
        borrowed_codes.len() == required_count,
        "battle logical codebook needs {required_count} borrowed source codes but only {} preserved-display codes are absent from every preserved battle literal",
        borrowed_codes.len(),
    );
    Ok(borrowed_codes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserved_literals_are_never_borrowed() {
        let preserved_literal_codes = PRESERVED_DISPLAY_CODES
            .iter()
            .copied()
            .take(2)
            .collect::<BTreeSet<_>>();

        let borrowed = select_source_safe_borrowed_codes(2, &preserved_literal_codes).unwrap();

        assert_eq!(borrowed.len(), 2);
        assert!(borrowed.is_disjoint(&preserved_literal_codes));
        assert!(
            borrowed
                .iter()
                .all(|code| PRESERVED_DISPLAY_CODES.contains(code))
        );
    }

    #[test]
    fn unavailable_source_safe_codes_fail_closed() {
        let preserved_literal_codes = PRESERVED_DISPLAY_CODES.into_iter().collect();

        assert!(select_source_safe_borrowed_codes(1, &preserved_literal_codes).is_err());
    }
}
