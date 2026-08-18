use std::collections::BTreeSet;

use anyhow::Result;

use crate::rom::Rom;

use super::{EVENT_CODE_TARGETS, bind_exact_code};

pub(super) fn bind_computed_selector_sources(source: &Rom) -> Result<()> {
    bind_exact_code(
        source,
        0x8400,
        &[
            0xA0, 0x16, 0xB1, 0x9D, 0x29, 0x0C, 0x4A, 0x4A, 0x85, 0xA9, 0x20, 0x4C, 0xC3,
        ],
        "derive the unit-kind selector from record field sixteen",
    )?;
    bind_exact_code(
        source,
        0x8424,
        &[0xA9, 0x00, 0x85, 0xAB],
        "write direction zero",
    )?;
    bind_exact_code(
        source,
        0x846C,
        &[0xA9, 0x07, 0x85, 0xAB],
        "write guarded direction seven",
    )?;
    bind_exact_code(
        source,
        0x8473,
        &[0xA5, 0xAC, 0x29, 0x1C, 0x4A, 0x4A, 0x85, 0xAB],
        "derive direction from the lower record class",
    )?;
    bind_exact_code(
        source,
        0x8511,
        &[0xA9, 0x01, 0x85, 0xAB],
        "write direction one",
    )?;
    bind_exact_code(
        source,
        0x853C,
        &[0xA5, 0xAC, 0x29, 0x1C, 0x4A, 0x4A, 0x85, 0xAB],
        "derive direction for the second unit-kind handler",
    )?;
    bind_exact_code(
        source,
        0x8890,
        &[0xA5, 0xAC, 0x29, 0x70, 0x4A, 0x4A, 0x4A, 0x4A, 0x85, 0xAB],
        "derive the guarded map direction",
    )?;
    bind_exact_code(
        source,
        0x8921,
        &[0xA5, 0xAB, 0xC9, 0x07, 0xD0, 0x0E],
        "exclude direction seven before neighbor dispatch",
    )?;
    bind_exact_code(
        source,
        0x8927,
        &[
            0xC4, 0xC2, 0xD0, 0x07, 0xE4, 0xC3, 0xD0, 0x03, 0x4C, 0x79, 0x8A, 0x4C, 0x7D, 0x8A,
        ],
        "route direction seven away from neighbor dispatch",
    )?;
    bind_exact_code(
        source,
        0x8968,
        &[0x4C, 0xBB, 0x89],
        "route guarded directions to their handler table",
    )?;
    bind_exact_code(
        source,
        0x9B46,
        &[0xAD, 0x3B, 0x05, 0xC9, 0x80, 0x90, 0x62],
        "bound event code below eighty before dispatch",
    )?;
    bind_exact_code(
        source,
        0x9BAF,
        &[
            0xAD, 0x3B, 0x05, 0x38, 0xE9, 0x78, 0x90, 0x13, 0x20, 0x4C, 0xC3,
        ],
        "project event codes 0x78 through 0x7F",
    )?;
    Ok(())
}

pub(super) fn unit_kind_selector_domain() -> BTreeSet<u8> {
    (u8::MIN..=u8::MAX)
        .map(|value| (value & 0x0C) >> 2)
        .collect()
}

pub(super) fn map_direction_selector_domain() -> BTreeSet<u8> {
    (u8::MIN..=u8::MAX)
        .filter_map(map_direction_selector)
        .collect()
}

fn map_direction_selector(value: u8) -> Option<u8> {
    let selector = (value & 0x70) >> 4;
    (selector != 7).then_some(selector)
}

pub(super) fn event_code_selector_domain() -> BTreeSet<u8> {
    (u8::MIN..=u8::MAX)
        .filter_map(event_code_selector)
        .collect()
}

fn event_code_selector(value: u8) -> Option<u8> {
    value
        .checked_sub(0x78)
        .filter(|selector| *selector < EVENT_CODE_TARGETS.len() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computed_selectors_fill_only_their_handler_domains() {
        assert_eq!(unit_kind_selector_domain(), BTreeSet::from([0, 1, 2, 3]));
        assert_eq!(
            map_direction_selector_domain(),
            (0..=6).collect::<BTreeSet<_>>()
        );
        assert_eq!(
            event_code_selector_domain(),
            (0..=7).collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn direction_seven_and_out_of_range_events_never_select_a_handler() {
        assert_eq!(map_direction_selector(0x70), None);
        assert_eq!(map_direction_selector(0xF0), None);
        assert_eq!(event_code_selector(0x77), None);
        assert_eq!(event_code_selector(0x78), Some(0));
        assert_eq!(event_code_selector(0x7F), Some(7));
        assert_eq!(event_code_selector(0x80), None);
    }
}
