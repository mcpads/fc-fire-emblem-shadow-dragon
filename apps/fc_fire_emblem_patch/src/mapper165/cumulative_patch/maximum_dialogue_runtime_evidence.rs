use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::Value;

use crate::{rom::EXPECTED_SOURCE_SHA1, sha1_hex};

use super::super::maximum_dialogue_page::{COMPLETED_PAGE_COUNT, SCREEN_ROLE, TARGET_RECORD_ID};

const EXPECTED_FRAME_OFFSETS: [u64; 6] = [0, 7, 19, 43, 82, 171];
const EXPECTED_SAMPLE_ARTIFACT_ROLES: [&str; 4] =
    ["state.json", "prgram.bin", "nametable.bin", "screen.png"];
const EXPECTED_TEMPORAL_VISUAL_ARTIFACT_ROLES: [&str; 6] = [
    "temporal-contact-sheets/frame-0000.png",
    "temporal-contact-sheets/frame-0007.png",
    "temporal-contact-sheets/frame-0019.png",
    "temporal-contact-sheets/frame-0043.png",
    "temporal-contact-sheets/frame-0082.png",
    "temporal-contact-sheets/frame-0171.png",
];
const EXPECTED_EXIT_ARTIFACT_ROLES: [&str; 6] = [
    "final-exit-immediate-events.json",
    "next-story/state.json",
    "next-story/prgram.bin",
    "next-story/iram.bin",
    "next-story/nametable.bin",
    "next-story.png",
];
const PRG_RAM_SIZE: usize = 0x2000;
const INTERNAL_RAM_SIZE: usize = 0x0800;
const NAMETABLE_SIZE: usize = 0x0800;
const PRG_RAM_CPU_BASE: usize = 0x6000;
const DIALOGUE_STATE_ADDRESS: usize = 0x77F7;
const COMPLETED_LINE_COUNT_ADDRESS: usize = 0x77F8;
const CURRENT_POINTER_LOW_ADDRESS: usize = 0x7812;
const CURRENT_POINTER_HIGH_ADDRESS: usize = 0x7814;
const OUTER_STATE_ADDRESS: usize = 0x24;
const MAIN_STATE_ADDRESS: usize = 0x84;
const INITIAL_SELECTOR_ADDRESS: u64 = 0xF990;
const INITIAL_SELECTOR_WRITE_ADDRESS: u64 = 0x8001;
const INITIAL_SELECTOR_WRITE_PC: u64 = 0xF375;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1A\n";

pub(super) struct MaximumDialogueRuntimeEvidence {
    pub(super) manifest_sha1: String,
    pub(super) sample_count: usize,
    pub(super) page_count: usize,
    pub(super) unique_nametable_count: usize,
    pub(super) temporal_screen_count: usize,
    pub(super) pages_with_visual_phase_change: usize,
    pub(super) visual_review_passed: bool,
    pub(super) initial_selector_observed: bool,
    pub(super) page_reload_bound_to_build: bool,
    pub(super) final_exit_bound_to_build: bool,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    screen_role: String,
    target_record_id: String,
    source_sha1: String,
    workspace_sha1: String,
    output_sha1: String,
    runtime_binding: RuntimeBinding,
    initial_selector: InitialSelectorObservation,
    sample_frame_offsets: Vec<u64>,
    sample_artifact_roles: Vec<String>,
    completed_page_pointers: Vec<String>,
    page_mapper_registers: Vec<String>,
    sample_tree_sha1: String,
    temporal_visual_artifact_roles: Vec<String>,
    temporal_visual_tree_sha1: String,
    temporal_visual_review_passed: bool,
    exit: ExitObservation,
}

#[derive(Debug, Deserialize)]
struct RuntimeBinding {
    dialogue_selector: String,
    completed_page_state: String,
    completed_line_count: u8,
    current_pointer_low_address: String,
    current_pointer_high_address: String,
    proceed_input: String,
    first_page_entry: String,
    initial_selector_observed: bool,
}

#[derive(Debug, Deserialize)]
struct InitialSelectorObservation {
    entry_method: String,
    selector_address: String,
    target_hit_ordinal: u8,
    supply_state: String,
    supply_pointer: String,
    selected_mapper_register: String,
    artifact_roles: Vec<String>,
    artifact_tree_sha1: String,
}

#[derive(Debug, Deserialize)]
struct ExitObservation {
    input: String,
    dialogue_state_immediately_after_input: String,
    next_story_outer_state: String,
    next_story_main_state: String,
    next_story_dialogue_state: String,
    original_english_visible: bool,
    artifact_roles: Vec<String>,
    artifact_tree_sha1: String,
}

pub(super) fn verify_maximum_dialogue_runtime_evidence(
    manifest_path: &Path,
    output_sha1: &str,
    workspace_sha1: &str,
    completed_page_pointers: &[u16],
    page_groups: &[usize],
    group_mapper_registers: &[u8],
) -> Result<MaximumDialogueRuntimeEvidence> {
    let manifest_bytes = fs::read(manifest_path).with_context(|| {
        format!(
            "read installed maximum-dialogue evidence {}",
            manifest_path.display()
        )
    })?;
    let manifest: RuntimeManifest = serde_json::from_slice(&manifest_bytes).with_context(|| {
        format!(
            "parse installed maximum-dialogue evidence {}",
            manifest_path.display()
        )
    })?;
    ensure!(
        manifest.screen_role == SCREEN_ROLE
            && manifest.target_record_id == TARGET_RECORD_ID
            && manifest.source_sha1 == EXPECTED_SOURCE_SHA1
            && manifest.workspace_sha1 == workspace_sha1
            && manifest.output_sha1.eq_ignore_ascii_case(output_sha1),
        "installed maximum-dialogue runtime evidence is not bound to this output"
    );
    ensure!(
        manifest.runtime_binding.dialogue_selector == "C0:18"
            && manifest.runtime_binding.completed_page_state == "0x0E"
            && manifest.runtime_binding.completed_line_count == 4
            && manifest.runtime_binding.current_pointer_low_address == "0x7812"
            && manifest.runtime_binding.current_pointer_high_address == "0x7814"
            && manifest.runtime_binding.proceed_input == "A"
            && manifest.runtime_binding.first_page_entry == "state_bridged_chapter_7_seize_route"
            && manifest.runtime_binding.initial_selector_observed,
        "installed maximum-dialogue runtime route or proof boundary changed"
    );
    ensure!(
        manifest.sample_frame_offsets == EXPECTED_FRAME_OFFSETS
            && manifest.sample_artifact_roles == EXPECTED_SAMPLE_ARTIFACT_ROLES
            && manifest.temporal_visual_artifact_roles == EXPECTED_TEMPORAL_VISUAL_ARTIFACT_ROLES,
        "installed maximum-dialogue temporal sample grid changed"
    );
    ensure!(
        completed_page_pointers.len() == COMPLETED_PAGE_COUNT
            && page_groups.len() == COMPLETED_PAGE_COUNT
            && manifest.completed_page_pointers.len() == COMPLETED_PAGE_COUNT
            && manifest.page_mapper_registers.len() == COMPLETED_PAGE_COUNT,
        "installed maximum-dialogue page coverage changed"
    );

    let observed_pointers = manifest
        .completed_page_pointers
        .iter()
        .map(|pointer| parse_hex_u16(pointer, "completed-page pointer"))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        observed_pointers == completed_page_pointers,
        "installed maximum-dialogue completed-page pointers changed"
    );
    let observed_mapper_registers = manifest
        .page_mapper_registers
        .iter()
        .map(|register| parse_hex_u8(register, "page mapper register"))
        .collect::<Result<Vec<_>>>()?;
    let expected_mapper_registers = page_groups
        .iter()
        .map(|group| {
            group_mapper_registers
                .get(*group)
                .copied()
                .context("maximum-dialogue page group has no mapper register")
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        observed_mapper_registers == expected_mapper_registers,
        "installed maximum-dialogue page mapper sequence changed"
    );

    let parent = manifest_path
        .parent()
        .context("installed maximum-dialogue evidence has no parent directory")?;
    let initial_mapper_register = group_mapper_registers
        .first()
        .copied()
        .context("maximum-dialogue runtime has no initial mapper register")?;
    verify_initial_selector_evidence(parent, &manifest.initial_selector, initial_mapper_register)?;
    let mut sample_tree = Vec::new();
    let mut unique_nametables = BTreeSet::new();
    let mut pages_with_visual_phase_change = 0;
    let mut prior_page_base_frame = None;
    for (page_index, (pointer, mapper_register)) in completed_page_pointers
        .iter()
        .zip(&expected_mapper_registers)
        .enumerate()
    {
        let mut page_base_frame = None;
        let mut page_screens = BTreeSet::new();
        for frame_offset in EXPECTED_FRAME_OFFSETS {
            let directory = format!("page-{:02}/frame-{frame_offset:04}", page_index + 1);
            let state_bytes =
                read_tree_artifact(parent, &format!("{directory}/state.json"), &mut sample_tree)?;
            let prg_ram =
                read_tree_artifact(parent, &format!("{directory}/prgram.bin"), &mut sample_tree)?;
            let nametable = read_tree_artifact(
                parent,
                &format!("{directory}/nametable.bin"),
                &mut sample_tree,
            )?;
            let screen =
                read_tree_artifact(parent, &format!("{directory}/screen.png"), &mut sample_tree)?;
            ensure!(
                prg_ram.len() == PRG_RAM_SIZE
                    && nametable.len() == NAMETABLE_SIZE
                    && screen.starts_with(PNG_SIGNATURE),
                "installed maximum-dialogue runtime artifact size changed at {directory}"
            );

            let state: Value = serde_json::from_slice(&state_bytes)
                .with_context(|| format!("parse installed runtime state at {directory}"))?;
            ensure!(
                state_u64(&state, "mapper.registers2")? == u64::from(*mapper_register),
                "installed maximum-dialogue mapper register changed at {directory}"
            );
            let frame_count = state_u64(&state, "ppu.frameCount")?;
            let base_frame = *page_base_frame.get_or_insert(frame_count);
            ensure!(
                frame_count == base_frame + frame_offset,
                "installed maximum-dialogue frame offset changed at {directory}"
            );
            ensure!(
                prg_ram[prg_ram_offset(DIALOGUE_STATE_ADDRESS)?] == 0x0E
                    && prg_ram[prg_ram_offset(COMPLETED_LINE_COUNT_ADDRESS)?] == 4,
                "installed maximum-dialogue completed-page state changed at {directory}"
            );
            let observed_pointer = u16::from_le_bytes([
                prg_ram[prg_ram_offset(CURRENT_POINTER_LOW_ADDRESS)?],
                prg_ram[prg_ram_offset(CURRENT_POINTER_HIGH_ADDRESS)?],
            ]);
            ensure!(
                observed_pointer == *pointer,
                "installed maximum-dialogue pointer changed at {directory}"
            );
            unique_nametables.insert(sha1_hex(&nametable));
            page_screens.insert(sha1_hex(&screen));
        }
        let page_base_frame = page_base_frame.context("maximum-dialogue page has no samples")?;
        if let Some(prior) = prior_page_base_frame {
            ensure!(
                page_base_frame > prior,
                "installed maximum-dialogue page samples are not in route order"
            );
        }
        ensure!(
            page_screens.len() >= 2,
            "installed maximum-dialogue temporal samples lost the visual phase change on page {}",
            page_index + 1
        );
        pages_with_visual_phase_change += 1;
        prior_page_base_frame = Some(page_base_frame);
    }
    ensure!(
        sha1_hex(&sample_tree) == manifest.sample_tree_sha1,
        "installed maximum-dialogue sample tree SHA-1 changed"
    );

    let mut temporal_visual_tree = Vec::new();
    for artifact in EXPECTED_TEMPORAL_VISUAL_ARTIFACT_ROLES {
        let bytes = read_tree_artifact(parent, artifact, &mut temporal_visual_tree)?;
        ensure!(
            bytes.starts_with(PNG_SIGNATURE),
            "installed maximum-dialogue temporal contact sheet is not PNG: {artifact}"
        );
    }
    ensure!(
        sha1_hex(&temporal_visual_tree) == manifest.temporal_visual_tree_sha1
            && manifest.temporal_visual_review_passed,
        "installed maximum-dialogue temporal visual review changed"
    );

    ensure!(
        manifest.exit.input == "A"
            && manifest.exit.dialogue_state_immediately_after_input == "0x0F"
            && manifest.exit.next_story_outer_state == "0x0D"
            && manifest.exit.next_story_main_state == "0x03"
            && manifest.exit.next_story_dialogue_state == "0x00"
            && manifest.exit.original_english_visible
            && manifest.exit.artifact_roles == EXPECTED_EXIT_ARTIFACT_ROLES,
        "installed maximum-dialogue final-exit observation changed"
    );
    let mut exit_tree = Vec::new();
    let immediate_exit_events =
        read_tree_artifact(parent, "final-exit-immediate-events.json", &mut exit_tree)?;
    let exit_state = read_tree_artifact(parent, "next-story/state.json", &mut exit_tree)?;
    let exit_prg_ram = read_tree_artifact(parent, "next-story/prgram.bin", &mut exit_tree)?;
    let exit_internal_ram = read_tree_artifact(parent, "next-story/iram.bin", &mut exit_tree)?;
    let exit_nametable = read_tree_artifact(parent, "next-story/nametable.bin", &mut exit_tree)?;
    let exit_image = read_tree_artifact(parent, "next-story.png", &mut exit_tree)?;
    ensure!(
        exit_prg_ram.len() == PRG_RAM_SIZE
            && exit_internal_ram.len() == INTERNAL_RAM_SIZE
            && exit_nametable.len() == NAMETABLE_SIZE
            && exit_image.starts_with(PNG_SIGNATURE),
        "installed maximum-dialogue final-exit artifact size changed"
    );
    verify_final_exit_event(&immediate_exit_events)?;
    let _: Value = serde_json::from_slice(&exit_state)
        .context("parse installed maximum-dialogue final-exit state")?;
    ensure!(
        exit_prg_ram[prg_ram_offset(DIALOGUE_STATE_ADDRESS)?] == 0x00
            && exit_internal_ram[OUTER_STATE_ADDRESS] == 0x0D
            && exit_internal_ram[MAIN_STATE_ADDRESS] == 0x03,
        "installed maximum-dialogue final exit no longer reaches NEXT STORY"
    );
    ensure!(
        sha1_hex(&exit_tree) == manifest.exit.artifact_tree_sha1,
        "installed maximum-dialogue final-exit artifact tree SHA-1 changed"
    );

    Ok(MaximumDialogueRuntimeEvidence {
        manifest_sha1: sha1_hex(&manifest_bytes),
        sample_count: COMPLETED_PAGE_COUNT * EXPECTED_FRAME_OFFSETS.len(),
        page_count: COMPLETED_PAGE_COUNT,
        unique_nametable_count: unique_nametables.len(),
        temporal_screen_count: COMPLETED_PAGE_COUNT * EXPECTED_FRAME_OFFSETS.len(),
        pages_with_visual_phase_change,
        visual_review_passed: manifest.temporal_visual_review_passed,
        initial_selector_observed: manifest.runtime_binding.initial_selector_observed,
        page_reload_bound_to_build: true,
        final_exit_bound_to_build: true,
    })
}

fn verify_initial_selector_evidence(
    parent: &Path,
    observation: &InitialSelectorObservation,
    expected_mapper_register: u8,
) -> Result<()> {
    let expected_write_artifact =
        format!("initial-selector/{expected_mapper_register:02x}-write.mss");
    let expected_artifact_roles = [
        "initial-selector/events.json",
        expected_write_artifact.as_str(),
    ];
    ensure!(
        observation.entry_method == "mapper165_bridge_then_chapter_7_pre_castle_command_state"
            && observation.selector_address == "0xF990"
            && observation.target_hit_ordinal == 2
            && observation.supply_state == "0x05"
            && observation.supply_pointer == "0x8FF1"
            && parse_hex_u8(
                &observation.selected_mapper_register,
                "initial-selector mapper register",
            )? == expected_mapper_register
            && observation.artifact_roles == expected_artifact_roles,
        "installed maximum-dialogue initial-selector observation changed"
    );

    let mut tree = Vec::new();
    let events_bytes = read_tree_artifact(parent, "initial-selector/events.json", &mut tree)?;
    read_tree_artifact(parent, &expected_write_artifact, &mut tree)?;
    ensure!(
        sha1_hex(&tree) == observation.artifact_tree_sha1,
        "installed maximum-dialogue initial-selector artifact tree SHA-1 changed"
    );

    let event_log: Value = serde_json::from_slice(&events_bytes)
        .context("parse installed maximum-dialogue initial-selector events")?;
    ensure!(
        event_log.get("dropped").and_then(Value::as_u64) == Some(0),
        "installed maximum-dialogue initial-selector event log lost events"
    );
    let events = event_log
        .get("events")
        .and_then(Value::as_array)
        .context("installed maximum-dialogue initial-selector event list is missing")?;
    ensure!(
        events.len() == 3
            && event_string(&events[0], "kind")? == "exec"
            && event_u64(&events[0], "address")? == INITIAL_SELECTOR_ADDRESS
            && event_string(&events[1], "kind")? == "exec"
            && event_u64(&events[1], "address")? == INITIAL_SELECTOR_ADDRESS
            && event_string(&events[2], "kind")? == "write"
            && event_u64(&events[2], "address")? == INITIAL_SELECTOR_WRITE_ADDRESS
            && event_u64(&events[2], "pc")? == INITIAL_SELECTOR_WRITE_PC
            && event_u64(&events[2], "value")? == u64::from(expected_mapper_register)
            && event_u64(&events[1], "frame")? == event_u64(&events[2], "frame")?,
        "installed maximum-dialogue initial-selector event sequence changed"
    );

    let target_internal_ram = event_snapshot(&events[1], "nesInternalRam", 0x20)?;
    for (address, expected) in [
        (0x24, 0x0C),
        (0x84, 0x3C),
        (0x59, 0x1B),
        (0x5A, 0x1B),
        (0x5B, 0x00),
        (0x5C, 0x18),
    ] {
        ensure!(
            snapshot_byte(&target_internal_ram, 0x20, address)? == expected,
            "installed maximum-dialogue initial-selector zero-page predicate changed at ${address:02X}"
        );
    }
    let target_cpu_memory = event_snapshot(&events[1], "nesMemory", 0x7670)?;
    for (address, expected) in [
        (0x7674, 0x07),
        (0x77F1, 0x18),
        (0x77F2, 0x0C),
        (0x77F4, 0xC0),
        (0x77F7, 0x05),
        (0x7812, 0xF1),
        (0x7814, 0x8F),
    ] {
        ensure!(
            snapshot_byte(&target_cpu_memory, 0x7670, address)? == expected,
            "installed maximum-dialogue initial-selector supply predicate changed at ${address:04X}"
        );
    }
    Ok(())
}

fn verify_final_exit_event(bytes: &[u8]) -> Result<()> {
    let event_log: Value = serde_json::from_slice(bytes)
        .context("parse installed maximum-dialogue final-exit event")?;
    ensure!(
        event_log.get("dropped").and_then(Value::as_u64) == Some(0),
        "installed maximum-dialogue final-exit event log lost events"
    );
    let events = event_log
        .get("events")
        .and_then(Value::as_array)
        .context("installed maximum-dialogue final-exit event list is missing")?;
    ensure!(
        events.len() == 1
            && event_string(&events[0], "kind")? == "write"
            && event_u64(&events[0], "address")? == DIALOGUE_STATE_ADDRESS as u64
            && event_u64(&events[0], "pc")? == 0x85E4
            && event_u64(&events[0], "value")? == 0x0F,
        "installed maximum-dialogue final-exit write event changed"
    );
    let pre_write_memory = event_snapshot(&events[0], "nesMemory", 0x77F0)?;
    ensure!(
        snapshot_byte(&pre_write_memory, 0x77F0, DIALOGUE_STATE_ADDRESS)? == 0x0E,
        "installed maximum-dialogue final-exit input did not leave completed-page state"
    );
    Ok(())
}

fn event_u64(event: &Value, key: &str) -> Result<u64> {
    event
        .get(key)
        .and_then(Value::as_u64)
        .with_context(|| format!("installed maximum-dialogue event lost {key}"))
}

fn event_string<'a>(event: &'a Value, key: &str) -> Result<&'a str> {
    event
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("installed maximum-dialogue event lost {key}"))
}

fn event_snapshot(event: &Value, memory_type: &str, address: usize) -> Result<Vec<u8>> {
    let snapshots = event
        .get("snapshot")
        .and_then(Value::as_array)
        .context("installed maximum-dialogue event lost snapshots")?;
    let snapshot = snapshots
        .iter()
        .find(|snapshot| {
            snapshot.get("memory_type").and_then(Value::as_str) == Some(memory_type)
                && snapshot.get("address").and_then(Value::as_u64) == Some(address as u64)
        })
        .with_context(|| {
            format!(
                "installed maximum-dialogue event lost {memory_type} snapshot at ${address:04X}"
            )
        })?;
    parse_hex_bytes(
        snapshot
            .get("hex")
            .and_then(Value::as_str)
            .context("installed maximum-dialogue event snapshot lost hex bytes")?,
    )
}

fn snapshot_byte(snapshot: &[u8], base: usize, address: usize) -> Result<u8> {
    let offset = address
        .checked_sub(base)
        .context("installed maximum-dialogue snapshot address precedes its base")?;
    snapshot
        .get(offset)
        .copied()
        .with_context(|| format!("installed maximum-dialogue snapshot lost ${address:04X}"))
}

fn parse_hex_bytes(hex: &str) -> Result<Vec<u8>> {
    ensure!(
        hex.len().is_multiple_of(2),
        "installed maximum-dialogue event snapshot has an odd hex length"
    );
    (0..hex.len())
        .step_by(2)
        .map(|offset| {
            u8::from_str_radix(&hex[offset..offset + 2], 16)
                .context("parse installed maximum-dialogue event snapshot byte")
        })
        .collect()
}

fn read_tree_artifact(parent: &Path, relative: &str, tree: &mut Vec<u8>) -> Result<Vec<u8>> {
    let bytes = fs::read(parent.join(relative))
        .with_context(|| format!("read installed maximum-dialogue artifact {relative}"))?;
    tree.extend_from_slice(relative.as_bytes());
    tree.push(0);
    tree.extend_from_slice(&bytes);
    tree.push(0xFF);
    Ok(bytes)
}

fn state_u64(state: &Value, key: &str) -> Result<u64> {
    state
        .get(key)
        .and_then(Value::as_u64)
        .with_context(|| format!("installed maximum-dialogue state lost {key}"))
}

fn prg_ram_offset(cpu_address: usize) -> Result<usize> {
    ensure!(
        (PRG_RAM_CPU_BASE..PRG_RAM_CPU_BASE + PRG_RAM_SIZE).contains(&cpu_address),
        "maximum-dialogue PRG-RAM address is outside the dump"
    );
    Ok(cpu_address - PRG_RAM_CPU_BASE)
}

fn parse_hex_u16(value: &str, role: &str) -> Result<u16> {
    let digits = value
        .strip_prefix("0x")
        .with_context(|| format!("maximum-dialogue {role} is not 0x-prefixed"))?;
    ensure!(
        digits.len() == 4,
        "maximum-dialogue {role} must have four hexadecimal digits"
    );
    u16::from_str_radix(digits, 16)
        .with_context(|| format!("parse maximum-dialogue {role} {value}"))
}

fn parse_hex_u8(value: &str, role: &str) -> Result<u8> {
    let digits = value
        .strip_prefix("0x")
        .with_context(|| format!("maximum-dialogue {role} is not 0x-prefixed"))?;
    ensure!(
        digits.len() == 2,
        "maximum-dialogue {role} must have two hexadecimal digits"
    );
    u8::from_str_radix(digits, 16).with_context(|| format!("parse maximum-dialogue {role} {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_addresses_are_projected_from_cpu_to_prg_ram() {
        assert_eq!(prg_ram_offset(0x77F7).unwrap(), 0x17F7);
        assert_eq!(prg_ram_offset(0x7814).unwrap(), 0x1814);
        assert!(prg_ram_offset(0x5FFF).is_err());
        assert!(prg_ram_offset(0x8000).is_err());
    }
}
