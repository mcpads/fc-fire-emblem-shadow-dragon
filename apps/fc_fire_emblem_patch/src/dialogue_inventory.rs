mod battle_dialogue;
mod main_dialogue_graph;
mod main_dialogue_record;
mod main_dialogue_state;
mod main_dialogue_translation_view;
mod menu_layout;
mod report;
mod source_binding;
mod source_spec;
#[cfg(test)]
mod tests;

use battle_dialogue::*;
use main_dialogue_graph::*;
pub(crate) use main_dialogue_record::inspect_main_dialogue_fixed_text_width;
use main_dialogue_record::*;
use main_dialogue_state::*;
use main_dialogue_translation_view::{
    build_main_dialogue_storage_records, safe_main_dialogue_japanese_literal_offsets,
};
pub(crate) use main_dialogue_translation_view::{
    inspect_main_dialogue_runtime_identities, inspect_main_dialogue_storage,
};
pub(crate) use menu_layout::{
    MainDialogueMenuLayoutBounds, inspect_main_dialogue_menu_layout_bounds,
};
pub(crate) use report::*;
use source_binding::{
    extract_dialogue_table, fixed_cpu_to_file_offset, switchable_bank_file_start,
};
pub(crate) use source_binding::{switchable_cpu_to_file_offset, switchable_file_to_cpu};
use source_spec::*;
pub(crate) use source_spec::{
    MAIN_DIALOGUE_CALLER_HANDOFF_FLAG_ADDRESS, MAIN_DIALOGUE_COMPLETION_FLAG_ADDRESS,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    japanese_encoding::is_japanese_text_code,
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, PRG_SIZE, Rom},
    sha1_hex,
    source_literals::{
        TranslationSurfaceLiteralInventory, classify_translation_surface_literal_codes,
    },
    text_inventory::{DIALOGUE_CONTROL_SPECS, DIALOGUE_SCRIPT_CONTROL_CODES},
    typed_source::decode_rp2a03_sequence,
};

#[derive(Debug)]
pub struct DialogueStructureSummary {
    pub report_sha1: String,
    pub table_count: usize,
    pub pointer_count: usize,
    pub unique_target_count: usize,
    pub alias_group_count: usize,
}

pub fn analyze_dialogue_structure(
    source_path: &Path,
    report_path: &Path,
) -> Result<DialogueStructureSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let report = build_report(rom.data())?;
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize dialogue structure report")?;
    report_bytes.push(b'\n');

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, &report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(DialogueStructureSummary {
        report_sha1: sha1_hex(&report_bytes),
        table_count: report.summary.table_count,
        pointer_count: report.summary.pointer_count,
        unique_target_count: report.summary.unique_target_count,
        alias_group_count: report.summary.alias_group_count,
    })
}

pub(crate) fn inspect_main_dialogue_graph(source: &[u8]) -> Result<MainDialogueGraphReport> {
    Ok(build_report(source)?.main_dialogue_graph)
}

pub(crate) fn main_dialogue_transition_chain_record_ids(
    graph: &MainDialogueGraphReport,
    root_table_id: &str,
    root_entry_index: usize,
) -> Result<Vec<String>> {
    let mut next = BTreeMap::new();
    for edge in &graph.transition_edges {
        ensure!(
            next.insert(
                (edge.source_table_id, edge.source_canonical_entry_index),
                (edge.target_table_id, edge.target_canonical_entry_index),
            )
            .is_none(),
            "main-dialogue record has multiple transition targets"
        );
    }
    let mut chain = Vec::new();
    let mut current = (root_table_id, root_entry_index);
    loop {
        ensure!(
            !chain.contains(&current),
            "main-dialogue transition chain contains a cycle"
        );
        chain.push(current);
        let Some(target) = next.get(&current).copied() else {
            break;
        };
        current = target;
    }
    Ok(chain
        .into_iter()
        .map(|(table_id, entry_index)| format!("{table_id}:{entry_index:03}"))
        .collect())
}

pub(crate) const fn main_dialogue_runtime_handler_roots() -> [u16; 18] {
    MAIN_DIALOGUE_STATE_HANDLERS
}

pub(crate) const MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES: [u8; 3] = [0x12, 0x1F, 0x20];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MainDialogueCompositeAppenderSource {
    pub(crate) dialogue_state: u8,
    pub(crate) composite_state: u8,
    pub(crate) prg_bank: u8,
    pub(crate) load_address: u16,
    pub(crate) transfer_address: u16,
}

#[derive(Clone, Copy)]
struct MainDialogueCompositeAppenderSpec {
    dialogue_state: u8,
    composite_state: u8,
    handler_address: u16,
    load_address: u16,
    code_region_role: &'static str,
}

const MAIN_DIALOGUE_COMPOSITE_APPENDER_SPECS: [MainDialogueCompositeAppenderSpec; 3] = [
    MainDialogueCompositeAppenderSpec {
        dialogue_state: 0x02,
        composite_state: MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES[0],
        handler_address: 0x80A2,
        load_address: 0x80DD,
        code_region_role: "inspect_and_consume_optional_E5_prefix",
    },
    MainDialogueCompositeAppenderSpec {
        dialogue_state: 0x04,
        composite_state: MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES[1],
        handler_address: 0x80E6,
        load_address: 0x8110,
        code_region_role: "consume_fixed_four_byte_record_header",
    },
    MainDialogueCompositeAppenderSpec {
        dialogue_state: 0x06,
        composite_state: MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES[2],
        handler_address: 0x8126,
        load_address: 0x8168,
        code_region_role: "publish_optional_E8_composite_and_display_flags",
    },
];

/// Binds the three bank-0A dialogue handlers that hand decoded record metadata to the
/// bank-0B composite appender. These are auxiliary parts of one live dialogue surface, not
/// independent screens that may choose a font page from the composite-state byte alone.
pub(crate) fn bind_main_dialogue_composite_appenders(
    rom: &Rom,
) -> Result<Vec<MainDialogueCompositeAppenderSource>> {
    rom.verify_supported_japanese()?;
    bind_main_dialogue_composite_appenders_in_source(rom.data())
}

fn bind_main_dialogue_composite_appenders_in_source(
    source: &[u8],
) -> Result<Vec<MainDialogueCompositeAppenderSource>> {
    let state_machine = build_main_dialogue_state_machine(source)?;
    let mut routes = Vec::with_capacity(MAIN_DIALOGUE_COMPOSITE_APPENDER_SPECS.len());
    for spec in MAIN_DIALOGUE_COMPOSITE_APPENDER_SPECS {
        let handler = state_machine
            .handlers
            .get(usize::from(spec.dialogue_state))
            .context("main-dialogue composite appender lost its dialogue state")?;
        let transfer_address = spec
            .load_address
            .checked_add(2)
            .context("main-dialogue composite transfer address overflow")?;
        let sequence_end = spec
            .load_address
            .checked_add(5)
            .context("main-dialogue composite producer range overflow")?;
        let containing_region_count = state_machine
            .code_regions
            .iter()
            .filter(|region| {
                let region_start = usize::from(region.cpu_address);
                let region_end = region_start + region.byte_count;
                region.role == spec.code_region_role
                    && region_start <= usize::from(spec.load_address)
                    && region_end >= usize::from(sequence_end)
            })
            .count();
        ensure!(
            containing_region_count == 1,
            "main-dialogue composite state {:02X} lost its unique owning code region",
            spec.composite_state
        );
        ensure!(
            handler.cpu_address == spec.handler_address,
            "main-dialogue composite state {:02X} left its owning handler",
            spec.composite_state
        );

        let file_offset = switchable_cpu_to_file_offset(MAIN_DIALOGUE_PRG_BANK, spec.load_address)?;
        let expected = [0xA9, spec.composite_state, 0x20, 0x90, 0xE6];
        ensure!(
            source.get(file_offset..file_offset + expected.len()) == Some(expected.as_slice()),
            "main-dialogue composite state {:02X} producer changed",
            spec.composite_state
        );
        decode_rp2a03_sequence(
            &expected,
            spec.load_address,
            "publish one main-dialogue auxiliary composite",
        )?;
        routes.push(MainDialogueCompositeAppenderSource {
            dialogue_state: spec.dialogue_state,
            composite_state: spec.composite_state,
            prg_bank: MAIN_DIALOGUE_PRG_BANK,
            load_address: spec.load_address,
            transfer_address,
        });
    }
    ensure!(
        routes
            .iter()
            .map(|route| route.composite_state)
            .collect::<Vec<_>>()
            == MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES,
        "main-dialogue composite appender state family changed"
    );
    Ok(routes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MainDialogueProgressSource {
    caller_prg_bank: u8,
    caller_handler_address: u16,
    caller_observer_address: u16,
    dialogue_prg_bank: u8,
    dialogue_dispatcher_address: u16,
    completion_flag_address: u16,
    caller_handoff_flag_address: u16,
    pending_value: u8,
    asserted_value: u8,
}

impl MainDialogueProgressSource {
    pub(crate) fn caller_prg_bank(self) -> u8 {
        self.caller_prg_bank
    }

    pub(crate) fn caller_handler_address(self) -> u16 {
        self.caller_handler_address
    }

    pub(crate) fn caller_observer_address(self) -> u16 {
        self.caller_observer_address
    }

    pub(crate) fn dialogue_prg_bank(self) -> u8 {
        self.dialogue_prg_bank
    }

    pub(crate) fn dialogue_dispatcher_address(self) -> u16 {
        self.dialogue_dispatcher_address
    }

    pub(crate) fn completion_flag_address(self) -> u16 {
        self.completion_flag_address
    }

    pub(crate) fn caller_handoff_flag_address(self) -> u16 {
        self.caller_handoff_flag_address
    }

    pub(crate) fn pending_value(self) -> u8 {
        self.pending_value
    }

    pub(crate) fn asserted_value(self) -> u8 {
        self.asserted_value
    }
}

/// Binds the main-dialogue progress signals and one exact caller observer. The returned values
/// describe the engine-owned zero-to-one signal transition; they do not claim that every caller
/// or every dialogue route is reachable.
pub(crate) fn bind_main_dialogue_progress_source(
    rom: &Rom,
    caller_prg_bank: u8,
    caller_handler_address: u16,
) -> Result<MainDialogueProgressSource> {
    rom.verify_supported_japanese()?;
    build_main_dialogue_state_machine(rom.data())?;

    let observers = CALLER_HANDOFF_OBSERVER_SPECS
        .iter()
        .copied()
        .filter(|observer| {
            observer.prg_bank == caller_prg_bank
                && observer.handler_cpu_address == caller_handler_address
        })
        .collect::<Vec<_>>();
    ensure!(
        observers.len() == 1,
        "main-dialogue progress source expected one caller observer at bank {caller_prg_bank:02X}:${caller_handler_address:04X}, found {}",
        observers.len(),
    );
    let observer = observers[0];
    let dispatch_bindings = build_caller_handoff_dispatch_bindings(rom.data(), observer)?;
    ensure!(
        !dispatch_bindings.is_empty(),
        "main-dialogue caller observer has no owner-bound state dispatcher"
    );

    Ok(MainDialogueProgressSource {
        caller_prg_bank,
        caller_handler_address,
        caller_observer_address: observer.cpu_address,
        dialogue_prg_bank: MAIN_DIALOGUE_PRG_BANK,
        dialogue_dispatcher_address: MAIN_DIALOGUE_DISPATCHER_CPU_ADDRESS,
        completion_flag_address: MAIN_DIALOGUE_COMPLETION_FLAG_ADDRESS,
        caller_handoff_flag_address: MAIN_DIALOGUE_CALLER_HANDOFF_FLAG_ADDRESS,
        pending_value: 0,
        asserted_value: 1,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CallerHandoffStateDispatchSource {
    prg_bank: u8,
    call_address: u16,
    selector_address: u16,
    selector_domain: BTreeSet<u8>,
    handler_targets: Vec<u16>,
}

impl CallerHandoffStateDispatchSource {
    pub(crate) fn prg_bank(&self) -> u8 {
        self.prg_bank
    }

    pub(crate) fn call_address(&self) -> u16 {
        self.call_address
    }

    pub(crate) fn selector_address(&self) -> u16 {
        self.selector_address
    }

    pub(crate) fn selector_domain(&self) -> &BTreeSet<u8> {
        &self.selector_domain
    }

    pub(crate) fn handler_target(&self, selector: u8) -> Option<u16> {
        self.handler_targets.get(usize::from(selector)).copied()
    }
}

/// Returns the exact caller-owned state domains for the dispatch tables that can resume a main
/// dialogue handoff. The state-machine inspection binds every call prefix and every pointer slot
/// before these domains are exposed to another execution analysis.
pub(crate) fn bind_caller_handoff_state_dispatch_sources(
    rom: &Rom,
) -> Result<Vec<CallerHandoffStateDispatchSource>> {
    rom.verify_supported_japanese()?;
    build_main_dialogue_state_machine(rom.data())?;

    CALLER_HANDOFF_DISPATCH_SPECS
        .iter()
        .map(|spec| {
            let selector_count = u8::try_from(spec.handlers.len())
                .context("caller-handoff selector count exceeds one byte")?;
            ensure!(
                selector_count != 0 && selector_count <= 0x80,
                "caller-handoff selector domain is empty or aliases through eight-bit ASL"
            );
            let call_address = spec
                .dispatcher_cpu_address
                .checked_add(3)
                .context("caller-handoff dispatch call address overflow")?;
            ensure!(
                spec.handler_table_cpu_address == call_address + 3,
                "caller-handoff table no longer follows its dispatch call"
            );
            Ok(CallerHandoffStateDispatchSource {
                prg_bank: spec.prg_bank,
                call_address,
                selector_address: spec.state_address,
                selector_domain: (0..selector_count).collect(),
                handler_targets: spec.handlers.to_vec(),
            })
        })
        .collect()
}

pub(crate) fn inspect_battle_dialogue_translation_records(
    source: &[u8],
) -> Result<Vec<BattleDialogueTranslationRecord>> {
    let report = build_report(source)?;
    let table = report
        .tables
        .iter()
        .find(|table| table.id == BATTLE_DIALOGUE_TABLE_ID)
        .context("battle-dialogue table is absent")?;
    let mut entry_indices_by_pointer = BTreeMap::<u16, Vec<usize>>::new();
    for entry in &table.entries {
        entry_indices_by_pointer
            .entry(entry.pointer_cpu_address)
            .or_default()
            .push(entry.index);
    }

    let mut records = Vec::new();
    for entry in &table.entries {
        let entry_indices = &entry_indices_by_pointer[&entry.pointer_cpu_address];
        if entry_indices[0] != entry.index {
            continue;
        }
        let storage = entry
            .battle_record_storage
            .as_ref()
            .context("canonical battle-dialogue entry has no storage report")?;
        let pointer_file_offsets = entry_indices
            .iter()
            .map(|index| table.pointer_table_file_offset + index * 2)
            .collect::<Vec<_>>();
        records.push(BattleDialogueTranslationRecord {
            table_id: table.id,
            source_prg_bank: table.source_prg_bank,
            canonical_entry_index: entry.index,
            entry_indices: entry_indices.clone(),
            pointer_cpu_address: entry.pointer_cpu_address,
            pointer_file_offsets,
            file_offset: storage.file_offset,
            end_file_offset_exclusive: storage.end_file_offset_exclusive,
            storage_sha1: storage.storage_sha1.clone(),
            header_hex: storage.header_hex.clone(),
            literal_file_offsets: storage.literal_file_offsets.clone(),
        });
    }
    ensure!(
        records.len() == 28,
        "battle-dialogue translation record count changed"
    );
    records.sort_by_key(|record| record.file_offset);
    ensure!(
        records
            .windows(2)
            .all(|pair| pair[0].file_offset < pair[1].file_offset),
        "battle-dialogue translation record storage overlaps or aliases"
    );
    Ok(records)
}

pub(crate) fn inspect_battle_dialogue_physical_layout(
    source: &[u8],
) -> Result<BattleDialoguePhysicalLayout> {
    let report = build_report(source)?;
    let table = report
        .tables
        .iter()
        .find(|table| table.id == BATTLE_DIALOGUE_TABLE_ID)
        .context("battle-dialogue table is absent")?;
    let summary = table
        .battle_record_storage_summary
        .as_ref()
        .context("battle-dialogue physical storage summary is absent")?;
    ensure!(
        summary.unreferenced_records.len() == 1,
        "battle-dialogue unreferenced record count changed"
    );
    let unreferenced = &summary.unreferenced_records[0];
    Ok(BattleDialoguePhysicalLayout {
        data_file_start: table.data_file_start,
        data_file_end_exclusive: summary.physical_data_file_end_exclusive,
        preserved_unreferenced_file_offset: unreferenced.file_offset,
        preserved_unreferenced_end_file_offset_exclusive: unreferenced.end_file_offset_exclusive,
        preserved_unreferenced_storage_sha1: unreferenced.storage_sha1.clone(),
    })
}

pub(crate) fn inspect_chapter_intro_contexts(
    source: &[u8],
) -> Result<Vec<ChapterIntroContextBinding>> {
    let report = build_report(source)?;
    let table = report
        .tables
        .iter()
        .find(|table| table.id == "chapter-intro-dialogue")
        .context("chapter-intro dialogue table is absent")?;

    table
        .entries
        .iter()
        .filter(|entry| {
            entry.target_kind == "script_entry_start" && is_canonical_dialogue_entry(entry)
        })
        .filter(|entry| {
            entry
                .main_record_prefix
                .as_ref()
                .is_some_and(|prefix| prefix.e5_prefix_present)
        })
        .map(|entry| {
            let prefix_end = entry
                .file_offset
                .checked_add(OPTIONAL_PREFIX_BYTE_COUNT)
                .context("chapter-intro E5 prefix range overflow")?;
            let prefix = source
                .get(entry.file_offset..prefix_end)
                .context("chapter-intro E5 prefix is outside the source")?;
            ensure!(
                prefix[0] == OPTIONAL_E5_PREFIX_CODE,
                "chapter-intro E5 prefix marker changed"
            );
            let prefix_payload: [u8; OPTIONAL_PREFIX_BYTE_COUNT - 1] = prefix[1..]
                .try_into()
                .expect("E5 payload has a fixed length");
            let mut entry_indices = vec![entry.index];
            entry_indices.extend(entry.alias_entry_indices.iter().copied());
            entry_indices.sort_unstable();

            Ok(ChapterIntroContextBinding {
                canonical_entry_index: entry.index,
                entry_indices,
                file_offset: entry.file_offset,
                chapter_index: prefix_payload[4],
                prefix_payload,
            })
        })
        .collect()
}

pub(crate) fn inspect_shop_dialogue_table(source: &[u8]) -> Result<ShopDialogueTableBinding> {
    let spec = DIALOGUE_TABLE_SPECS
        .iter()
        .find(|spec| spec.id == "shop-and-item-dialogue")
        .context("shop-and-item dialogue table is absent")?;
    let report = extract_dialogue_table(source, spec)?;
    let directory = report
        .directory_binding
        .as_ref()
        .context("shop-and-item dialogue table has no directory binding")?;
    let first_entry = report
        .entries
        .first()
        .context("shop-and-item dialogue table is empty")?;

    Ok(ShopDialogueTableBinding {
        table_id: report.id,
        source_prg_bank: report.source_prg_bank,
        source_prg_bank_hex: report.source_prg_bank_hex,
        directory_selector: directory.selector,
        directory_selector_hex: directory.selector_hex.clone(),
        directory_entry_cpu_address: directory.directory_entry_cpu_address,
        directory_entry_cpu_address_hex: directory.directory_entry_cpu_address_hex.clone(),
        pointer_table_cpu_address: report.pointer_table_cpu_address,
        pointer_table_cpu_address_hex: report.pointer_table_cpu_address_hex,
        pointer_table_sha1: report.pointer_table_sha1,
        pointer_count: report.pointer_count,
        unique_target_count: report.unique_target_count,
        first_entry_pointer_cpu_address: first_entry.pointer_cpu_address,
        first_entry_pointer_cpu_address_hex: first_entry.pointer_cpu_address_hex.clone(),
    })
}

pub(crate) fn inspect_translation_surface_dialogue_tables(
    source: &[u8],
) -> Result<Vec<TranslationSurfaceDialogueTableBinding>> {
    const TABLE_IDS: [&str; 3] = [
        "battle-dialogue",
        "epilogue-dialogue",
        "epilogue-routing-dialogue",
    ];

    TABLE_IDS
        .into_iter()
        .map(|table_id| {
            let spec = DIALOGUE_TABLE_SPECS
                .iter()
                .find(|spec| spec.id == table_id)
                .with_context(|| {
                    format!("translation-surface dialogue table {table_id} is absent")
                })?;
            let report = extract_dialogue_table(source, spec)?;
            let directory_selector = report
                .directory_binding
                .as_ref()
                .map(|directory| directory.selector);
            let directory_selector_hex = report
                .directory_binding
                .as_ref()
                .map(|directory| directory.selector_hex.clone());
            let separate_loader_cpu_address = report
                .separate_consumer_binding
                .as_ref()
                .map(|consumer| consumer.loader_cpu_address);
            let separate_loader_cpu_address_hex = report
                .separate_consumer_binding
                .as_ref()
                .map(|consumer| consumer.loader_cpu_address_hex.clone());
            let proven_record_count = report
                .main_record_storage_summary
                .as_ref()
                .map(|summary| summary.unique_record_count)
                .or_else(|| {
                    report
                        .battle_record_storage_summary
                        .as_ref()
                        .map(|summary| summary.pointer_referenced_record_count)
                });
            let unique_record_storage_byte_count = report
                .main_record_storage_summary
                .as_ref()
                .map(|summary| summary.unique_storage_byte_count)
                .or_else(|| {
                    report
                        .battle_record_storage_summary
                        .as_ref()
                        .map(|summary| summary.unique_storage_byte_count)
                });
            let unreferenced_record_count = report
                .battle_record_storage_summary
                .as_ref()
                .map(|summary| summary.unreferenced_record_count);
            let (literal_inventory, literal_file_offsets) =
                translation_surface_literal_inventory(source, &report)?;

            Ok(TranslationSurfaceDialogueTableBinding {
                table_id: report.id,
                source_prg_bank: report.source_prg_bank,
                source_prg_bank_hex: report.source_prg_bank_hex,
                pointer_table_cpu_address: report.pointer_table_cpu_address,
                pointer_table_cpu_address_hex: report.pointer_table_cpu_address_hex,
                pointer_table_sha1: report.pointer_table_sha1,
                pointer_count: report.pointer_count,
                unique_target_count: report.unique_target_count,
                consumer_binding_status: report.consumer_binding_status,
                directory_selector,
                directory_selector_hex,
                separate_loader_cpu_address,
                separate_loader_cpu_address_hex,
                proven_record_count,
                unique_record_storage_byte_count,
                unreferenced_record_count,
                literal_inventory,
                literal_file_offsets,
            })
        })
        .collect()
}

fn translation_surface_literal_inventory(
    source: &[u8],
    report: &DialogueTableReport,
) -> Result<(TranslationSurfaceLiteralInventory, BTreeSet<usize>)> {
    let literal_file_offsets = report
        .entries
        .iter()
        .filter(|entry| {
            entry.target_kind == "script_entry_start" && is_canonical_dialogue_entry(entry)
        })
        .map(|entry| {
            if report.id == BATTLE_DIALOGUE_TABLE_ID {
                entry
                    .battle_record_storage
                    .as_ref()
                    .context("canonical battle-dialogue entry has no literal boundaries")
                    .map(|record| record.literal_file_offsets.clone())
            } else {
                entry
                    .main_linear_segment
                    .as_ref()
                    .context("canonical epilogue entry has no literal boundaries")
                    .map(|segment| {
                        segment
                            .lines
                            .iter()
                            .flat_map(|line| line.literal_file_offsets.iter().copied())
                            .collect()
                    })
            }
        })
        .collect::<Result<Vec<Vec<usize>>>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();

    let inventory = literal_inventory_from_file_offsets(source, &literal_file_offsets, report.id)?;
    Ok((inventory, literal_file_offsets))
}

pub(crate) fn aggregate_translation_surface_dialogue_literal_inventory(
    source: &[u8],
    tables: &[TranslationSurfaceDialogueTableBinding],
    requested_table_ids: &[&str],
) -> Result<TranslationSurfaceLiteralInventory> {
    let mut seen_table_ids = BTreeSet::new();
    let mut literal_file_offsets = BTreeSet::new();
    let mut source_offset_count = 0;
    for table_id in requested_table_ids {
        ensure!(
            seen_table_ids.insert(*table_id),
            "duplicate translation-surface dialogue table id {table_id}"
        );
        let table = tables
            .iter()
            .find(|table| table.table_id == *table_id)
            .with_context(|| format!("translation-surface dialogue table {table_id} is absent"))?;
        source_offset_count += table.literal_file_offsets.len();
        literal_file_offsets.extend(table.literal_file_offsets.iter().copied());
    }
    ensure!(
        source_offset_count == literal_file_offsets.len(),
        "translation-surface dialogue tables overlap literal storage"
    );

    literal_inventory_from_file_offsets(source, &literal_file_offsets, "dialogue-table aggregate")
}

fn literal_inventory_from_file_offsets(
    source: &[u8],
    literal_file_offsets: &BTreeSet<usize>,
    inventory_role: &str,
) -> Result<TranslationSurfaceLiteralInventory> {
    let codes = literal_file_offsets
        .iter()
        .map(|file_offset| {
            source
                .get(*file_offset)
                .copied()
                .context("translation-surface literal offset is outside the source")
        })
        .collect::<Result<Vec<_>>>()?;
    classify_translation_surface_literal_codes(codes, inventory_role)
}

fn build_report(source: &[u8]) -> Result<DialogueStructureReport> {
    let main_dialogue_state_machine = build_main_dialogue_state_machine(source)?;
    let battle_dialogue_state_machine = build_battle_dialogue_state_machine(source)?;
    let tables = DIALOGUE_TABLE_SPECS
        .iter()
        .map(|spec| extract_dialogue_table(source, spec))
        .collect::<Result<Vec<_>>>()?;
    let main_dialogue_graph = build_main_dialogue_graph(&tables)?;
    let main_translation_records =
        build_main_dialogue_storage_records(source, &tables, &main_dialogue_graph)?;
    let main_translation_view_line_count = main_translation_records
        .iter()
        .map(|record| record.lines.len())
        .sum();
    let main_translation_view_safe_japanese_source_byte_count =
        safe_main_dialogue_japanese_literal_offsets(source, &main_translation_records)?.len();
    let main_transition_target_record_count = main_dialogue_graph
        .transition_edges
        .iter()
        .map(|edge| (edge.target_table_id, edge.target_canonical_entry_index))
        .collect::<BTreeSet<_>>()
        .len();
    let main_literal_storage_summary = summarize_main_literal_storage(source, &tables)?;
    let main_first_line_count = tables
        .iter()
        .filter_map(|table| table.main_first_line_summary.as_ref())
        .map(|summary| summary.unique_line_count)
        .sum();
    let max_main_first_line_storage_byte_count = tables
        .iter()
        .filter_map(|table| table.main_first_line_summary.as_ref())
        .map(|summary| summary.max_storage_byte_count)
        .max()
        .unwrap_or(0);
    let main_first_line_japanese_literal_byte_count = tables
        .iter()
        .filter_map(|table| table.main_first_line_summary.as_ref())
        .map(|summary| summary.japanese_literal_byte_count)
        .sum();
    let main_first_line_non_japanese_literal_byte_count = tables
        .iter()
        .filter_map(|table| table.main_first_line_summary.as_ref())
        .map(|summary| summary.non_japanese_literal_byte_count)
        .sum();
    let main_first_line_protected_original_alphanumeric_literal_byte_count = tables
        .iter()
        .filter_map(|table| table.main_first_line_summary.as_ref())
        .map(|summary| summary.protected_original_alphanumeric_literal_byte_count)
        .sum();
    let mut main_first_line_end_control_count_map = BTreeMap::new();
    for usage in tables
        .iter()
        .filter_map(|table| table.main_first_line_summary.as_ref())
        .flat_map(|summary| &summary.line_end_control_counts)
    {
        *main_first_line_end_control_count_map
            .entry(usage.code)
            .or_insert(0) += usage.count;
    }
    let main_first_line_end_control_counts =
        control_usage_reports(main_first_line_end_control_count_map, &MAIN_LINE_END_CODES);
    let main_linear_segment_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.unique_segment_count)
        .sum();
    let main_linear_line_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.total_line_count)
        .sum();
    let max_main_linear_segment_line_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.max_line_count)
        .max()
        .unwrap_or(0);
    let main_linear_segment_japanese_literal_byte_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.japanese_literal_byte_count)
        .sum();
    let main_linear_segment_non_japanese_literal_byte_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.non_japanese_literal_byte_count)
        .sum();
    let main_linear_segment_protected_original_alphanumeric_literal_byte_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.protected_original_alphanumeric_literal_byte_count)
        .sum();
    let mut main_linear_segment_boundary_control_count_map = BTreeMap::new();
    for usage in tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .flat_map(|summary| &summary.boundary_control_counts)
    {
        *main_linear_segment_boundary_control_count_map
            .entry(usage.code)
            .or_insert(0) += usage.count;
    }
    let main_linear_segment_boundary_control_counts = control_usage_reports(
        main_linear_segment_boundary_control_count_map,
        &MAIN_LINEAR_SEGMENT_BOUNDARY_CODES,
    );
    let main_linear_segment_transition_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.transition_count)
        .sum();
    let main_record_ranges = tables
        .iter()
        .filter(|table| table.directory_binding.is_some())
        .flat_map(|table| &table.entries)
        .filter(|entry| {
            entry.target_kind == "script_entry_start" && is_canonical_dialogue_entry(entry)
        })
        .map(|entry| {
            let storage = entry
                .main_record_storage
                .as_ref()
                .context("canonical main dialogue entry has no record-storage range")?;
            Ok(MainRecordStorageRange {
                start: storage.file_offset,
                end_exclusive: storage.end_file_offset_exclusive,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let main_record_storage_summary = summarize_main_record_storage(&main_record_ranges)?;
    let main_record_count = main_record_storage_summary.unique_record_count;
    let main_unique_script_entry_count: usize = tables
        .iter()
        .filter(|table| table.directory_binding.is_some())
        .map(|table| table.unique_script_entry_count)
        .sum();
    ensure!(
        main_first_line_count == main_unique_script_entry_count,
        "main first-line coverage does not match the directory-bound script entries"
    );
    ensure!(
        main_linear_segment_count == main_unique_script_entry_count,
        "main linear-segment coverage does not match the directory-bound script entries"
    );
    ensure!(
        main_record_count == main_unique_script_entry_count,
        "main record-storage coverage does not match the directory-bound script entries"
    );
    let battle_record_storage_summary = tables
        .iter()
        .find(|table| table.id == BATTLE_DIALOGUE_TABLE_ID)
        .and_then(|table| table.battle_record_storage_summary.as_ref())
        .context("battle-dialogue table has no record-storage summary")?;
    let battle_pointer_referenced_record_count =
        battle_record_storage_summary.pointer_referenced_record_count;
    let battle_unreferenced_record_count = battle_record_storage_summary.unreferenced_record_count;
    let battle_pointer_referenced_storage_byte_count =
        battle_record_storage_summary.unique_storage_byte_count;
    let battle_physical_record_storage_byte_count =
        battle_record_storage_summary.physical_record_storage_byte_count;
    let summary = ReportSummary {
        table_count: tables.len(),
        directory_bound_table_count: tables
            .iter()
            .filter(|table| table.directory_binding.is_some())
            .count(),
        separate_consumer_bound_table_count: tables
            .iter()
            .filter(|table| table.separate_consumer_binding.is_some())
            .count(),
        consumer_bound_table_count: tables
            .iter()
            .filter(|table| {
                table.directory_binding.is_some() || table.separate_consumer_binding.is_some()
            })
            .count(),
        unresolved_consumer_table_count: tables
            .iter()
            .filter(|table| {
                table.directory_binding.is_none() && table.separate_consumer_binding.is_none()
            })
            .count(),
        pointer_count: tables.iter().map(|table| table.pointer_count).sum(),
        unique_target_count: tables.iter().map(|table| table.unique_target_count).sum(),
        unique_script_entry_count: tables
            .iter()
            .map(|table| table.unique_script_entry_count)
            .sum(),
        handler_target_entry_count: tables
            .iter()
            .map(|table| table.handler_target_entry_count)
            .sum(),
        main_first_line_count,
        max_main_first_line_storage_byte_count,
        main_first_line_japanese_literal_byte_count,
        main_first_line_non_japanese_literal_byte_count,
        main_first_line_protected_original_alphanumeric_literal_byte_count,
        main_first_line_end_control_counts,
        main_linear_segment_count,
        main_linear_line_count,
        max_main_linear_segment_line_count,
        main_linear_segment_japanese_literal_byte_count,
        main_linear_segment_non_japanese_literal_byte_count,
        main_linear_segment_protected_original_alphanumeric_literal_byte_count,
        main_unique_japanese_literal_storage_byte_count: main_literal_storage_summary
            .unique_japanese_literal_storage_byte_count,
        main_unique_non_japanese_literal_storage_byte_count: main_literal_storage_summary
            .unique_non_japanese_literal_storage_byte_count,
        main_literal_kind_conflict_storage_byte_count: main_literal_storage_summary
            .literal_kind_conflict_storage_byte_count,
        main_literal_structural_conflict_storage_byte_count: main_literal_storage_summary
            .literal_structural_conflict_storage_byte_count,
        main_safe_japanese_translation_source_byte_count: main_literal_storage_summary
            .safe_japanese_translation_source_byte_count,
        main_translation_view_line_count,
        main_translation_view_safe_japanese_source_byte_count,
        main_transition_target_record_count,
        main_linear_segment_boundary_control_counts,
        main_linear_segment_transition_count,
        main_record_count,
        main_record_consumed_storage_byte_count: main_record_storage_summary
            .consumed_storage_byte_count,
        main_record_unique_storage_byte_count: main_record_storage_summary
            .unique_storage_byte_count,
        main_record_shared_storage_byte_count: main_record_storage_summary
            .shared_storage_byte_count,
        main_record_overlapping_pair_count: main_record_storage_summary
            .overlapping_record_pair_count,
        max_main_record_overlap_depth: main_record_storage_summary.max_overlap_depth,
        max_main_record_storage_byte_count: main_record_storage_summary.max_storage_byte_count,
        battle_pointer_referenced_record_count,
        battle_unreferenced_record_count,
        battle_pointer_referenced_storage_byte_count,
        battle_physical_record_storage_byte_count,
        alias_group_count: tables.iter().map(|table| table.alias_group_count).sum(),
        aliased_entry_count: tables.iter().map(|table| table.aliased_entry_count).sum(),
    };

    Ok(DialogueStructureReport {
        schema_version: 12,
        scope: ReportScope {
            source_sha1: EXPECTED_SOURCE_SHA1,
            translation_direction: "ja_to_ko",
            preserve_existing_english: true,
            proof_boundary: "exact pointer-table ranges, switchable-bank target mapping, aliases, all nine consumer roots, the selector-41 epilogue-routing use, direct-entry and E4/E6 transition-target prefix paths, every main entry's bounded consumed storage range and measured shared storage, the separate battle state machine and all EF-terminated battle record ranges, Japanese 00-5F and 84-8B literal classification with 60-83 Latin preservation, all explicit E4/E6 graph edges, the E7 caller-handoff contract, and eleven confirmed direct outer dispatch bindings; no dialogue bytes or translations are emitted",
        },
        summary,
        main_dialogue_state_machine,
        battle_dialogue_state_machine,
        main_dialogue_graph,
        tables,
        unknowns: vec![
            "All directory-bound script entries and all twenty-eight pointer-referenced battle records have bounded consumed storage ranges; main records may share bytes, while battle records are disjoint and one additional unreferenced structural record remains preserved but not admitted as a translation target.",
            "Battle record boundaries, required battle-route polarity, and the character-epilogue temporal variant union are proven; final Hangul page budgeting still requires the reviewed Korean glyph working set.",
            "Direct entries consume optional E5, a fixed four-byte header, and optional E8. E4/E6 transition targets instead consume only an optional E8 before decoding; all 139 target reparses preserve the previously bounded storage end, graph boundary, and graph destination.",
            "Eleven direct outer dispatch bindings reuse four observer handlers across twenty-two state slots; indirect bindings are not excluded, and bank 04:A20F has no confirmed direct dispatch binding.",
            "Ten of the eighteen main dialogue state handlers remain structurally named but semantically unresolved.",
            "Role labels began as external map candidates and do not prove every entry's gameplay context.",
            "Existing English and numeric content remains protected and is not a translation target.",
        ],
    })
}

fn control_usage_reports(
    counts: BTreeMap<u8, usize>,
    declared_order: &[u8],
) -> Vec<ControlUsageReport> {
    declared_order
        .iter()
        .filter_map(|code| {
            counts.get(code).map(|count| ControlUsageReport {
                code: *code,
                code_hex: format!("{code:02X}"),
                count: *count,
            })
        })
        .collect()
}
