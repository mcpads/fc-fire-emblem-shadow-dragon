use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::EncodedMainDialogueDisplayStorage,
    rom::{HEADER_SIZE, Rom},
    tracked::TrackedImage,
};

use super::{
    DIALOGUE_BYTE_BANK_SELECT_CALL, PRG_BANK_SIZE, SOURCE_DIALOGUE_BANK, TRANSITION_MIRROR_BANKS,
    TRANSITION_POINTER_RESOLVER_CALLS,
    transition_reader::{
        TRANSITION_BANK_SELECTOR, TRANSITION_POINTER_RESOLVER, TransitionReaderRoutines,
    },
};

#[derive(Serialize)]
pub(super) struct CompleteDialogueWriteSetPlan {
    direct_region_write_count: usize,
    canonical_pointer_write_count: usize,
    transition_mirror_bank_write_count: usize,
    transition_mode_hook_write_count: usize,
    dialogue_reader_hook_write_count: usize,
    fixed_routine_write_count: usize,
    pub(super) expected_write_count: usize,
    changed_byte_count: usize,
    every_change_tracked: bool,
    output_materialized_in_memory_only: bool,
    rom_emitted: bool,
}

pub(super) fn validate_complete_dialogue_write_set(
    candidate: &Rom,
    storage: &EncodedMainDialogueDisplayStorage,
    routines: &TransitionReaderRoutines,
) -> Result<CompleteDialogueWriteSetPlan> {
    let mut image = TrackedImage::new(candidate.data().to_vec());
    append_complete_dialogue_writes(&mut image, candidate, storage, routines)?;
    let expected_write_count = image.writes().len();
    image.verify_all_changes_tracked(candidate.data())?;
    let output = image.into_data();
    let changed_byte_count = candidate
        .data()
        .iter()
        .zip(&output)
        .filter(|(before, after)| before != after)
        .count();
    ensure!(
        expected_write_count
            == storage.direct_regions.len()
                + storage.pointer_writes.len()
                + storage.transition_mirrors.len()
                + TRANSITION_POINTER_RESOLVER_CALLS.len()
                + 1
                + 2,
        "complete dialogue Expected Write population changed"
    );
    ensure!(
        changed_byte_count > storage.transition_payload_byte_count,
        "complete dialogue write set changed no bytes beyond transition payloads"
    );
    Ok(CompleteDialogueWriteSetPlan {
        direct_region_write_count: storage.direct_regions.len(),
        canonical_pointer_write_count: storage.pointer_writes.len(),
        transition_mirror_bank_write_count: storage.transition_mirrors.len(),
        transition_mode_hook_write_count: TRANSITION_POINTER_RESOLVER_CALLS.len(),
        dialogue_reader_hook_write_count: 1,
        fixed_routine_write_count: 2,
        expected_write_count,
        changed_byte_count,
        every_change_tracked: true,
        output_materialized_in_memory_only: true,
        rom_emitted: false,
    })
}

pub(super) fn append_complete_dialogue_writes(
    image: &mut TrackedImage,
    candidate: &Rom,
    storage: &EncodedMainDialogueDisplayStorage,
    routines: &TransitionReaderRoutines,
) -> Result<()> {
    for (index, region) in storage.direct_regions.iter().enumerate() {
        write_current_candidate_bytes(
            image,
            candidate,
            format!("complete dialogue direct region {index}"),
            region.file_offset,
            &region.encoded_storage,
        )?;
    }
    for pointer in &storage.pointer_writes {
        write_current_candidate_bytes(
            image,
            candidate,
            format!("complete dialogue pointer {}", pointer.record_id),
            pointer.file_offset,
            &pointer.planned_pointer.to_le_bytes(),
        )?;
    }
    ensure!(
        storage.transition_mirrors.len() == TRANSITION_MIRROR_BANKS.len(),
        "complete dialogue write set lost transition mirror banks"
    );
    for (mirror, physical_bank) in storage
        .transition_mirrors
        .iter()
        .zip(TRANSITION_MIRROR_BANKS)
    {
        let offset = HEADER_SIZE
            + usize::from(physical_bank)
                .checked_mul(PRG_BANK_SIZE)
                .context("transition mirror write offset overflow")?;
        write_current_candidate_bytes(
            image,
            candidate,
            format!(
                "dialogue transition mirror {:02X} to {physical_bank:02X}",
                mirror.source_prg_bank
            ),
            offset,
            &mirror.material,
        )?;
    }
    let transition_resolver_call = [
        0x20,
        TRANSITION_POINTER_RESOLVER as u8,
        (TRANSITION_POINTER_RESOLVER >> 8) as u8,
    ];
    for address in TRANSITION_POINTER_RESOLVER_CALLS {
        write_current_candidate_bytes(
            image,
            candidate,
            format!("dialogue transition mode hook {address:04X}"),
            switchable_cpu_file_offset(SOURCE_DIALOGUE_BANK, address)?,
            &transition_resolver_call,
        )?;
    }
    let transition_selector_call = [
        0x20,
        TRANSITION_BANK_SELECTOR as u8,
        (TRANSITION_BANK_SELECTOR >> 8) as u8,
    ];
    write_current_candidate_bytes(
        image,
        candidate,
        "dialogue transition reader hook",
        fixed_cpu_file_offset(candidate, DIALOGUE_BYTE_BANK_SELECT_CALL)?,
        &transition_selector_call,
    )?;
    write_current_candidate_bytes(
        image,
        candidate,
        "dialogue transition pointer resolver",
        fixed_cpu_file_offset(candidate, TRANSITION_POINTER_RESOLVER)?,
        &routines.pointer_resolver,
    )?;
    write_current_candidate_bytes(
        image,
        candidate,
        "dialogue transition bank selector",
        fixed_cpu_file_offset(candidate, TRANSITION_BANK_SELECTOR)?,
        &routines.bank_selector,
    )?;

    Ok(())
}

fn write_current_candidate_bytes(
    image: &mut TrackedImage,
    candidate: &Rom,
    label: impl Into<String>,
    offset: usize,
    replacement: &[u8],
) -> Result<()> {
    let end = offset
        .checked_add(replacement.len())
        .context("complete dialogue Expected Write range overflow")?;
    let expected = candidate
        .data()
        .get(offset..end)
        .context("complete dialogue Expected Write is outside the current candidate")?;
    image.write_expected(label, offset, expected, replacement)
}

fn switchable_cpu_file_offset(bank: u8, address: u16) -> Result<usize> {
    ensure!(
        (0x8000..0xC000).contains(&address),
        "complete dialogue switchable hook is outside $8000-$BFFF"
    );
    Ok(HEADER_SIZE + usize::from(bank) * PRG_BANK_SIZE + usize::from(address - 0x8000))
}

fn fixed_cpu_file_offset(candidate: &Rom, address: u16) -> Result<usize> {
    ensure!(
        address >= 0xC000,
        "complete dialogue fixed hook is outside $C000-$FFFF"
    );
    candidate
        .prg()
        .len()
        .checked_sub(PRG_BANK_SIZE)
        .and_then(|offset| offset.checked_add(HEADER_SIZE))
        .and_then(|offset| offset.checked_add(usize::from(address - 0xC000)))
        .context("complete dialogue fixed hook offset overflow")
}
