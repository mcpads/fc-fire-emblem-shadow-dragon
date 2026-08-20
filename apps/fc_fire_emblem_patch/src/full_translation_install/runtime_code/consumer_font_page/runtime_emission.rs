use super::*;

/// 원본 `STA $05E8`를 같은 길이의 호출로 바꾼다. 호출 뒤의 `LDA #$01`이 A와
/// 플래그를 즉시 새로 정하므로 게시기는 X/Y만 건드리지 않으면 된다.
pub(in crate::full_translation_install::runtime_code) fn page_publisher_hook(
    publisher: u16,
) -> Result<DialogueRuntimeHook> {
    Ok(DialogueRuntimeHook {
        role: DialogueRuntimeHookRole::ConsumerFontPagePublisher,
        write_role: "consumer font page publisher hook",
        site: DialogueRuntimeHookSite::Fixed(COMPOSITE_PAGE_ENTRY),
        bytes: assemble_at(COMPOSITE_PAGE_ENTRY, &[Instruction::JsrAbsolute(publisher)])?,
    })
}

/// 복합 UI의 열기·닫기 두 호출만 각 수명 routine으로 보낸다. 둘 다 원본과 같은
/// 3바이트 `JSR`이어서 주변 명령 경계와 반환 주소를 바꾸지 않는다.
pub(in crate::full_translation_install::runtime_code) fn screen_lifetime_hooks(
    open: u16,
    close: u16,
) -> Result<[DialogueRuntimeHook; 2]> {
    Ok([
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::ConsumerFontPageOpen,
            write_role: "consumer font page screen-open hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: UNIT_UI_BANK,
                address: SCREEN_OPEN_RIGHT_FD_CALL,
            },
            bytes: assemble_at(SCREEN_OPEN_RIGHT_FD_CALL, &[Instruction::JsrAbsolute(open)])?,
        },
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::ConsumerFontPageClose,
            write_role: "consumer font page screen-close hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: UNIT_UI_BANK,
                address: SCREEN_CLOSE_RIGHT_FD_CALL,
            },
            bytes: assemble_at(
                SCREEN_CLOSE_RIGHT_FD_CALL,
                &[Instruction::JsrAbsolute(close)],
            )?,
        },
    ])
}

/// 저장 기록을 불러온 뒤 맵 초기화가 시작되는 원본의 두 제로 페이지 저장을 같은
/// 의미의 고정 루틴으로 보낸다. 네 바이트 자리를 그대로 소유하므로 뒤의 `$84` 상태
/// 초기화 경계는 움직이지 않는다.
pub(in crate::full_translation_install::runtime_code) fn gameplay_handoff_hook(
    handoff: u16,
) -> Result<DialogueRuntimeHook> {
    Ok(DialogueRuntimeHook {
        role: DialogueRuntimeHookRole::ConsumerFontPageGameplayHandoff,
        write_role: "front-end font page gameplay-handoff hook",
        site: DialogueRuntimeHookSite::Fixed(GAMEPLAY_HANDOFF_HOOK_ADDRESS),
        bytes: assemble_at(
            GAMEPLAY_HANDOFF_HOOK_ADDRESS,
            &[Instruction::JsrAbsolute(handoff), Instruction::Nop],
        )?,
    })
}

/// Standalone fixed-menu appender calls have the same three-byte footprint as their source
/// `JSR $8EEE`. Calls that draw over main dialogue are deliberately left on the source appender so
/// they cannot replace the active dialogue route with the unit-command page. The wrapper restores
/// the label index before tail-calling the source appender.
pub(in crate::full_translation_install::runtime_code) fn fixed_menu_font_page_hooks(
    wrapper: u16,
) -> Result<Vec<DialogueRuntimeHook>> {
    FIXED_MENU_FONT_PAGE_CALLS
        .into_iter()
        .filter_map(|(address, _, delegated, write_role)| {
            delegated.map(|(role, _)| {
                Ok(DialogueRuntimeHook {
                    role,
                    write_role,
                    site: DialogueRuntimeHookSite::Switchable {
                        bank: UNIT_UI_BANK,
                        address,
                    },
                    bytes: assemble_at(address, &[Instruction::JsrAbsolute(wrapper)])?,
                })
            })
        })
        .collect()
}

/// Carries the source-bound bank-0B cave routine through the same checked switchable-bank mutation
/// path as its call-site hooks.  Its distinct role prevents it from being mistaken for one of the
/// six replaced calls.
pub(in crate::full_translation_install::runtime_code) fn fixed_menu_font_page_appender_installation(
    routine: &RuntimeRoutine,
) -> Result<DialogueRuntimeHook> {
    ensure!(
        routine.address == FIXED_MENU_FONT_PAGE_APPENDER_ORIGIN
            && routine.bytes.len()
                == usize::from(
                    FIXED_MENU_FONT_PAGE_APPENDER_END - FIXED_MENU_FONT_PAGE_APPENDER_ORIGIN,
                ),
        "fixed-menu font-page appender no longer fills its admitted bank-0B cave"
    );
    Ok(DialogueRuntimeHook {
        role: DialogueRuntimeHookRole::FixedMenuFontPageAppenderRoutine,
        write_role: "fixed-menu font-page appender routine",
        site: DialogueRuntimeHookSite::Switchable {
            bank: UNIT_UI_BANK,
            address: routine.address,
        },
        bytes: routine.bytes.clone(),
    })
}

/// 계획 단계에서 검증한 FD/FE route를 현재 UI 페이지로 게시하고 즉시 적용한다.
/// 합성기는 `ScreenFontPageRoutes`의 상수만 넘기고, 이름 appender는 경계가 검증된
/// catalog record의 첫 바이트만 넘긴다. `$07FD=0`인 화면 열기는 이 routine을 부르기
/// 전에 원본 writer로 빠지므로, 이 작은 routine에는 실행 중 allowlist를 다시 복제하지
/// 않는다.
pub(in crate::full_translation_install::runtime_code) fn build_consumer_font_page_activation(
    origin: u16,
    apply_route: u16,
    pages: ScreenFontPageRoutes,
) -> Result<RuntimeRoutine> {
    pages.validate()?;
    let instructions = [
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::JmpAbsolute(apply_route),
    ];

    Ok(RuntimeRoutine {
        role: "consumer font page activation",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// Selects the fixed-menu page without losing the source appender's index in A.  A `JMP` into the
/// original appender makes its `RTS` return directly to the original screen composer, exactly as
/// the replaced `JSR $8EEE` did.
pub(in crate::full_translation_install::runtime_code) fn build_fixed_menu_font_page_appender(
    origin: u16,
    activation: u16,
    pages: ScreenFontPageRoutes,
) -> Result<RuntimeRoutine> {
    pages.validate()?;
    let instructions = [
        Instruction::Pha,
        Instruction::LdaImmediate(pages.unit_command),
        Instruction::JsrAbsolute(activation),
        Instruction::Pla,
        Instruction::JmpAbsolute(APPEND_FIXED_STRING),
    ];
    Ok(RuntimeRoutine {
        role: "fixed-menu font-page appender",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// The composite publisher contains one shared source-page entry.  Both the
/// state dispatcher and the screen-open fallback enter this exact sequence,
/// so clearing a stale translated route cannot drift into two implementations.
pub(in crate::full_translation_install::runtime_code) struct CompositeFontPagePublisher {
    pub(in crate::full_translation_install::runtime_code) routine: RuntimeRoutine,
    pub(in crate::full_translation_install::runtime_code) source_page_selection: u16,
}

/// 이번 합성이 바꾸는 페이지 수명을 게시한다. 요약은 route를 직접 바꾸지 않고
/// source-bound unit/enemy appender가 첫 글자를 쓰기 전에 실제 페이지를 게시하게
/// 하며, 상태는 요약이 게시한 route를 그대로 유지한다. 저장소 overlay는 완료 대사
/// selector로 명시적으로 넘긴다.
///
/// 그 밖의 합성 상태는 **현재 화면의 보조 합성**일 수 있으므로 게시값을 바꾸지 않는다.
/// 실제 화면 이탈은 별도 close/gameplay-handoff 경계가 소유한다. 합성기 번호 하나를
/// 화면 수명으로 오해해 여기서 0을 쓰면, unit-summary가 이름 페이지를 게시한 직후
/// 상태 `$0A` 보조 합성이 그 페이지를 지워 원문 CHR로 되돌아간다.
pub(in crate::full_translation_install::runtime_code) fn build_composite_font_page_publisher(
    origin: u16,
    activation: u16,
    pages: ScreenFontPageRoutes,
    storage_item_list: StorageItemListRuntimeRoute,
) -> Result<CompositeFontPagePublisher> {
    pages.validate()?;
    ensure!(
        storage_item_list.composite_state == UNIT_ITEM_LIST_COMPOSITE_STATE,
        "storage item-list route no longer refines composite state 0x{UNIT_ITEM_LIST_COMPOSITE_STATE:02X}"
    );
    let mut instructions = vec![Instruction::StaAbsolute(COMPOSITE_STATE)];
    instructions.push(Instruction::CmpImmediate(storage_item_list.composite_state));
    let non_item_list_state = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    instructions.extend([
        Instruction::LdaAbsolute(storage_item_list.caller_state_address),
        Instruction::CmpImmediate(storage_item_list.composition_state),
    ]);
    let catalog_for_ordinary_item_list_jump = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    let source_page_selection = next_address(origin, &instructions)?;
    instructions.extend([
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::JmpAbsolute(CENTRAL_RIGHT_FD_WRITER),
    ]);
    let ordinary_state_target = next_address(origin, &instructions)?;
    instructions[non_item_list_state] = Instruction::BneAbsolute(ordinary_state_target);
    let mut route_jumps = Vec::with_capacity(COMPOSITE_FONT_RESIDENCY_POLICIES.len());
    let mut source_page_states = Vec::new();
    for (state, policy) in COMPOSITE_FONT_RESIDENCY_POLICIES {
        if state == storage_item_list.composite_state {
            continue;
        }
        if policy == ScreenFontResidencyPolicy::SourcePageSelected {
            source_page_states.push(state);
            continue;
        }
        let Some(page) = policy.static_page() else {
            continue;
        };
        if [MAP_FUNDS_COMPOSITE_STATE, MAP_SUMMARY_COMPOSITE_STATE].contains(&state) {
            continue;
        }
        instructions.push(Instruction::CmpImmediate(state));
        let jump = instructions.len();
        instructions.push(Instruction::BeqAbsolute(origin));
        route_jumps.push((jump, page));
    }
    // Upstream-published item/name states and auxiliary composers share the default retain path.
    // Their source-bound appenders publish the exact catalog page only when they emit the dynamic
    // name; unit status then inherits the unit-summary page. Clearing here would be both redundant
    // and a transient source-page selection.
    instructions.push(Instruction::CmpImmediate(
        STORAGE_ACTION_MENU_COMPOSITE_STATE,
    ));
    let clear_for_storage_action_jump = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.push(Instruction::CmpImmediate(
        STORAGE_OVERFLOW_ACTION_COMPOSITE_STATE,
    ));
    let clear_for_storage_overflow_jump = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    // States $13 and $14 are adjacent and share the map-menu page.  The following
    // state $15 is the source-bound shop item composer: it must discard the prior
    // unit-command route so the active E7 caller can restore its dialogue page.
    // One subtraction therefore owns all three states without another full CMP route.
    instructions.extend([
        Instruction::Sec,
        Instruction::SbcImmediate(MAP_FUNDS_COMPOSITE_STATE),
        Instruction::CmpImmediate(2),
    ]);
    let map_summary_range_jump = instructions.len();
    instructions.push(Instruction::BccAbsolute(origin));
    let shop_dialogue_restore_jump = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    ensure!(
        source_page_states == [0x08, 0x10],
        "source-page composite state family changed"
    );
    // A now holds `state - $13`. Source-only states $08/$10 become $F5/$FD;
    // clearing bit 3 maps exactly those two values to $F5 across the complete
    // direct-state domain. This replaces two independent CMP/BEQ pairs and
    // keeps the publisher inside its already-owned cave.
    instructions.extend([
        Instruction::AndImmediate(0xF7),
        Instruction::CmpImmediate(0xF5),
    ]);
    let source_page_pair_jump = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.push(Instruction::Rts);
    instructions[clear_for_storage_action_jump] = Instruction::BeqAbsolute(source_page_selection);
    instructions[clear_for_storage_overflow_jump] = Instruction::BeqAbsolute(source_page_selection);
    instructions[shop_dialogue_restore_jump] = Instruction::BeqAbsolute(source_page_selection);
    instructions[source_page_pair_jump] = Instruction::BeqAbsolute(source_page_selection);
    for page in [
        ScreenFontPageRole::FrontEndMenu,
        ScreenFontPageRole::FrontEndRecordAction,
        ScreenFontPageRole::CatalogDefault,
        ScreenFontPageRole::UnitCommand,
        ScreenFontPageRole::MapMenu,
        ScreenFontPageRole::ChapterSaveOffer,
    ] {
        let target = next_address(origin, &instructions)?;
        for (jump, page_role) in &route_jumps {
            if *page_role == page {
                instructions[*jump] = Instruction::BeqAbsolute(target);
            }
        }
        if page == ScreenFontPageRole::MapMenu {
            instructions[map_summary_range_jump] = Instruction::BccAbsolute(target);
        }
        if page == ScreenFontPageRole::CatalogDefault {
            instructions[catalog_for_ordinary_item_list_jump] = Instruction::BneAbsolute(target);
        }
        instructions.extend([
            Instruction::LdaImmediate(page.mapper_route(pages)),
            // Every validated mapper route is nonzero. The taken relative branch is
            // therefore an exact unconditional transfer while keeping the publisher
            // inside its source-bound fixed-bank cave.
            Instruction::BneAbsolute(activation),
        ]);
    }

    Ok(CompositeFontPagePublisher {
        routine: RuntimeRoutine {
            role: "composite consumer font page publisher",
            address: origin,
            bytes: assemble_at(origin, &instructions)?,
        },
        source_page_selection,
    })
}

/// bank 0B의 복합 UI 열기에서 현재 route를 다시 적용한다. 게시값은 닫기 경계까지
/// 유지하며, 유효한 route가 없으면 원본 `LDA #0; JSR $C9BE` 의미로 돌아간다.
pub(in crate::full_translation_install::runtime_code) fn build_consumer_font_page_open(
    origin: u16,
    activation: u16,
    source_page_selection: u16,
) -> Result<RuntimeRoutine> {
    let mut instructions = vec![Instruction::LdaAbsolute(CONSUMER_FONT_PAGE)];
    let no_page = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.extend([
        Instruction::JsrAbsolute(activation),
        Instruction::LdaImmediate(0),
        Instruction::StaZeroPage(RIGHT_FD_SOURCE_SHADOW),
        Instruction::Rts,
    ]);
    instructions[no_page] = Instruction::BeqAbsolute(source_page_selection);

    Ok(RuntimeRoutine {
        role: "consumer font page screen open",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 프런트엔드에서 저장 기록을 불러와 맵 초기화로 넘어가는 경계다. 원본의 `$23`과
/// `$24` 초기화를 보존하면서, 첫 맵 프레임 전에 프런트엔드 글꼴 소유권을 반납한다.
pub(in crate::full_translation_install::runtime_code) fn build_consumer_font_page_gameplay_handoff(
    origin: u16,
) -> Result<RuntimeRoutine> {
    let instructions = [
        Instruction::StaZeroPage(GAMEPLAY_PHASE_LOW),
        Instruction::StaZeroPage(GAMEPLAY_PHASE_HIGH),
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::Rts,
    ];
    Ok(RuntimeRoutine {
        role: "front-end font page gameplay handoff",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 복합 UI 닫기는 route를 먼저 지운 뒤 원본 A를 그대로 중앙 FD 기록기에 넘긴다.
/// 이후 전역 selector는 `$07FD=0`을 보고 대사 또는 기존 그래픽 사슬로 복귀하므로
/// 지형·전투·제목이 마지막 UI 페이지를 상속하지 않는다.
pub(in crate::full_translation_install::runtime_code) fn build_consumer_font_page_close(
    origin: u16,
    restore_source_pair: u16,
) -> Result<RuntimeRoutine> {
    let instructions = [
        Instruction::Pha,
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::Pla,
        Instruction::JsrAbsolute(CENTRAL_RIGHT_FD_WRITER),
        Instruction::JmpAbsolute(restore_source_pair),
    ];
    Ok(RuntimeRoutine {
        role: "consumer font page screen close",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}
