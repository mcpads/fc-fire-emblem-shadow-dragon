use std::{
    collections::{BTreeMap, BTreeSet},
    ops::RangeInclusive,
};

use anyhow::{Result, ensure};

use crate::{
    mapper165::battle_codebook_plan::IndirectWriteDestinationBounds, rom::Rom,
    shop_flow::SharedMenuControllerSource,
};

mod dimension_bounds;
mod source_regions;

use dimension_bounds::{
    MENU_CACHE_BASES, bind_menu_dimension_producers, cache_destination_ranges,
    row_marker_destination_range,
};
use source_regions::{bind_request_state_landmarks, bind_source_regions, source_bytes};

const MENU_RECORD_BASES: [u16; 5] = [0x7EB0, 0x7EB6, 0x7EBC, 0x7EC2, 0x7EC8];
const MENU_RECORD_POINTER_TABLE: u16 = 0x93F0;
const MENU_RECORD_STORE_SITES: [(u16, u8); 6] = [
    (0x940A, 0),
    (0x9410, 1),
    (0x9415, 2),
    (0x941A, 3),
    (0x941F, 4),
    (0x9424, 5),
];

const MENU_PROJECTION_STORE: (u8, u16, u8) = (0x06, 0xAD23, 0x04);
const MENU_PAIR_STORES: [(u8, u16, u8); 2] = [(0x06, 0xB957, 0x00), (0x06, 0xB960, 0x00)];
const MENU_ROW_MARKER_STORE: (u8, u16, u8) = (0x0B, 0x9517, 0x02);
const MENU_QUEUE_COPY_STORE: (u8, u16, u8) = (0x0B, 0x95DD, 0x04);
const MENU_CACHE_COPY_STORE: (u8, u16, u8) = (0x0B, 0x98FE, 0x02);
const MENU_QUEUE_DESTINATION_START: u16 = 0x7ECE;
const MENU_QUEUE_DESTINATION_END: u16 = 0x7F6B;
const MENU_CACHE_POINTER_TABLE: u16 = 0x992D;

pub(super) struct SharedMenuExecutionSource {
    dispatch_call: u16,
    active_request_states: BTreeSet<u8>,
    indirect_write_destinations: BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
}

impl SharedMenuExecutionSource {
    pub(super) fn dispatch_call(&self) -> u16 {
        self.dispatch_call
    }

    pub(super) fn active_request_states(&self) -> &BTreeSet<u8> {
        &self.active_request_states
    }

    pub(super) fn indirect_write_destinations(
        &self,
    ) -> &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds> {
        &self.indirect_write_destinations
    }
}

pub(super) fn bind_shared_menu_execution_source(
    source: &Rom,
    shared_menu: &SharedMenuControllerSource,
) -> Result<SharedMenuExecutionSource> {
    ensure!(
        shared_menu.dispatch_call() == 0x9254,
        "shared-menu source owner no longer binds the controller dispatch"
    );
    bind_source_regions(source)?;
    bind_request_state_landmarks(source)?;
    let dimension_bounds = bind_menu_dimension_producers(source)?;

    let pointer_table = source_bytes(source, 0x0B, MENU_RECORD_POINTER_TABLE, 10)?;
    let actual_bases = pointer_table
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        actual_bases == MENU_RECORD_BASES,
        "shared-menu record pointer table changed"
    );

    let mut bounds = BTreeMap::new();
    insert_bound(
        &mut bounds,
        MENU_PROJECTION_STORE,
        "shared-menu eight-record projection",
        vec![0x7ECE..=0x7F4D],
    )?;
    for site in MENU_PAIR_STORES {
        insert_bound(
            &mut bounds,
            site,
            "shared-menu two-byte map projection",
            vec![0x0311..=0x0312],
        )?;
    }
    for (address, record_offset) in MENU_RECORD_STORE_SITES {
        let ranges = MENU_RECORD_BASES
            .iter()
            .map(|base| {
                let address = base + u16::from(record_offset);
                address..=address
            })
            .collect();
        insert_bound(
            &mut bounds,
            (0x0B, address, 0x6E),
            "shared-menu five-record workspace",
            ranges,
        )?;
    }
    insert_bound(
        &mut bounds,
        MENU_ROW_MARKER_STORE,
        "shared-menu row-marker buffer",
        vec![row_marker_destination_range(
            dimension_bounds.maximum_width,
        )?],
    )?;
    insert_bound(
        &mut bounds,
        MENU_QUEUE_COPY_STORE,
        "shared-menu normalized PPU queue destination",
        vec![MENU_QUEUE_DESTINATION_START..=MENU_QUEUE_DESTINATION_END],
    )?;

    let cache_table = source_bytes(source, 0x0B, MENU_CACHE_POINTER_TABLE, 10)?;
    let actual_cache_bases = cache_table
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        actual_cache_bases == MENU_CACHE_BASES,
        "shared-menu cache pointer table changed"
    );
    insert_bound(
        &mut bounds,
        MENU_CACHE_COPY_STORE,
        "shared-menu five-slot screen cache",
        cache_destination_ranges(
            dimension_bounds.maximum_width,
            dimension_bounds.maximum_row_count,
        )?,
    )?;
    Ok(SharedMenuExecutionSource {
        dispatch_call: shared_menu.dispatch_call(),
        // State zero is idle. State six is a helper called by the active handlers rather than a
        // pending request published through $E65C. The source regions and landmarks above bind
        // the request lifecycle that reaches states one through five.
        active_request_states: (1..=5).collect(),
        indirect_write_destinations: bounds,
    })
}

fn insert_bound(
    bounds: &mut BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    site: (u8, u16, u8),
    role: &'static str,
    destination_ranges: Vec<RangeInclusive<u16>>,
) -> Result<()> {
    ensure!(
        bounds
            .insert(
                site,
                IndirectWriteDestinationBounds::from_source_ranges(role, destination_ranges)?,
            )
            .is_none(),
        "shared-menu indirect write site is duplicated at {:02X}:${:04X}",
        site.0,
        site.1,
    );
    Ok(())
}
