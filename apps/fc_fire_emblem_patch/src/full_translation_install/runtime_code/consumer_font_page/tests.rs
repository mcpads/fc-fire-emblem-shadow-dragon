use super::*;
use crate::{
    front_end_menu::{
        RECORD_ACTION_COMPOSITE_STATE, RECORD_LIST_COMPOSITE_STATE,
        SAVE_SLOT_SELECTION_COMPOSITE_STATE, START_MENU_COMPOSITE_STATE,
    },
    full_translation_install::screen_font_residency::{
        ITEM_ACTION_COMPOSITE_STATE, STORAGE_ITEM_DETAIL_COMPOSITE_STATE,
        UNIT_STATUS_COMPOSITE_STATE, UNIT_SUMMARY_COMPOSITE_STATE,
    },
    shop_flow::SHOP_ITEM_COMPOSITE_STATE,
};

const ORIGIN: u16 = 0xF620;
const APPLY_ROUTE: u16 = 0xF900;
const RESTORE_SOURCE_PAIR: u16 = 0xF360;
const REUSED_STATE_WITHOUT_FONT_OWNERSHIP: u8 = 0x20;
const STATUS_CARRY: u8 = 0x01;
const STATUS_ZERO: u8 = 0x02;

fn pages() -> ScreenFontPageRoutes {
    ScreenFontPageRoutes {
        front_end_menu: 0xA9,
        front_end_record_action: 0xDD,
        unit_command: 0xCC,
        map_menu: 0xD0,
        ending_record: 0xD9,
        chapter_save_offer: 0xD4,
        catalog: [0xDC, 0xE0],
    }
}

fn storage_item_list_route() -> StorageItemListRuntimeRoute {
    StorageItemListRuntimeRoute {
        caller_state_address: 0x05DB,
        composition_state: 0x06,
        facility_composite_state: UNIT_ITEM_LIST_COMPOSITE_STATE,
        overflow_composite_state: STORAGE_ITEM_DETAIL_COMPOSITE_STATE,
    }
}

#[derive(Default)]
struct RunResult {
    applied_route: Option<u8>,
    appended_fixed_string_index: Option<u8>,
    central_writer_value: Option<u8>,
    restored_source_pair: bool,
    a: u8,
}

struct TestCpu {
    memory: Box<[u8; 0x10000]>,
    a: u8,
    p: u8,
    sp: u8,
    pc: u16,
    applied_route: Option<u8>,
    appended_fixed_string_index: Option<u8>,
    central_writer_value: Option<u8>,
    restored_source_pair: bool,
}

impl TestCpu {
    fn new(
        memory: Box<[u8; 0x10000]>,
        routines: &[&RuntimeRoutine],
        entry: u16,
        a: u8,
        p: u8,
    ) -> Self {
        let mut memory = memory;
        for routine in routines {
            let start = usize::from(routine.address);
            let end = start + routine.bytes.len();
            memory[start..end].copy_from_slice(&routine.bytes);
        }
        Self {
            memory,
            a,
            p,
            sp: 0xFD,
            pc: entry,
            applied_route: None,
            appended_fixed_string_index: None,
            central_writer_value: None,
            restored_source_pair: false,
        }
    }

    fn run(mut self) -> (Box<[u8; 0x10000]>, RunResult) {
        for _ in 0..256 {
            let opcode = self.read_pc();
            match opcode {
                0xEA => {}
                0x08 => self.push(self.p),
                0x20 => {
                    let target = self.read_word_pc();
                    if target == APPLY_ROUTE {
                        self.applied_route = Some(self.a);
                    } else if target == CENTRAL_RIGHT_FD_WRITER {
                        self.central_writer_value = Some(self.a);
                    } else {
                        let return_address = self.pc.wrapping_sub(1);
                        self.push((return_address >> 8) as u8);
                        self.push(return_address as u8);
                        self.pc = target;
                    }
                }
                0x28 => {
                    self.p = self.pop();
                }
                0x29 => {
                    self.a &= self.read_pc();
                    self.set_zero(self.a == 0);
                }
                0x38 => self.p |= STATUS_CARRY,
                0x48 => self.push(self.a),
                0x4C => {
                    let target = self.read_word_pc();
                    if target == CENTRAL_RIGHT_FD_WRITER {
                        self.central_writer_value = Some(self.a);
                        return self.finish();
                    }
                    if target == APPLY_ROUTE {
                        self.applied_route = Some(self.a);
                        if self.sp == 0xFD {
                            return self.finish();
                        }
                        let low = self.pop();
                        let high = self.pop();
                        self.pc = u16::from_le_bytes([low, high]).wrapping_add(1);
                        continue;
                    }
                    if target == RESTORE_SOURCE_PAIR {
                        self.restored_source_pair = true;
                        return self.finish();
                    }
                    if target == APPEND_FIXED_STRING {
                        self.appended_fixed_string_index = Some(self.a);
                        return self.finish();
                    }
                    self.pc = target;
                }
                0x60 => {
                    if self.sp == 0xFD {
                        return self.finish();
                    }
                    let low = self.pop();
                    let high = self.pop();
                    self.pc = u16::from_le_bytes([low, high]).wrapping_add(1);
                }
                0x68 => {
                    self.a = self.pop();
                    self.set_zero(self.a == 0);
                }
                0x8D => {
                    let address = self.read_word_pc();
                    self.memory[usize::from(address)] = self.a;
                }
                0x85 => {
                    let address = self.read_pc();
                    self.memory[usize::from(address)] = self.a;
                }
                0xA9 => {
                    self.a = self.read_pc();
                    self.set_zero(self.a == 0);
                }
                0xAD => {
                    let address = self.read_word_pc();
                    self.a = self.memory[usize::from(address)];
                    self.set_zero(self.a == 0);
                }
                0xC9 => {
                    let value = self.read_pc();
                    self.set_zero(self.a == value);
                    self.set_carry(self.a >= value);
                }
                0xE9 => {
                    let value = self.read_pc();
                    let borrow = u8::from(self.p & STATUS_CARRY == 0);
                    let required = u16::from(value) + u16::from(borrow);
                    let result = self.a.wrapping_sub(value).wrapping_sub(borrow);
                    self.set_carry(u16::from(self.a) >= required);
                    self.a = result;
                    self.set_zero(self.a == 0);
                }
                0xF0 => {
                    let displacement = self.read_pc() as i8;
                    if self.p & STATUS_ZERO != 0 {
                        self.pc = self.pc.wrapping_add_signed(i16::from(displacement));
                    }
                }
                0xD0 => {
                    let displacement = self.read_pc() as i8;
                    if self.p & STATUS_ZERO == 0 {
                        self.pc = self.pc.wrapping_add_signed(i16::from(displacement));
                    }
                }
                0x90 => {
                    let displacement = self.read_pc() as i8;
                    if self.p & STATUS_CARRY == 0 {
                        self.pc = self.pc.wrapping_add_signed(i16::from(displacement));
                    }
                }
                other => panic!("test runtime reached unsupported opcode {other:02X}"),
            }
        }
        panic!("test runtime did not terminate");
    }

    fn finish(self) -> (Box<[u8; 0x10000]>, RunResult) {
        (
            self.memory,
            RunResult {
                applied_route: self.applied_route,
                appended_fixed_string_index: self.appended_fixed_string_index,
                central_writer_value: self.central_writer_value,
                restored_source_pair: self.restored_source_pair,
                a: self.a,
            },
        )
    }

    fn read_pc(&mut self) -> u8 {
        let value = self.memory[usize::from(self.pc)];
        self.pc = self.pc.wrapping_add(1);
        value
    }

    fn read_word_pc(&mut self) -> u16 {
        let low = self.read_pc();
        let high = self.read_pc();
        u16::from_le_bytes([low, high])
    }

    fn push(&mut self, value: u8) {
        self.memory[0x100 + usize::from(self.sp)] = value;
        self.sp = self.sp.wrapping_sub(1);
    }

    fn pop(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.memory[0x100 + usize::from(self.sp)]
    }

    fn set_zero(&mut self, set: bool) {
        if set {
            self.p |= STATUS_ZERO;
        } else {
            self.p &= !STATUS_ZERO;
        }
    }

    fn set_carry(&mut self, set: bool) {
        if set {
            self.p |= STATUS_CARRY;
        } else {
            self.p &= !STATUS_CARRY;
        }
    }
}

fn run_routines(
    memory: Box<[u8; 0x10000]>,
    routines: &[&RuntimeRoutine],
    entry: u16,
    a: u8,
    p: u8,
) -> (Box<[u8; 0x10000]>, RunResult) {
    TestCpu::new(memory, routines, entry, a, p).run()
}

#[test]
fn static_redraw_maps_immediately_and_screen_close_clears_the_page() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap()
    .routine;
    let close_origin = publisher.address + u16::try_from(publisher.bytes.len()).unwrap();
    let close = build_consumer_font_page_close(close_origin, RESTORE_SOURCE_PAIR).unwrap();
    let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

    let (memory, first_draw) = run_routines(
        memory,
        &[&activation, &publisher],
        publisher.address,
        ITEM_ACTION_COMPOSITE_STATE,
        0xA5,
    );
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[0]);
    assert_eq!(first_draw.applied_route, Some(pages.catalog[0]));

    // 커서 이동이 같은 합성기를 다시 부르면 페이지를 즉시 다시 고르되, 닫기
    // 경계가 올 때까지 현재 UI 수명 안에 남는다.
    let (memory, redraw) = run_routines(
        memory,
        &[&activation, &publisher],
        publisher.address,
        ITEM_ACTION_COMPOSITE_STATE,
        0x24,
    );
    assert_eq!(redraw.applied_route, Some(pages.catalog[0]));
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[0]);

    let restore_page = 0x19;
    let (memory, closed) = run_routines(memory, &[&close], close.address, restore_page, 0x64);
    assert_eq!(closed.central_writer_value, Some(restore_page));
    assert!(closed.restored_source_pair);
    assert_eq!(closed.applied_route, None);
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
}

#[test]
fn unit_selection_help_selects_the_same_page_as_the_unit_command_family() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap()
    .routine;
    let memory: Box<[u8; 0x10000]> = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

    let (memory, result) = run_routines(
        memory,
        &[&activation, &publisher],
        publisher.address,
        crate::fixed_menu_labels::UNIT_SELECTION_HELP_COMPOSITE_STATE,
        0,
    );

    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.unit_command);
    assert_eq!(result.applied_route, Some(pages.unit_command));
}

#[test]
fn reused_item_list_state_retains_storage_dialogue_only_for_its_source_composer() {
    let pages = pages();
    let route = storage_item_list_route();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher =
        build_composite_font_page_publisher(publisher_origin, activation.address, pages, route)
            .unwrap()
            .routine;

    let mut storage_memory: Box<[u8; 0x10000]> =
        vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
    storage_memory[usize::from(CONSUMER_FONT_PAGE)] = pages.unit_command;
    storage_memory[usize::from(route.caller_state_address)] = route.composition_state;
    let (storage_memory, storage) = run_routines(
        storage_memory,
        &[&activation, &publisher],
        publisher.address,
        route.facility_composite_state,
        0xA5,
    );
    assert_eq!(storage_memory[usize::from(CONSUMER_FONT_PAGE)], 0);
    assert_eq!(storage.central_writer_value, Some(0));
    assert_eq!(storage.applied_route, None);

    for caller_state in [
        route.composition_state.wrapping_sub(1),
        route.composition_state.wrapping_add(1),
    ] {
        let mut ordinary_memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        ordinary_memory[usize::from(route.caller_state_address)] = caller_state;
        let (ordinary_memory, ordinary) = run_routines(
            ordinary_memory,
            &[&activation, &publisher],
            publisher.address,
            route.facility_composite_state,
            0x24,
        );
        assert_eq!(
            ordinary_memory[usize::from(CONSUMER_FONT_PAGE)],
            pages.catalog[0]
        );
        assert_eq!(ordinary.applied_route, Some(pages.catalog[0]));
        assert_eq!(ordinary.central_writer_value, None);
    }
}

#[test]
fn map_funds_and_summary_states_share_the_map_menu_page_until_close() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap()
    .routine;
    let close_origin = publisher.address + u16::try_from(publisher.bytes.len()).unwrap();
    let close = build_consumer_font_page_close(close_origin, RESTORE_SOURCE_PAIR).unwrap();
    let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

    let (memory, funds) = run_routines(
        memory,
        &[&activation, &publisher],
        publisher.address,
        MAP_FUNDS_COMPOSITE_STATE,
        0xA5,
    );
    assert_eq!(funds.applied_route, Some(pages.map_menu));
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.map_menu);

    let (memory, summary) = run_routines(
        memory,
        &[&activation, &publisher],
        publisher.address,
        MAP_SUMMARY_COMPOSITE_STATE,
        0x24,
    );
    assert_eq!(summary.applied_route, Some(pages.map_menu));
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.map_menu);

    // The compact range gives $13/$14 to the map page and $15 to the active
    // shop dialogue. States outside that owned run retain the current screen
    // lifetime until its explicit close boundary.
    for state in [MAP_FUNDS_COMPOSITE_STATE - 1, SHOP_ITEM_COMPOSITE_STATE + 1] {
        let mut adjacent_memory = memory.clone();
        adjacent_memory[usize::from(CONSUMER_FONT_PAGE)] = pages.map_menu;
        let (adjacent_memory, adjacent) = run_routines(
            adjacent_memory,
            &[&activation, &publisher],
            publisher.address,
            state,
            0x24,
        );
        assert_eq!(adjacent.applied_route, None);
        assert_eq!(
            adjacent_memory[usize::from(CONSUMER_FONT_PAGE)],
            pages.map_menu
        );
    }

    let (memory, closed) = run_routines(memory, &[&close], close.address, 0x19, 0x64);
    assert_eq!(closed.central_writer_value, Some(0x19));
    assert!(closed.restored_source_pair);
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
}

#[test]
fn dynamic_name_page_can_change_during_one_open_screen() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

    let (memory, first_name) = run_routines(
        memory,
        &[&activation],
        activation.address,
        pages.catalog[1],
        0x22,
    );
    assert_eq!(first_name.applied_route, Some(pages.catalog[1]));
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[1]);

    let (memory, next_name) = run_routines(
        memory,
        &[&activation],
        activation.address,
        pages.catalog[0],
        0x24,
    );
    assert_eq!(next_name.applied_route, Some(pages.catalog[0]));
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[0]);
}

#[test]
fn unit_summary_leaves_residency_to_its_mandatory_name_appender() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap()
    .routine;
    let mut memory: Box<[u8; 0x10000]> = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
    memory[usize::from(CONSUMER_FONT_PAGE)] = pages.catalog[0];

    let (memory, summary_entry) = run_routines(
        memory,
        &[&activation, &publisher],
        publisher.address,
        UNIT_SUMMARY_COMPOSITE_STATE,
        0x24,
    );
    assert_eq!(summary_entry.applied_route, None);
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[0]);

    let (memory, name_appended) = run_routines(
        memory,
        &[&activation],
        activation.address,
        pages.catalog[1],
        0x24,
    );
    assert_eq!(name_appended.applied_route, Some(pages.catalog[1]));
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[1]);
}

#[test]
fn unit_status_retains_the_page_published_by_unit_summary() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap()
    .routine;

    for retained_route in [0, pages.catalog[0], pages.catalog[1]] {
        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(CONSUMER_FONT_PAGE)] = retained_route;

        let (memory, status_entry) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            UNIT_STATUS_COMPOSITE_STATE,
            0xA5,
        );

        assert_eq!(
            memory[usize::from(COMPOSITE_STATE)],
            UNIT_STATUS_COMPOSITE_STATE
        );
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], retained_route);
        assert_eq!(status_entry.applied_route, None);
    }
}

#[test]
fn dialogue_owned_composites_reenter_the_central_fd_selector_after_clearing_static_residency() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap()
    .routine;

    for state in [
        SHOP_ITEM_COMPOSITE_STATE,
        STORAGE_ACTION_MENU_COMPOSITE_STATE,
        STORAGE_OVERFLOW_ACTION_COMPOSITE_STATE,
    ] {
        for request_state in [0, super::super::transport::STATE_COMPLETED_PAGE_SUSPENDED] {
            let mut memory: Box<[u8; 0x10000]> =
                vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
            memory[usize::from(CONSUMER_FONT_PAGE)] = pages.unit_command;
            memory[usize::from(super::super::transport::REQUEST_STATE)] = request_state;

            let (memory, result) = run_routines(
                memory,
                &[&activation, &publisher],
                publisher.address,
                state,
                0xA5,
            );

            assert_eq!(memory[usize::from(COMPOSITE_STATE)], state);
            assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
            assert_eq!(
                memory[usize::from(super::super::transport::REQUEST_STATE)],
                request_state
            );
            assert_eq!(result.applied_route, None);
            assert_eq!(
                result.central_writer_value,
                Some(0),
                "dialogue-owned state {state:02X} did not re-enter the central right-FD selector"
            );
        }
    }
}

#[test]
fn static_fixed_menu_appender_selects_its_page_without_shadowing_storage_dialogue() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let wrapper_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let wrapper =
        build_fixed_menu_font_page_appender(wrapper_origin, activation.address, pages).unwrap();
    let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

    let (memory, result) = run_routines(
        memory,
        &[&activation, &wrapper],
        wrapper.address,
        0x47,
        0xA5,
    );

    assert_eq!(result.applied_route, Some(pages.unit_command));
    assert_eq!(result.appended_fixed_string_index, Some(0x47));
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.unit_command);

    let hooks = fixed_menu_font_page_hooks(wrapper.address).unwrap();
    let hooked_sites = hooks
        .iter()
        .map(|hook| match hook.site {
            DialogueRuntimeHookSite::Switchable { bank, address } => (bank, address),
            DialogueRuntimeHookSite::Fixed(_) => panic!("fixed-menu hook became fixed"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        hooked_sites,
        [
            (UNIT_UI_BANK, 0x8A3C),
            (UNIT_UI_BANK, 0x8A6D),
            (UNIT_UI_BANK, 0x8A7A),
            (UNIT_UI_BANK, 0x8E31),
        ]
    );
    assert!(!hooked_sites.contains(&(UNIT_UI_BANK, 0x8B1D)));
    assert!(!hooked_sites.contains(&(UNIT_UI_BANK, 0x8DA8)));
    assert!(hooks.iter().all(|hook| hook.bytes[0] == 0x20));
}

#[test]
fn every_front_end_state_selects_its_page_without_prior_residency() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap()
    .routine;

    let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
    let (memory, start_menu) = run_routines(
        memory,
        &[&activation, &publisher],
        publisher.address,
        START_MENU_COMPOSITE_STATE,
        0xA5,
    );
    assert_eq!(
        memory[usize::from(CONSUMER_FONT_PAGE)],
        pages.front_end_menu
    );
    assert_eq!(start_menu.applied_route, Some(pages.front_end_menu));

    for (state, expected_route) in [
        (RECORD_LIST_COMPOSITE_STATE, pages.front_end_menu),
        (SAVE_SLOT_SELECTION_COMPOSITE_STATE, pages.front_end_menu),
        (RECORD_ACTION_COMPOSITE_STATE, pages.front_end_record_action),
    ] {
        let memory: Box<[u8; 0x10000]> = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

        let (memory, result) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            state,
            0xA5,
        );

        assert_eq!(memory[usize::from(COMPOSITE_STATE)], state);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], expected_route);
        assert_eq!(result.applied_route, Some(expected_route));
    }
}

#[test]
fn auxiliary_composite_preserves_the_page_until_the_screen_close_boundary() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap()
    .routine;
    let mut memory: Box<[u8; 0x10000]> = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
    memory[usize::from(CONSUMER_FONT_PAGE)] = pages.catalog[1];

    let (memory, result) = run_routines(
        memory,
        &[&activation, &publisher],
        publisher.address,
        0x0A,
        0x00,
    );

    assert_eq!(memory[usize::from(COMPOSITE_STATE)], 0x0A);
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[1]);
    assert_eq!(result.applied_route, None);
}

#[test]
fn control_only_composites_retain_every_current_page_without_mapper_writes() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap()
    .routine;

    for state in [0x11, 0x17] {
        for retained_route in [0, pages.unit_command, pages.catalog[1]] {
            let mut memory: Box<[u8; 0x10000]> =
                vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
            memory[usize::from(CONSUMER_FONT_PAGE)] = retained_route;
            let (memory, result) = run_routines(
                memory,
                &[&activation, &publisher],
                publisher.address,
                state,
                0,
            );

            assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], retained_route);
            assert_eq!(result.applied_route, None);
            assert_eq!(result.central_writer_value, None);
        }
    }
}

#[test]
fn every_direct_composite_state_follows_its_declared_page_action() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap()
    .routine;
    let retained_route = 0xB4;

    for (state, policy) in COMPOSITE_FONT_RESIDENCY_POLICIES {
        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(CONSUMER_FONT_PAGE)] = retained_route;
        let (memory, result) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            state,
            0,
        );

        let (expected_page, expected_applied_route, expected_central_write) =
            if let Some(page) = policy.static_page() {
                let route = page.mapper_route(pages);
                (route, Some(route), None)
            } else if matches!(
                policy,
                ScreenFontResidencyPolicy::SourcePageSelected
                    | ScreenFontResidencyPolicy::CompletedDialoguePageRetained
                    | ScreenFontResidencyPolicy::ActiveDialogueCallerRestored
            ) {
                (0, None, Some(0))
            } else {
                (retained_route, None, None)
            };

        assert_eq!(
            memory[usize::from(CONSUMER_FONT_PAGE)],
            expected_page,
            "composite state {state:02X} selected the wrong retained page"
        );
        assert_eq!(
            result.applied_route, expected_applied_route,
            "composite state {state:02X} applied the wrong translated route"
        );
        assert_eq!(
            result.central_writer_value, expected_central_write,
            "composite state {state:02X} took the wrong source-page path"
        );
    }
}

#[test]
fn source_only_composites_clear_a_stale_translation_route_and_select_page_zero() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap()
    .routine;

    for state in [0x08, 0x10] {
        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(CONSUMER_FONT_PAGE)] = pages.catalog[1];

        let (memory, result) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            state,
            0,
        );

        assert_eq!(memory[usize::from(COMPOSITE_STATE)], state);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
        assert_eq!(result.applied_route, None);
        assert_eq!(result.central_writer_value, Some(0));
    }
}

#[test]
fn reused_state_20_never_claims_the_ending_font_page() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap()
    .routine;

    let close_origin = publisher.address + u16::try_from(publisher.bytes.len()).unwrap();
    let close = build_consumer_font_page_close(close_origin, RESTORE_SOURCE_PAIR).unwrap();
    let mut memory: Box<[u8; 0x10000]> = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
    memory[usize::from(CONSUMER_FONT_PAGE)] = pages.catalog[0];
    let (memory, closed) = run_routines(memory, &[&close], close.address, 0, 0);
    assert!(closed.restored_source_pair);
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
    let (memory, result) = run_routines(
        memory,
        &[&activation, &publisher],
        publisher.address,
        REUSED_STATE_WITHOUT_FONT_OWNERSHIP,
        0,
    );
    assert_eq!(result.applied_route, None);
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
}

#[test]
fn screen_open_reapplies_and_retains_the_page_until_close() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap();
    let open_origin =
        publisher.routine.address + u16::try_from(publisher.routine.bytes.len()).unwrap();
    let open = build_consumer_font_page_open(
        open_origin,
        activation.address,
        publisher.source_page_selection,
    )
    .unwrap();
    let mut memory: Box<[u8; 0x10000]> = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
    memory[usize::from(CONSUMER_FONT_PAGE)] = pages.unit_command;
    memory[usize::from(RIGHT_FD_SOURCE_SHADOW)] = 0x19;

    let (memory, result) = run_routines(
        memory,
        &[&activation, &publisher.routine, &open],
        open.address,
        0x55,
        0xA4,
    );

    assert_eq!(result.applied_route, Some(pages.unit_command));
    assert_eq!(result.central_writer_value, None);
    assert_eq!(result.a, 0);
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.unit_command);
    assert_eq!(memory[usize::from(RIGHT_FD_SOURCE_SHADOW)], 0);
}

#[test]
fn empty_page_uses_the_source_writer_without_calling_activation() {
    let pages = pages();
    let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
    let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
    let publisher = build_composite_font_page_publisher(
        publisher_origin,
        activation.address,
        pages,
        storage_item_list_route(),
    )
    .unwrap();
    let open_origin =
        publisher.routine.address + u16::try_from(publisher.routine.bytes.len()).unwrap();
    let open = build_consumer_font_page_open(
        open_origin,
        activation.address,
        publisher.source_page_selection,
    )
    .unwrap();
    let memory: Box<[u8; 0x10000]> = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

    let (memory, result) = run_routines(
        memory,
        &[&activation, &publisher.routine, &open],
        open.address,
        0x11,
        0xA4,
    );

    assert_eq!(result.central_writer_value, Some(0));
    assert_eq!(result.applied_route, None);
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
}

#[test]
fn gameplay_handoff_releases_the_front_end_page_before_state_reset() {
    let routine = build_consumer_font_page_gameplay_handoff(ORIGIN).unwrap();
    let mut memory: Box<[u8; 0x10000]> = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
    memory[usize::from(CONSUMER_FONT_PAGE)] = pages().front_end_menu;
    memory[usize::from(GAMEPLAY_PHASE_LOW)] = 0xA5;
    memory[usize::from(GAMEPLAY_PHASE_HIGH)] = 0x5A;

    let (memory, result) = run_routines(memory, &[&routine], routine.address, 0, 0x64);

    assert_eq!(result.a, 0);
    assert_eq!(memory[usize::from(GAMEPLAY_PHASE_LOW)], 0);
    assert_eq!(memory[usize::from(GAMEPLAY_PHASE_HIGH)], 0);
    assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);

    let hook = gameplay_handoff_hook(routine.address).unwrap();
    assert!(matches!(
        hook.site,
        DialogueRuntimeHookSite::Fixed(GAMEPLAY_HANDOFF_HOOK_ADDRESS)
    ));
    assert_eq!(
        hook.bytes,
        [
            0x20,
            routine.address as u8,
            (routine.address >> 8) as u8,
            0xEA,
        ]
    );
}

#[test]
fn page_roles_cannot_share_the_empty_sentinel_or_each_other() {
    let mut invalid = pages();
    invalid.map_menu = 0;
    assert!(
        invalid
            .validate()
            .unwrap_err()
            .to_string()
            .contains("empty sentinel")
    );

    invalid = pages();
    invalid.map_menu = invalid.unit_command;
    assert!(
        invalid
            .validate()
            .unwrap_err()
            .to_string()
            .contains("same translated page")
    );

    invalid = pages();
    invalid.map_menu |= 2;
    assert!(
        invalid
            .validate()
            .unwrap_err()
            .to_string()
            .contains("invalid FD/FE page route")
    );
}
