use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use crate::{
    mapper165::inline_pointer_dispatch::bind_inline_pointer_dispatch, rom::Rom, sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

use super::super::selector_transition_graph::{StateTransition, reachable_selectors};
use super::{
    MAIN_STATE_ADDRESS, NestedMainStateLifecycle, OUTER_SCREEN_BANK, OUTER_SCREEN_STATE_ADDRESS,
    bind_exact_code, scan_raw_direct_state_operands, source_bytes,
};

const SAVE_OFFER_DISPATCH_ENTRY: u16 = 0xB5AC;
const SAVE_OFFER_DISPATCH_CALL: u16 = 0xB5B1;
const SAVE_OFFER_HANDLER_TARGETS: [u16; 10] = [
    0xB5C8, 0xB69D, 0xB6C9, 0xB6E9, 0xB726, 0xB783, 0xB78D, 0xB6F3, 0xB726, 0xB737,
];
const SAVE_COMPLETE_DISPATCH_ENTRY: u16 = 0xB771;
const SAVE_COMPLETE_DISPATCH_CALL: u16 = 0xB776;
const SAVE_COMPLETE_HANDLER_TARGETS: [u16; 5] = [0xB783, 0xB78D, 0xB797, 0xB7B9, 0xB7CB];

const ENTER_SAVE_OFFER: (u16, &[u8]) = (
    0x93B6,
    &[
        0xA9, 0x0D, 0x85, 0x24, // outer screen = save offer
        0xA9, 0x00, 0x85, 0x84, 0x85, 0x26, // main state and substate = zero
        0x60,
    ],
);
const ENTER_SAVE_COMPLETE: (u16, &[u8]) = (
    0xB704,
    &[
        0xA9, 0x0E, 0x85, 0x24, // outer screen = save complete
        0xA9, 0x02, 0x85, 0x84, // enter the shared state-two body
        0x4C, 0xA1, 0xB7,
    ],
);
const ENTER_SAVE_COMPLETE_CONTINUATION: (u16, &[u8]) = (
    0xB7A1,
    &[
        0xA9, 0x01, 0x8D, 0xEE, 0x05, // start the save-complete dialogue
        0x20, 0x8C, 0xB5, // close the previous choice composition
        0xE6, 0x84, // state two becomes state three before the next dispatch
        0xD0, 0x0B,
    ],
);

#[derive(Clone, Copy)]
struct CodeRegion {
    start: u16,
    end: u16,
    sha1: &'static str,
    role: &'static str,
}

const CHAPTER_SAVE_CODE_REGIONS: [CodeRegion; 14] = [
    CodeRegion {
        start: 0x93A0,
        end: 0x93C1,
        sha1: "9844e48533420f569c18bb2f7cd6af4cb63a0b3e",
        role: "enter chapter save-offer lifetime",
    },
    CodeRegion {
        start: 0xB5C8,
        end: 0xB67B,
        sha1: "37c70a01a184d607d4cac39726267a9c3e684695",
        role: "save-offer main state zero",
    },
    CodeRegion {
        start: 0xB67B,
        end: 0xB69D,
        sha1: "7f89c168cb33e2425af50f7f68cab47ac2e67c4b",
        role: "save-offer unit-record preparation",
    },
    CodeRegion {
        start: 0xB69D,
        end: 0xB6C9,
        sha1: "7c2bd90f4f38875627b24b71e477530c4b3fe58c",
        role: "save-offer main state one",
    },
    CodeRegion {
        start: 0xB6C9,
        end: 0xB6E9,
        sha1: "0ae6533d3f6b4332659c186ebab3e2b3dfa58a43",
        role: "save-offer main state two",
    },
    CodeRegion {
        start: 0xB6E9,
        end: 0xB6F3,
        sha1: "37173b41a8e8ec655562b826b8809585774f4d8f",
        role: "save-offer main state three",
    },
    CodeRegion {
        start: 0xB6F3,
        end: 0xB726,
        sha1: "29942463dc2cd16a4429463a8ccb740fa02a85dd",
        role: "save-offer main state seven",
    },
    CodeRegion {
        start: 0xB726,
        end: 0xB737,
        sha1: "0c6886d518bda0859e0d05686bebbfbb9ed27583",
        role: "save-offer main states four and eight",
    },
    CodeRegion {
        start: 0xB737,
        end: 0xB771,
        sha1: "f9049c33cd0a97144a7e6d43960ba701924e1454",
        role: "save-offer main state nine",
    },
    CodeRegion {
        start: 0xB783,
        end: 0xB78D,
        sha1: "dd96e403da1e682f447a0c249ac91d22bc1952b8",
        role: "shared save main-state first page",
    },
    CodeRegion {
        start: 0xB78D,
        end: 0xB797,
        sha1: "c2ea500af4fa414c4c88eb1904743c48c9f358e3",
        role: "shared save main-state second page",
    },
    CodeRegion {
        start: 0xB797,
        end: 0xB7B9,
        sha1: "b80b91126ff0af1a50202828c45f58e27a83524d",
        role: "save-complete choice branch",
    },
    CodeRegion {
        start: 0xB7B9,
        end: 0xB7CB,
        sha1: "1af40f2599d69f77f351e81c17608b2f782141c7",
        role: "save-complete composition wait",
    },
    CodeRegion {
        start: 0xB7CB,
        end: 0xB7EE,
        sha1: "c430e8af28ef3425abdd4e67eec417ae9d8da23e",
        role: "save-complete dialogue wait and exit",
    },
];

const CHAPTER_SAVE_RAW_STATE_OPERANDS: [(u16, u16, u8); 18] = [
    (0xB673, MAIN_STATE_ADDRESS, 0xE6),
    (0xB6C6, MAIN_STATE_ADDRESS, 0xE6),
    (0xB6DA, MAIN_STATE_ADDRESS, 0x85),
    (0xB6E1, MAIN_STATE_ADDRESS, 0xE6),
    (0xB6F0, MAIN_STATE_ADDRESS, 0xE6),
    (0xB706, OUTER_SCREEN_STATE_ADDRESS, 0x85),
    (0xB70A, MAIN_STATE_ADDRESS, 0x85),
    (0xB723, MAIN_STATE_ADDRESS, 0xE6),
    (0xB734, MAIN_STATE_ADDRESS, 0xE6),
    (0xB762, MAIN_STATE_ADDRESS, 0x85),
    (0xB76E, OUTER_SCREEN_STATE_ADDRESS, 0x85),
    (0xB786, MAIN_STATE_ADDRESS, 0xE6),
    (0xB790, MAIN_STATE_ADDRESS, 0xE6),
    (0xB7A9, MAIN_STATE_ADDRESS, 0xE6),
    (0xB7B3, OUTER_SCREEN_STATE_ADDRESS, 0x85),
    (0xB7C8, MAIN_STATE_ADDRESS, 0xE6),
    (0xB7DE, MAIN_STATE_ADDRESS, 0x85),
    (0xB7E2, OUTER_SCREEN_STATE_ADDRESS, 0x85),
];

#[derive(Clone, Copy)]
enum TransitionEncoding {
    Increment { address: u16 },
    StoreImmediate { address: u16 },
}

#[derive(Clone, Copy)]
struct MainStateTransition {
    from: u8,
    to: u8,
    encoding: TransitionEncoding,
}

impl MainStateTransition {
    const fn edge(self) -> StateTransition {
        StateTransition::new(self.from, self.to)
    }
}

const SAVE_OFFER_TRANSITIONS: [MainStateTransition; 10] = [
    increment(0, 0xB673),
    increment(1, 0xB6C6),
    MainStateTransition {
        from: 2,
        to: 8,
        encoding: TransitionEncoding::StoreImmediate { address: 0xB6D8 },
    },
    increment(2, 0xB6E1),
    increment(3, 0xB6F0),
    increment(4, 0xB734),
    increment(5, 0xB786),
    increment(6, 0xB790),
    increment(7, 0xB723),
    increment(8, 0xB734),
];
const SAVE_COMPLETE_ENTRY_TRANSITION: MainStateTransition = increment(2, 0xB7A9);
const SAVE_COMPLETE_TRANSITIONS: [MainStateTransition; 1] = [increment(3, 0xB7C8)];

const fn increment(from: u8, address: u16) -> MainStateTransition {
    MainStateTransition {
        from,
        to: from + 1,
        encoding: TransitionEncoding::Increment { address },
    }
}

pub(super) fn bind_chapter_save_main_state_lifecycles(
    source: &Rom,
) -> Result<Vec<NestedMainStateLifecycle>> {
    source.verify_supported_japanese()?;
    for region in CHAPTER_SAVE_CODE_REGIONS {
        bind_code_region(source, region)?;
    }
    bind_exact_code(
        source,
        OUTER_SCREEN_BANK,
        SAVE_OFFER_DISPATCH_ENTRY,
        &[0x20, 0x88, 0xC2, 0xA5, 0x84, 0x20, 0x4C, 0xC3],
        "dispatch save-offer main state",
    )?;
    bind_exact_code(
        source,
        OUTER_SCREEN_BANK,
        SAVE_COMPLETE_DISPATCH_ENTRY,
        &[0x20, 0x88, 0xC2, 0xA5, 0x84, 0x20, 0x4C, 0xC3],
        "dispatch save-complete main state",
    )?;
    bind_exact_code(
        source,
        OUTER_SCREEN_BANK,
        ENTER_SAVE_OFFER.0,
        ENTER_SAVE_OFFER.1,
        "enter save-offer outer and main states",
    )?;
    bind_exact_code(
        source,
        OUTER_SCREEN_BANK,
        ENTER_SAVE_COMPLETE.0,
        ENTER_SAVE_COMPLETE.1,
        "enter save-complete outer state",
    )?;
    bind_exact_code(
        source,
        OUTER_SCREEN_BANK,
        ENTER_SAVE_COMPLETE_CONTINUATION.0,
        ENTER_SAVE_COMPLETE_CONTINUATION.1,
        "complete save-complete main-state handoff",
    )?;
    ensure!(
        scan_raw_direct_state_operands(source, OUTER_SCREEN_BANK, 0xB5AC, 0xB7EE)?
            == BTreeSet::from(CHAPTER_SAVE_RAW_STATE_OPERANDS),
        "chapter-save lifetime changed its direct outer/main-state operand census"
    );

    let save_offer_handler_domain =
        (0..u8::try_from(SAVE_OFFER_HANDLER_TARGETS.len())?).collect::<BTreeSet<_>>();
    let save_offer_dispatch = bind_inline_pointer_dispatch(
        source,
        OUTER_SCREEN_BANK,
        SAVE_OFFER_DISPATCH_CALL,
        save_offer_handler_domain.iter().copied(),
        "save-offer main-state dispatch",
    )?;
    ensure!(
        save_offer_dispatch.table_start() == 0xB5B4
            && save_offer_dispatch.targets_in_selector_order() == SAVE_OFFER_HANDLER_TARGETS,
        "save-offer main-state handlers changed"
    );
    for transition in SAVE_OFFER_TRANSITIONS {
        bind_transition(source, transition)?;
    }
    let save_offer_produced_selectors = reachable_selectors(
        "save-offer main state",
        &save_offer_handler_domain,
        [0],
        SAVE_OFFER_TRANSITIONS.map(MainStateTransition::edge),
    )?;
    ensure!(
        save_offer_produced_selectors == save_offer_handler_domain,
        "save-offer main-state producer closure no longer reaches its complete owned table"
    );

    let save_complete_handler_domain =
        (0..u8::try_from(SAVE_COMPLETE_HANDLER_TARGETS.len())?).collect::<BTreeSet<_>>();
    let save_complete_dispatch = bind_inline_pointer_dispatch(
        source,
        OUTER_SCREEN_BANK,
        SAVE_COMPLETE_DISPATCH_CALL,
        save_complete_handler_domain.iter().copied(),
        "save-complete main-state dispatch",
    )?;
    ensure!(
        save_complete_dispatch.table_start() == 0xB779
            && save_complete_dispatch.targets_in_selector_order() == SAVE_COMPLETE_HANDLER_TARGETS,
        "save-complete main-state handlers changed"
    );
    bind_transition(source, SAVE_COMPLETE_ENTRY_TRANSITION)?;
    for transition in SAVE_COMPLETE_TRANSITIONS {
        bind_transition(source, transition)?;
    }
    let save_complete_entry_selector = SAVE_COMPLETE_ENTRY_TRANSITION.to;
    let save_complete_produced_selectors = reachable_selectors(
        "save-complete main state",
        &save_complete_handler_domain,
        [save_complete_entry_selector],
        SAVE_COMPLETE_TRANSITIONS.map(MainStateTransition::edge),
    )?;
    ensure!(
        save_complete_produced_selectors == BTreeSet::from([0x03, 0x04]),
        "save-complete producer closure admitted dormant table entries or lost its active states"
    );

    Ok(vec![
        NestedMainStateLifecycle {
            dispatch_call: SAVE_OFFER_DISPATCH_CALL,
            handler_domain: save_offer_handler_domain,
            produced_selectors: Some(save_offer_produced_selectors),
        },
        NestedMainStateLifecycle {
            dispatch_call: SAVE_COMPLETE_DISPATCH_CALL,
            handler_domain: save_complete_handler_domain,
            produced_selectors: Some(save_complete_produced_selectors),
        },
    ])
}

fn bind_code_region(source: &Rom, region: CodeRegion) -> Result<()> {
    let byte_count = usize::from(region.end - region.start);
    let bytes = source_bytes(source, OUTER_SCREEN_BANK, region.start, byte_count)?;
    ensure!(
        sha1_hex(bytes) == region.sha1,
        "{} source bytes changed",
        region.role
    );
    decode_rp2a03_sequence(bytes, region.start, region.role)?;
    Ok(())
}

fn bind_transition(source: &Rom, transition: MainStateTransition) -> Result<()> {
    match transition.encoding {
        TransitionEncoding::Increment { address } => {
            ensure!(
                transition.to == transition.from.wrapping_add(1),
                "main-state increment transition does not advance exactly once"
            );
            bind_exact_code(
                source,
                OUTER_SCREEN_BANK,
                address,
                &[0xE6, MAIN_STATE_ADDRESS as u8],
                "advance chapter-save main state",
            )?;
        }
        TransitionEncoding::StoreImmediate { address } => {
            bind_exact_code(
                source,
                OUTER_SCREEN_BANK,
                address,
                &[0xA9, transition.to, 0x85, MAIN_STATE_ADDRESS as u8],
                "select chapter-save main state",
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_offer_state_machine_reaches_every_owned_handler() {
        let handlers = (0..u8::try_from(SAVE_OFFER_HANDLER_TARGETS.len()).unwrap()).collect();
        assert_eq!(
            reachable_selectors(
                "save-offer main state",
                &handlers,
                [0],
                SAVE_OFFER_TRANSITIONS.map(MainStateTransition::edge),
            )
            .unwrap(),
            handlers
        );
    }

    #[test]
    fn save_complete_entry_does_not_promote_dormant_table_entries() {
        let handlers = (0..u8::try_from(SAVE_COMPLETE_HANDLER_TARGETS.len()).unwrap()).collect();
        assert_eq!(
            reachable_selectors(
                "save-complete main state",
                &handlers,
                [SAVE_COMPLETE_ENTRY_TRANSITION.to],
                SAVE_COMPLETE_TRANSITIONS.map(MainStateTransition::edge),
            )
            .unwrap(),
            BTreeSet::from([0x03, 0x04])
        );
    }
}
