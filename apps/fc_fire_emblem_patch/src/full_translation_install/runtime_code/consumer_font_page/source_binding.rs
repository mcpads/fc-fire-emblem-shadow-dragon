use super::*;
use crate::dialogue_inventory::bind_main_dialogue_composite_appenders;

/// 합성 진입과 화면 열기·닫기의 중앙 FD 공급을 원본과 후보 양쪽에 묶는다.
/// bank 0B에서 `$C9BE`를 직접 부르는 곳이 이 두 자리뿐이어야, 열기만 현재 UI
/// 페이지를 적용하고 닫기는 원본 selector 사슬로 복귀한다는 분모가 닫힌다.
pub(in crate::full_translation_install::runtime_code) fn bind_consumer_font_page_lifetime(
    source: &Rom,
    candidate: &Rom,
) -> Result<()> {
    bind_unit_summary_status_page_inheritance_source(source.data())?;
    bind_main_dialogue_composite_page_ownership(source)?;
    bind_direct_composite_state_producers(source, candidate)?;
    bind_fixed_menu_font_page_appender_cave(source, candidate)?;
    let fixed_strings = inspect_fixed_string_consumers(source)?;
    for (image_role, rom) in [("source", source), ("candidate", candidate)] {
        for (call, index, delegated, role) in FIXED_MENU_FONT_PAGE_CALLS {
            let producer = call
                .checked_sub(2)
                .context("fixed-menu appender producer address underflow")?;
            let expected = [0xA9, index, 0x20, 0xEE, 0x8E];
            let actual = switchable_slice(rom, UNIT_UI_BANK, producer, expected.len())?;
            ensure!(
                actual == expected,
                "{image_role} {role} changed at {UNIT_UI_BANK:02X}:{producer:04X}"
            );
            decode_rp2a03_sequence(
                actual,
                producer,
                "load and append the first fixed-menu label on one execution path",
            )?;
            if image_role == "source"
                && let Some((_, owner)) = delegated
            {
                let call_site = fixed_strings
                    .call_sites
                    .iter()
                    .find(|candidate| candidate.cpu_address == call)
                    .with_context(|| {
                        format!("{role} disappeared from the fixed-string call census")
                    })?;
                ensure!(
                    call_site.possible_indices == [index]
                        && composite_font_residency_policy(call_site.composite_state)
                            == Some(ScreenFontResidencyPolicy::Delegated(owner)),
                    "{role} no longer delegates composite state {:02X} to {}",
                    call_site.composite_state,
                    owner.id()
                );
            }
        }

        let entry = fixed_slice(rom, COMPOSITE_PAGE_ENTRY, COMPOSITE_PAGE_ENTRY_SOURCE.len())?;
        ensure!(
            entry == COMPOSITE_PAGE_ENTRY_SOURCE,
            "{image_role} composite page entry changed at {COMPOSITE_PAGE_ENTRY:04X}"
        );
        decode_rp2a03_sequence(
            entry,
            COMPOSITE_PAGE_ENTRY,
            "publish and dispatch one composite request",
        )?;

        for (address, expected, role) in [
            (
                SCREEN_OPEN_SEQUENCE_ADDRESS,
                SCREEN_OPEN_SEQUENCE.as_slice(),
                "open one composite screen and supply its central right-FD page",
            ),
            (
                SCREEN_CLOSE_SEQUENCE_ADDRESS,
                SCREEN_CLOSE_SEQUENCE.as_slice(),
                "close one composite screen and restore its central right-FD page",
            ),
        ] {
            let actual = switchable_slice(rom, UNIT_UI_BANK, address, expected.len())?;
            ensure!(
                actual == expected,
                "{image_role} {role} changed at {UNIT_UI_BANK:02X}:{address:04X}"
            );
            decode_rp2a03_sequence(actual, address, role)?;
        }

        let gameplay_handoff = fixed_slice(
            rom,
            GAMEPLAY_HANDOFF_SEQUENCE_ADDRESS,
            GAMEPLAY_HANDOFF_SEQUENCE.len(),
        )?;
        ensure!(
            gameplay_handoff == GAMEPLAY_HANDOFF_SEQUENCE,
            "{image_role} gameplay initialization handoff changed at {GAMEPLAY_HANDOFF_SEQUENCE_ADDRESS:04X}"
        );
        decode_rp2a03_sequence(
            gameplay_handoff,
            GAMEPLAY_HANDOFF_SEQUENCE_ADDRESS,
            "clear the front-end font lifetime while initializing gameplay state",
        )?;

        let transfers = direct_transfer_sites_in_bank(rom, UNIT_UI_BANK, CENTRAL_RIGHT_FD_WRITER)?;
        ensure!(
            transfers
                == [
                    (SCREEN_OPEN_RIGHT_FD_CALL, JSR_ABSOLUTE_OPCODE),
                    (SCREEN_CLOSE_RIGHT_FD_CALL, JSR_ABSOLUTE_OPCODE),
                ],
            "{image_role} bank {UNIT_UI_BANK:02X} central right-FD direct-transfer census changed: {transfers:?}"
        );
    }
    Ok(())
}

fn bind_main_dialogue_composite_page_ownership(source: &Rom) -> Result<()> {
    let routes = bind_main_dialogue_composite_appenders(source)?;
    ensure!(
        !routes.is_empty()
            && routes.iter().all(|route| {
                composite_font_residency_policy(route.composite_state)
                    == Some(ScreenFontResidencyPolicy::Delegated(
                        DelegatedFontPageOwner::MainDialogueRuntimeSelector,
                    ))
            }),
        "main-dialogue auxiliary composites are not delegated to the dialogue runtime selector"
    );
    Ok(())
}

/// Bank 0B is already selected while its composite handlers append fixed labels.  Keeping this
/// ten-byte wrapper in that bank avoids spending scarce fixed-bank selector space.  The selected
/// range is exact FF in both inputs and no adjacent little-endian source word names any address in
/// it; a future table, call, or data owner therefore fails this admission instead of being covered.
fn bind_fixed_menu_font_page_appender_cave(source: &Rom, candidate: &Rom) -> Result<()> {
    let length =
        usize::from(FIXED_MENU_FONT_PAGE_APPENDER_END - FIXED_MENU_FONT_PAGE_APPENDER_ORIGIN);
    for (image_role, rom) in [("source", source), ("candidate", candidate)] {
        let bytes = switchable_slice(
            rom,
            UNIT_UI_BANK,
            FIXED_MENU_FONT_PAGE_APPENDER_ORIGIN,
            length,
        )?;
        ensure!(
            bytes.iter().all(|byte| *byte == 0xFF),
            "{image_role} fixed-menu font-page appender cave is not exact FF"
        );
    }

    let bank_start = usize::from(UNIT_UI_BANK) * FIXED_BANK_BYTE_COUNT;
    let bank = source
        .prg()
        .get(bank_start..bank_start + FIXED_BANK_BYTE_COUNT)
        .context("source bank 0B is missing")?;
    let literal_references = bank
        .windows(2)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            let target = u16::from_le_bytes([bytes[0], bytes[1]]);
            (FIXED_MENU_FONT_PAGE_APPENDER_ORIGIN..FIXED_MENU_FONT_PAGE_APPENDER_END)
                .contains(&target)
                .then_some((0x8000_u16 + u16::try_from(offset).ok()?, target))
        })
        .collect::<Vec<_>>();
    ensure!(
        literal_references.is_empty(),
        "source bank 0B gained a literal reference into the fixed-menu appender cave: {literal_references:?}"
    );
    Ok(())
}

/// `$E690`의 모든 직접 호출은 바로 앞 `LDA #state`와 한 명령 단위로 결속한다.
/// 페이지를 게시하는 상태의 생산자 집합도 별도로 고정해, 재사용 상태가 늘거나 새
/// 호출자가 생겼을 때 화면 역할을 추측한 채 통과하지 못하게 한다.
fn bind_direct_composite_state_producers(source: &Rom, candidate: &Rom) -> Result<()> {
    let source_catalog = bind_direct_composite_state_producer_catalog(source)?;
    ensure!(
        scan_direct_composite_state_producers(candidate)? == source_catalog,
        "candidate direct composite-state producer catalog changed"
    );

    let policy_states = COMPOSITE_FONT_RESIDENCY_POLICIES
        .iter()
        .map(|(state, _)| *state)
        .collect::<BTreeSet<_>>();
    let source_states = source_catalog
        .iter()
        .map(|producer| producer.state)
        .collect::<BTreeSet<_>>();
    ensure!(
        source_states == policy_states,
        "screen font residency and direct composite-state producers disagree: source {source_states:?}, policies {policy_states:?}"
    );
    Ok(())
}

fn fixed_slice(rom: &Rom, address: u16, len: usize) -> Result<&[u8]> {
    ensure!(address >= 0xC000, "fixed source address is below $C000");
    let start = rom
        .prg()
        .len()
        .checked_sub(FIXED_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - 0xC000)))
        .context("fixed source offset overflow")?;
    rom.prg()
        .get(start..start + len)
        .context("fixed source range is outside PRG")
}

fn switchable_slice(rom: &Rom, bank: u8, address: u16, len: usize) -> Result<&[u8]> {
    ensure!(
        (0x8000..0xC000).contains(&address),
        "switchable source address is outside $8000..$BFFF"
    );
    let start = usize::from(bank) * FIXED_BANK_BYTE_COUNT + usize::from(address - 0x8000);
    rom.prg()
        .get(start..start + len)
        .context("switchable source range is outside PRG")
}

fn direct_transfer_sites_in_bank(rom: &Rom, bank: u8, target: u16) -> Result<Vec<(u16, u8)>> {
    let bytes = switchable_slice(rom, bank, 0x8000, FIXED_BANK_BYTE_COUNT)?;
    let operand = target.to_le_bytes();
    let mut transfers = bytes
        .windows(3)
        .enumerate()
        .filter_map(|(offset, window)| {
            ([JSR_ABSOLUTE_OPCODE, JMP_ABSOLUTE_OPCODE].contains(&window[0])
                && window[1..] == operand)
                .then_some((
                    0x8000 + u16::try_from(offset).expect("16 KiB offset fits u16"),
                    window[0],
                ))
        })
        .collect::<Vec<_>>();
    transfers.sort_unstable();
    Ok(transfers)
}
