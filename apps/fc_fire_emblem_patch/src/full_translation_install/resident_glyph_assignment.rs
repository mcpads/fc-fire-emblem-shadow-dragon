//! 여러 화면 수명에서 같은 저장 바이트를 읽는 글리프에 하나의 물리 코드를 배정한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

pub(super) fn assign_resident_glyph_codes(
    role: &str,
    forbidden_codes_by_glyph: &BTreeMap<char, BTreeSet<u8>>,
    preassigned_codes_by_glyph: &BTreeMap<char, BTreeSet<u8>>,
    active_codes: &BTreeSet<u8>,
) -> Result<BTreeMap<char, u8>> {
    ensure!(
        !forbidden_codes_by_glyph.is_empty(),
        "{role} has no resident glyphs"
    );
    let candidates = forbidden_codes_by_glyph
        .iter()
        .map(|(glyph, forbidden)| {
            let preassigned = preassigned_codes_by_glyph.get(glyph);
            ensure!(
                preassigned.is_none_or(|codes| codes.len() == 1),
                "{role} glyph {glyph:?} has conflicting preassigned codes"
            );
            let allowed = match preassigned.and_then(|codes| codes.first().copied()) {
                Some(code) => {
                    ensure!(
                        active_codes.contains(&code) && !forbidden.contains(&code),
                        "{role} glyph {glyph:?} cannot keep preassigned code {code:02X}"
                    );
                    BTreeSet::from([code])
                }
                None => active_codes
                    .difference(forbidden)
                    .copied()
                    .collect::<BTreeSet<_>>(),
            };
            ensure!(
                !allowed.is_empty(),
                "{role} glyph {glyph:?} has no code valid across its resident pages"
            );
            Ok((*glyph, allowed))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut glyph_order = candidates.keys().copied().collect::<Vec<_>>();
    glyph_order.sort_by_key(|glyph| (candidates[glyph].len(), *glyph));

    let mut glyph_by_code = BTreeMap::<u8, char>::new();
    for glyph in glyph_order {
        let mut visited_codes = BTreeSet::new();
        ensure!(
            assign_one_glyph_code(glyph, &candidates, &mut glyph_by_code, &mut visited_codes),
            "{role} glyphs have no injective code assignment across resident pages"
        );
    }
    let assignments = glyph_by_code
        .into_iter()
        .map(|(code, glyph)| (glyph, code))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        assignments.len() == candidates.len()
            && assignments
                .iter()
                .all(|(glyph, code)| candidates[glyph].contains(code)),
        "{role} matching returned an invalid code assignment"
    );
    Ok(assignments)
}

fn assign_one_glyph_code(
    glyph: char,
    candidates: &BTreeMap<char, BTreeSet<u8>>,
    glyph_by_code: &mut BTreeMap<u8, char>,
    visited_codes: &mut BTreeSet<u8>,
) -> bool {
    for code in &candidates[&glyph] {
        if !visited_codes.insert(*code) {
            continue;
        }
        let displaced = glyph_by_code.get(code).copied();
        if displaced.is_none_or(|other| {
            assign_one_glyph_code(other, candidates, glyph_by_code, visited_codes)
        }) {
            glyph_by_code.insert(*code, glyph);
            return true;
        }
    }
    false
}

pub(super) fn assignment_sha1(assignments: &BTreeMap<char, u8>) -> String {
    let mut bytes = Vec::with_capacity(assignments.len() * 5);
    for (glyph, code) in assignments {
        bytes.extend_from_slice(&u32::from(*glyph).to_le_bytes());
        bytes.push(*code);
    }
    crate::sha1_hex(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_reserves_the_scarce_code_for_the_constrained_glyph() {
        let active = BTreeSet::from([1, 2]);
        let forbidden = BTreeMap::from([('가', BTreeSet::new()), ('나', BTreeSet::from([2]))]);

        let assignments =
            assign_resident_glyph_codes("synthetic", &forbidden, &BTreeMap::new(), &active)
                .unwrap();

        assert_eq!(assignments[&'나'], 1);
        assert_eq!(assignments[&'가'], 2);
    }

    #[test]
    fn matching_fails_when_two_glyphs_have_only_one_shared_code() {
        let active = BTreeSet::from([1]);
        let forbidden = BTreeMap::from([('가', BTreeSet::new()), ('나', BTreeSet::new())]);

        let error = assign_resident_glyph_codes("synthetic", &forbidden, &BTreeMap::new(), &active)
            .unwrap_err();

        assert!(error.to_string().contains("no injective code assignment"));
    }

    #[test]
    fn a_preassigned_glyph_keeps_its_code() {
        let active = BTreeSet::from([1, 2]);
        let forbidden = BTreeMap::from([('가', BTreeSet::new()), ('나', BTreeSet::new())]);
        let preassigned = BTreeMap::from([('가', BTreeSet::from([2]))]);

        let assignments =
            assign_resident_glyph_codes("synthetic", &forbidden, &preassigned, &active).unwrap();

        assert_eq!(assignments[&'가'], 2);
        assert_eq!(assignments[&'나'], 1);
    }
}
