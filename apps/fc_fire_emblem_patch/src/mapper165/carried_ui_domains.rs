use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    class_profile::{bind_installed_consumers, plan_class_profiles},
    font::{load_dalmoori, rasterize_glyph},
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE},
    front_end_menu::plan_front_end_menu,
    hangul_page_plan::assemble_hangul_page_pack,
    localization::OptionsLocalization,
    mmc5_chr::switchable_bank_file_offset,
    options::OPTIONS_TABLE_OFFSET,
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    roster_localization::{ROSTER_HEADER_CPU_ADDRESS, ROSTER_TEXT_PRG_BANK, RosterLocalization},
    sha1_hex,
    title_graphics::{
        TITLE_RUNTIME_COMPLETION_STREAM_BYTE_COUNT, TITLE_STREAM_BYTE_COUNT,
        bind_installed_title_consumer_route, plan_title_graphics, title_stream_file_offset,
    },
};

use super::{
    FIRST_EXTENSION_CHR_PAGE,
    battle_composition_loader_probe::{
        CUMULATIVE_RUNTIME_LAYOUT, cumulative_battle_central_right_fd_selector,
    },
    bind_front_end_font_page_selector, bind_unit_name_font_page_selector,
    build_front_end_font_page_forwarder, build_unit_name_font_page_forwarder,
    class_profile_page::{
        PROFILE_PAGE_SELECTOR_ADDRESS, TITLE_COMPOSER_HOOK_ADDRESS, build_profile_page_selector,
        build_title_composer_hook,
    },
    dialogue_probe_font::assignment_sha1,
    encode_chr_page_register,
    font_pair_projection::{WRITE_TRANSLATED_CHR_PAGE_ADDRESS, build_translated_chr_page_writer},
    front_end_page::PAGE_ROUTINE_ADDRESS as FRONT_END_PAGE_ROUTINE_ADDRESS,
    maximum_dialogue_runtime::{
        INITIAL_PAGE_SELECTOR_ADDRESS, bind_installed_initial_page_selector,
    },
    options_page::{
        PAGE_A_REGISTER as OPTIONS_PAGE_A_REGISTER, PAGE_B_REGISTER as OPTIONS_PAGE_B_REGISTER,
        PAGE_ROUTINE_ADDRESS as OPTIONS_PAGE_ROUTINE_ADDRESS,
        ROW_HOOK_ADDRESS as OPTIONS_ROW_HOOK_ADDRESS,
        ROW_OWNER_GATE_ADDRESS as OPTIONS_ROW_OWNER_GATE_ADDRESS,
        ROW_PRG_BANK as OPTIONS_ROW_PRG_BANK,
        build_page_routine_with_fallback as build_options_page_routine,
        build_row_owner_gate as build_options_row_owner_gate, row_hook as build_options_row_hook,
    },
    roster_page::{
        CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS, CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS,
        HEADER_CALL_ADDRESS as ROSTER_HEADER_CALL_ADDRESS,
        OWNER_CONSTRUCTOR_ADDRESS as ROSTER_OWNER_CONSTRUCTOR_ADDRESS,
        OWNER_CONSTRUCTOR_PRG_BANK as ROSTER_OWNER_CONSTRUCTOR_PRG_BANK,
        OWNER_CONSTRUCTOR_SIGNATURE as ROSTER_OWNER_CONSTRUCTOR_SIGNATURE,
        PAGE_REGISTERS as ROSTER_PAGE_REGISTERS,
        PAGE_ROUTINE_ADDRESS as ROSTER_PAGE_ROUTINE_ADDRESS,
        PHYSICAL_CHR_PAGES as ROSTER_PHYSICAL_CHR_PAGES,
        build_page_routine_with_fallback as build_roster_page_routine,
        central_right_fd_selector_call, central_right_fe_companion_fd_refresh_call,
    },
    shop_dialogue_page::{
        PAGE_ROUTINE_ADDRESS as SHOP_DIALOGUE_PAGE_ROUTINE_ADDRESS,
        SCREEN_ROLE as SHOP_DIALOGUE_SCREEN_ROLE,
        build_page_selector as build_shop_dialogue_page_selector,
    },
    unit_name_page::PAGE_ROUTINE_ADDRESS as UNIT_NAME_PAGE_ROUTINE_ADDRESS,
};

const CLASS_PROFILE_PRG_BANK: u8 = 0x0D;
const CLASS_PROFILE_PHYSICAL_PAGES: [u8; 2] = [46, 47];
const TITLE_ROUTE_PRG_BANK: u8 = 0x0D;
const TITLE_ROUTE_RANGES: [(u16, usize); 4] =
    [(0xAC45, 10), (0xAC56, 3), (0xAC82, 3), (0xA682, 0x62)];

pub(crate) struct CarriedUiDomainInputs<'a> {
    pub(crate) source: &'a Rom,
    pub(crate) cumulative: &'a Rom,
    pub(crate) integrated: &'a Rom,
    pub(crate) cumulative_report_path: &'a Path,
    pub(crate) options_localization_path: &'a Path,
    pub(crate) roster_localization_path: &'a Path,
    pub(crate) front_end_menu_localization_path: &'a Path,
    pub(crate) class_profile_localization_path: &'a Path,
    pub(crate) title_graphics_localization_path: &'a Path,
    pub(crate) title_logo_asset_path: &'a Path,
    pub(crate) final_roster_consumer_route: &'a FinalRosterConsumerRoute,
}

pub(crate) struct FinalRosterConsumerRoute {
    pub(crate) central_fallback_target: u16,
    pub(crate) regions: Vec<FinalConsumerRouteRegion>,
}

pub(crate) struct FinalConsumerRouteRegion {
    pub(crate) role: &'static str,
    pub(crate) cpu_address: u16,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CarriedUiDomainPreservation {
    strategy: &'static str,
    cumulative_candidate_sha1: String,
    cumulative_report_sha1: String,
    integrated_image_sha1: String,
    domain_count: usize,
    domains: Vec<CarriedUiDomain>,
    all_translation_inputs_rebound: bool,
    all_storage_regions_rebound: bool,
    all_font_regions_rebound: bool,
    all_consumer_routes_rebound: bool,
    human_review_complete: bool,
    complete: bool,
}

impl CarriedUiDomainPreservation {
    pub(crate) fn complete(&self) -> bool {
        self.complete
    }

    pub(crate) fn human_review_complete(&self) -> bool {
        self.human_review_complete
    }
}

#[derive(Debug, Serialize)]
struct CarriedUiDomain {
    id: &'static str,
    target_unit_count: usize,
    screen_roles: Vec<&'static str>,
    translation_input_bound: bool,
    review_complete: bool,
    storage_regions: Vec<FinalRegionBinding>,
    font_regions: Vec<FinalRegionBinding>,
    consumer_regions: Vec<FinalRegionBinding>,
    consumer_route_binding_ids: Vec<&'static str>,
    complete_for_declared_domain_plan: bool,
}

#[derive(Debug, Serialize)]
struct FinalRegionBinding {
    role: &'static str,
    binding_kind: &'static str,
    file_offset_hex: String,
    byte_count: usize,
    sha1: String,
    final_bytes_match_binding: bool,
}

#[derive(Debug, Deserialize)]
struct CumulativeReport {
    schema: u8,
    source_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    options_menu: OptionsReport,
    front_end_menu: FrontEndReport,
    playable_unit_names: UnitNameReport,
    automatic_class_profiles: ClassProfileReport,
    title_logo: TitleReport,
    main_dialogue: DialogueReport,
}

#[derive(Debug, Deserialize)]
struct DialogueReport {
    lifetimes: Vec<DialogueLifetimeReport>,
    maximum_page_reloaded_lifetime: MaximumDialogueReport,
}

#[derive(Debug, Deserialize)]
struct DialogueLifetimeReport {
    screen_role: String,
    font_mapper_register: u8,
}

#[derive(Debug, Deserialize)]
struct MaximumDialogueReport {
    initial_selector_byte_count: usize,
}

#[derive(Debug, Deserialize)]
struct OptionsReport {
    installed_entry_count: usize,
    target_glyph_count: usize,
    visible_active_code_count: usize,
    preserved_active_code_count: usize,
    total_slot_demand: usize,
    capacity_bound_to_build: bool,
}

#[derive(Debug, Deserialize)]
struct FrontEndReport {
    workspace_sha1: String,
    workspace_entry_count: usize,
    installed_entry_count: usize,
    installed_source_storage_byte_count: usize,
    installed_output_storage_byte_count: usize,
    unique_glyph_count: usize,
    glyph_assignment_sha1: String,
    font_physical_page: u8,
    font_mapper_register: u8,
    font_page_sha1: String,
    font_page_pack_sha1: String,
    central_fe_companion_refresh_routed: bool,
    no_save_source_lifetime_bound: bool,
    save_slot_selection_source_lifetime_bound: bool,
}

#[derive(Debug, Deserialize)]
struct UnitNameReport {
    roster_page_pack_sha1: String,
    unit_ui_font_mapper_register: u8,
    roster_projection_installed: bool,
    roster_capacity_bound_to_build: bool,
}

#[derive(Debug, Deserialize)]
struct ClassProfileReport {
    workspace_sha1: String,
    workspace_entry_count: usize,
    installed_entry_count: usize,
    installed_description_line_count: usize,
    installed_source_storage_byte_count: usize,
    installed_output_storage_byte_count: usize,
    total_unique_glyph_count: usize,
    page_unique_glyph_counts: [usize; 2],
    glyph_assignment_sha1s: [String; 2],
    font_physical_pages: [u8; 2],
    font_mapper_registers: [u8; 2],
    font_page_sha1s: [String; 2],
    font_page_pack_sha1: String,
    original_english_digits_and_ui_preserved: bool,
    profile_index_page_selector_installed: bool,
}

#[derive(Debug, Deserialize)]
struct TitleReport {
    workspace_sha1: String,
    asset_sha1: String,
    installed_unique_tile_count: usize,
    installed_tilemap_cell_count: usize,
    physical_chr_page: u8,
    installed_chr_page_sha1: String,
    installed_stream_sha1: String,
    installed_runtime_completion_stream_sha1: String,
    preserved_title_stream_bytes_unchanged: bool,
    preserved_runtime_completion_control_bytes_unchanged: bool,
    unassigned_title_chr_patterns_unchanged: bool,
    source_sword_sprite_tm_and_copyright_assets_unchanged: bool,
}

pub(crate) fn inspect_carried_ui_domains(
    inputs: CarriedUiDomainInputs<'_>,
) -> Result<CarriedUiDomainPreservation> {
    inputs.source.verify_supported_japanese()?;
    ensure!(
        inputs.cumulative.mapper() == 165
            && inputs.integrated.mapper() == 165
            && inputs.cumulative.prg().len() == inputs.integrated.prg().len()
            && inputs.cumulative.chr().len() <= inputs.integrated.chr().len(),
        "carried UI domain artifacts do not share the mapper-165 cumulative layout"
    );
    let report_bytes = fs::read(inputs.cumulative_report_path)
        .with_context(|| format!("read {}", inputs.cumulative_report_path.display()))?;
    let report: CumulativeReport = serde_json::from_slice(&report_bytes)
        .with_context(|| format!("parse {}", inputs.cumulative_report_path.display()))?;
    ensure!(
        report.schema == super::cumulative_patch::REPORT_SCHEMA
            && report.source_sha1 == EXPECTED_SOURCE_SHA1
            && report.output_sha1 == sha1_hex(inputs.cumulative.data())
            && report.output_mapper == inputs.cumulative.mapper()
            && report.prg_size == inputs.cumulative.prg().len()
            && report.chr_size == inputs.cumulative.chr().len(),
        "carried UI domain report does not describe the exact cumulative candidate"
    );

    let domains = vec![
        inspect_options(&inputs, &report)?,
        inspect_roster(&inputs, &report)?,
        inspect_front_end(&inputs, &report)?,
        inspect_class_profiles(&inputs, &report)?,
        inspect_title(&inputs, &report)?,
    ];
    ensure!(
        domains
            .iter()
            .map(|domain| domain.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == domains.len()
            && domains
                .iter()
                .all(|domain| domain.complete_for_declared_domain_plan),
        "carried UI domain preservation is incomplete"
    );

    let human_review_complete = domains.iter().all(|domain| domain.review_complete);
    Ok(CarriedUiDomainPreservation {
        strategy: "recompute each carried translation payload from its source-bound input, then bind its exact final storage, font supply, and consumer route without inheriting cumulative runtime evidence",
        cumulative_candidate_sha1: report.output_sha1,
        cumulative_report_sha1: sha1_hex(&report_bytes),
        integrated_image_sha1: sha1_hex(inputs.integrated.data()),
        domain_count: domains.len(),
        domains,
        all_translation_inputs_rebound: true,
        all_storage_regions_rebound: true,
        all_font_regions_rebound: true,
        all_consumer_routes_rebound: true,
        human_review_complete,
        complete: true,
    })
}

fn inspect_options(
    inputs: &CarriedUiDomainInputs<'_>,
    report: &CumulativeReport,
) -> Result<CarriedUiDomain> {
    let localization = OptionsLocalization::from_path(inputs.options_localization_path)?;
    let validated = localization.validate()?;
    let expected_page_pack = assemble_hangul_page_pack(inputs.source, &localization)?;
    ensure!(
        report.options_menu.installed_entry_count == 3
            && report.options_menu.target_glyph_count == validated.tiles.len()
            && report.options_menu.visible_active_code_count
                == report.options_menu.total_slot_demand
            && report.options_menu.total_slot_demand
                == report.options_menu.target_glyph_count
                    + report.options_menu.preserved_active_code_count
            && report.options_menu.capacity_bound_to_build,
        "cumulative options report lost its installed capacity contract"
    );
    let storage_regions = vec![bind_expected_region(
        "options_label_storage",
        OPTIONS_TABLE_OFFSET,
        &validated.replacement_table,
        inputs.cumulative,
        inputs.integrated,
    )?];
    let font_regions = vec![bind_expected_region(
        "options_font_page_pair",
        chr_file_offset(inputs.cumulative, FIRST_EXTENSION_CHR_PAGE)?,
        &expected_page_pack,
        inputs.cumulative,
        inputs.integrated,
    )?];
    let options_routine = build_options_page_routine(
        OPTIONS_PAGE_A_REGISTER,
        OPTIONS_PAGE_B_REGISTER,
        ROSTER_PAGE_ROUTINE_ADDRESS,
    )?;
    let consumer_regions = vec![
        bind_expected_region(
            "options_page_selector",
            active_fixed_file_offset(inputs.cumulative, OPTIONS_PAGE_ROUTINE_ADDRESS)?,
            &options_routine,
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_expected_region(
            "options_row_owner_gate",
            active_fixed_file_offset(inputs.cumulative, OPTIONS_ROW_OWNER_GATE_ADDRESS)?,
            &build_options_row_owner_gate()?,
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_expected_region(
            "options_row_hook",
            switchable_bank_file_offset(OPTIONS_ROW_PRG_BANK, OPTIONS_ROW_HOOK_ADDRESS)?,
            &build_options_row_hook()?,
            inputs.cumulative,
            inputs.integrated,
        )?,
    ];
    Ok(complete_domain(
        "options_labels",
        3,
        vec!["options"],
        validated.review_complete,
        storage_regions,
        font_regions,
        consumer_regions,
        vec![
            "0B:93B7:options_row_hook",
            "0F:FB68:options_row_owner_gate",
            "0F:FB20:options_page_selector",
        ],
    ))
}

fn inspect_roster(
    inputs: &CarriedUiDomainInputs<'_>,
    report: &CumulativeReport,
) -> Result<CarriedUiDomain> {
    let localization =
        RosterLocalization::from_path(inputs.roster_localization_path)?.validate()?;
    ensure!(
        report.playable_unit_names.roster_projection_installed
            && report.playable_unit_names.roster_capacity_bound_to_build,
        "cumulative roster report lost its installed page contract"
    );
    let storage_regions = vec![bind_expected_region(
        "roster_header_storage",
        switchable_bank_file_offset(ROSTER_TEXT_PRG_BANK, ROSTER_HEADER_CPU_ADDRESS)?,
        &localization.replacement_header,
        inputs.cumulative,
        inputs.integrated,
    )?];
    let page_pack_offset = chr_file_offset(inputs.cumulative, ROSTER_PHYSICAL_CHR_PAGES[0])?;
    let page_pack = inputs
        .integrated
        .data()
        .get(page_pack_offset..page_pack_offset + 2 * FONT_PAGE_SIZE)
        .context("roster page pair is outside the integrated artifact")?;
    ensure!(
        sha1_hex(page_pack) == report.playable_unit_names.roster_page_pack_sha1,
        "integrated roster page pair no longer matches the cumulative report"
    );
    for page in page_pack.chunks_exact(FONT_PAGE_SIZE) {
        for (code, expected) in &localization.tiles {
            let start = usize::from(*code) * FONT_TILE_SIZE;
            ensure!(
                page[start..start + FONT_TILE_SIZE] == *expected,
                "integrated roster header glyph {code:02X} changed"
            );
        }
    }
    let font_regions = vec![bind_preserved_region(
        "roster_font_page_pair",
        page_pack_offset,
        2 * FONT_PAGE_SIZE,
        inputs.cumulative,
        inputs.integrated,
        Some(&report.playable_unit_names.roster_page_pack_sha1),
    )?];
    let header_call_offset = switchable_bank_file_offset(
        ROSTER_OWNER_CONSTRUCTOR_PRG_BANK,
        ROSTER_HEADER_CALL_ADDRESS,
    )?;
    let header_call = inputs
        .source
        .data()
        .get(header_call_offset..header_call_offset + 5)
        .context("roster header call is outside the source")?;
    let central_selector_address = CUMULATIVE_RUNTIME_LAYOUT.battle_central_right_fd_selector;
    let central_selector =
        cumulative_battle_central_right_fd_selector(INITIAL_PAGE_SELECTOR_ADDRESS)?;
    let central_selector_offset =
        active_fixed_file_offset(inputs.cumulative, central_selector_address)?;
    ensure!(
        inputs
            .cumulative
            .data()
            .get(central_selector_offset..central_selector_offset + central_selector.len())
            .context("cumulative battle central selector is outside the image")?
            == central_selector,
        "cumulative battle central selector no longer reaches the maximum-dialogue selector"
    );
    let initial_selector_offset =
        active_fixed_file_offset(inputs.cumulative, INITIAL_PAGE_SELECTOR_ADDRESS)?;
    let initial_selector_byte_count = report
        .main_dialogue
        .maximum_page_reloaded_lifetime
        .initial_selector_byte_count;
    let installed_initial_selector = inputs
        .cumulative
        .data()
        .get(initial_selector_offset..initial_selector_offset + initial_selector_byte_count)
        .context("maximum-dialogue initial selector is outside the cumulative image")?;
    bind_installed_initial_page_selector(installed_initial_selector, ROSTER_PAGE_ROUTINE_ADDRESS)?;
    let final_central_selector = cumulative_battle_central_right_fd_selector(
        inputs.final_roster_consumer_route.central_fallback_target,
    )?;
    let mut consumer_regions = vec![
        bind_expected_region(
            "roster_page_selector",
            active_fixed_file_offset(inputs.cumulative, ROSTER_PAGE_ROUTINE_ADDRESS)?,
            &build_roster_page_routine(
                ROSTER_PAGE_REGISTERS[0],
                ROSTER_PAGE_REGISTERS[1],
                UNIT_NAME_PAGE_ROUTINE_ADDRESS,
            )?,
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_expected_region(
            "roster_owner_constructor",
            switchable_bank_file_offset(
                ROSTER_OWNER_CONSTRUCTOR_PRG_BANK,
                ROSTER_OWNER_CONSTRUCTOR_ADDRESS,
            )?,
            &ROSTER_OWNER_CONSTRUCTOR_SIGNATURE,
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_expected_region(
            "roster_header_resource_call",
            header_call_offset,
            header_call,
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_expected_region(
            "central_roster_selector_call",
            active_fixed_file_offset(inputs.cumulative, CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS)?,
            &central_right_fd_selector_call(central_selector_address)?,
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_expected_region(
            "central_roster_companion_refresh_call",
            active_fixed_file_offset(
                inputs.cumulative,
                CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS,
            )?,
            &central_right_fe_companion_fd_refresh_call(central_selector_address)?,
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_final_expected_region(
            "battle_central_roster_fallback_selector",
            central_selector_offset,
            &final_central_selector,
            inputs.integrated,
        )?,
    ];
    for region in &inputs.final_roster_consumer_route.regions {
        consumer_regions.push(bind_final_expected_region(
            region.role,
            active_fixed_file_offset(inputs.integrated, region.cpu_address)?,
            &region.bytes,
            inputs.integrated,
        )?);
    }
    Ok(complete_domain(
        "roster_header",
        1,
        vec!["unit_roster"],
        localization.review_complete,
        storage_regions,
        font_regions,
        consumer_regions,
        vec![
            "0B:89DB:roster_owner_constructor",
            "0B:89F2:roster_header_resource_call",
            "0F:C9C2:roster_page_selector_entry",
            "0F:FABB:roster_companion_refresh_entry",
            "0F:FF1D:integrated_battle_central_roster_fallback_selector",
            "0F:F558:integrated_font_page_route_selector",
            "0F:F5A3:integrated_dialogue_chr_selector",
            "0F:FB80:roster_page_selector",
        ],
    ))
}

fn inspect_front_end(
    inputs: &CarriedUiDomainInputs<'_>,
    report: &CumulativeReport,
) -> Result<CarriedUiDomain> {
    let plan = plan_front_end_menu(inputs.source, inputs.front_end_menu_localization_path)?;
    let assignments = plan.bind_installed_glyph_codes(inputs.integrated.data())?;
    ensure!(
        report.front_end_menu.workspace_sha1 == plan.workspace_sha1
            && report.front_end_menu.workspace_entry_count == plan.entries.len()
            && report.front_end_menu.installed_entry_count == plan.entries.len()
            && report.front_end_menu.installed_source_storage_byte_count
                == plan
                    .entries
                    .iter()
                    .map(|entry| entry.source_storage_byte_count)
                    .sum::<usize>()
            && report.front_end_menu.installed_output_storage_byte_count
                == report.front_end_menu.installed_source_storage_byte_count
            && report.front_end_menu.unique_glyph_count == assignments.len()
            && report.front_end_menu.glyph_assignment_sha1 == assignment_sha1(&assignments)
            && report.front_end_menu.central_fe_companion_refresh_routed
            && report.front_end_menu.no_save_source_lifetime_bound
            && report
                .front_end_menu
                .save_slot_selection_source_lifetime_bound,
        "cumulative front-end report no longer matches its translation input"
    );
    let storage_regions = plan
        .entries
        .iter()
        .map(|entry| {
            bind_preserved_region(
                "front_end_label_storage",
                entry.file_offset,
                entry.source_storage_byte_count,
                inputs.cumulative,
                inputs.integrated,
                None,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let page_pack_offset =
        chr_file_offset(inputs.cumulative, report.front_end_menu.font_physical_page)?;
    let page_pack = inputs
        .integrated
        .data()
        .get(page_pack_offset..page_pack_offset + 2 * FONT_PAGE_SIZE)
        .context("front-end page pair is outside the integrated artifact")?;
    ensure!(
        report.front_end_menu.font_physical_page == 42
            && encode_chr_page_register(report.front_end_menu.font_physical_page)?
                == report.front_end_menu.font_mapper_register
            && report.front_end_menu.font_page_sha1 == sha1_hex(&page_pack[..FONT_PAGE_SIZE])
            && report.front_end_menu.font_page_pack_sha1 == sha1_hex(page_pack),
        "integrated front-end font page no longer matches the cumulative report"
    );
    verify_glyph_tiles(&page_pack[..FONT_PAGE_SIZE], &assignments)?;
    let font_regions = vec![bind_preserved_region(
        "front_end_font_page_pair",
        page_pack_offset,
        2 * FONT_PAGE_SIZE,
        inputs.cumulative,
        inputs.integrated,
        Some(&report.front_end_menu.font_page_pack_sha1),
    )?];
    let installed_selector = bind_front_end_font_page_selector(inputs.cumulative)?;
    ensure!(
        installed_selector.mapper_register == report.front_end_menu.font_mapper_register,
        "cumulative front-end selector and reported page disagree"
    );
    let final_forwarder = build_front_end_font_page_forwarder(&installed_selector)?;
    let installed_unit_selector = bind_unit_name_font_page_selector(inputs.cumulative)?;
    ensure!(
        installed_unit_selector.mapper_register
            == report.playable_unit_names.unit_ui_font_mapper_register,
        "cumulative unit-name selector and reported page disagree"
    );
    let final_unit_forwarder = build_unit_name_font_page_forwarder(&installed_unit_selector)?;
    let shop_dialogue_mapper_register = report
        .main_dialogue
        .lifetimes
        .iter()
        .find(|lifetime| lifetime.screen_role == SHOP_DIALOGUE_SCREEN_ROLE)
        .map(|lifetime| lifetime.font_mapper_register)
        .context("cumulative report lost the weapon-shop dialogue lifetime")?;
    let consumer_regions = vec![
        bind_replaced_region(
            "front_end_page_forwarder",
            active_fixed_file_offset(inputs.cumulative, FRONT_END_PAGE_ROUTINE_ADDRESS)?,
            &installed_selector.expected_bytes,
            &final_forwarder,
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_replaced_region(
            "front_end_upstream_unit_name_forwarder",
            active_fixed_file_offset(inputs.cumulative, UNIT_NAME_PAGE_ROUTINE_ADDRESS)?,
            &installed_unit_selector.expected_bytes,
            &final_unit_forwarder,
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_expected_region(
            "front_end_upstream_shop_dialogue_selector",
            active_fixed_file_offset(inputs.cumulative, SHOP_DIALOGUE_PAGE_ROUTINE_ADDRESS)?,
            &build_shop_dialogue_page_selector(
                shop_dialogue_mapper_register,
                FRONT_END_PAGE_ROUTINE_ADDRESS,
            )?,
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_expected_region(
            "front_end_translated_page_writer",
            active_fixed_file_offset(inputs.cumulative, WRITE_TRANSLATED_CHR_PAGE_ADDRESS)?,
            &build_translated_chr_page_writer()?,
            inputs.cumulative,
            inputs.integrated,
        )?,
    ];
    Ok(complete_domain(
        "front_end_menu_labels",
        7,
        vec!["new_game_choice", "save_slot_selection"],
        plan.review_complete,
        storage_regions,
        font_regions,
        consumer_regions,
        vec![
            "0F:F700:unit_name_page_forwarder_to_shop_dialogue",
            "0F:F748:shop_dialogue_page_fallback_to_front_end",
            "0F:FC60:front_end_page_forwarder",
            "0F:F386:translated_chr_page_writer",
        ],
    ))
}

fn inspect_class_profiles(
    inputs: &CarriedUiDomainInputs<'_>,
    report: &CumulativeReport,
) -> Result<CarriedUiDomain> {
    let plan = plan_class_profiles(inputs.source, inputs.class_profile_localization_path)?;
    let assignments = plan.bind_installed_glyph_codes(inputs.integrated.data())?;
    ensure!(
        report.automatic_class_profiles.workspace_sha1 == plan.workspace_sha1
            && report.automatic_class_profiles.workspace_entry_count == plan.entries.len()
            && report.automatic_class_profiles.installed_entry_count == plan.entries.len()
            && report
                .automatic_class_profiles
                .installed_description_line_count
                == plan.description_line_count()
            && report
                .automatic_class_profiles
                .installed_source_storage_byte_count
                == plan
                    .entries
                    .iter()
                    .map(|entry| {
                        entry.title_source_storage_byte_count
                            + entry.description_source_storage_byte_count
                    })
                    .sum::<usize>()
            && report
                .automatic_class_profiles
                .installed_output_storage_byte_count
                == report
                    .automatic_class_profiles
                    .installed_source_storage_byte_count
            && report.automatic_class_profiles.total_unique_glyph_count
                == plan.unique_glyphs().len()
            && report.automatic_class_profiles.page_unique_glyph_counts
                == [assignments[0].len(), assignments[1].len()]
            && report.automatic_class_profiles.glyph_assignment_sha1s
                == [
                    assignment_sha1(&assignments[0]),
                    assignment_sha1(&assignments[1])
                ]
            && report.automatic_class_profiles.font_physical_pages == CLASS_PROFILE_PHYSICAL_PAGES
            && report
                .automatic_class_profiles
                .original_english_digits_and_ui_preserved
            && report
                .automatic_class_profiles
                .profile_index_page_selector_installed,
        "cumulative class-profile report no longer matches its translation input"
    );
    let storage_regions = plan
        .entries
        .iter()
        .flat_map(|entry| {
            [
                (
                    "class_profile_title_storage",
                    entry.title_file_offset,
                    entry.title_source_storage_byte_count,
                ),
                (
                    "class_profile_description_storage",
                    entry.description_file_offset,
                    entry.description_source_storage_byte_count,
                ),
            ]
        })
        .map(|(role, offset, byte_count)| {
            bind_preserved_region(
                role,
                offset,
                byte_count,
                inputs.cumulative,
                inputs.integrated,
                None,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let page_pack_offset = chr_file_offset(inputs.cumulative, CLASS_PROFILE_PHYSICAL_PAGES[0])?;
    let page_pack = inputs
        .integrated
        .data()
        .get(page_pack_offset..page_pack_offset + 2 * FONT_PAGE_SIZE)
        .context("class-profile page pair is outside the integrated artifact")?;
    ensure!(
        report.automatic_class_profiles.font_page_pack_sha1 == sha1_hex(page_pack)
            && report.automatic_class_profiles.font_page_sha1s
                == [
                    sha1_hex(&page_pack[..FONT_PAGE_SIZE]),
                    sha1_hex(&page_pack[FONT_PAGE_SIZE..]),
                ]
            && report.automatic_class_profiles.font_mapper_registers
                == [
                    encode_chr_page_register(CLASS_PROFILE_PHYSICAL_PAGES[0])?,
                    encode_chr_page_register(CLASS_PROFILE_PHYSICAL_PAGES[1])?,
                ],
        "integrated class-profile font pages no longer match the cumulative report"
    );
    verify_glyph_tiles(&page_pack[..FONT_PAGE_SIZE], &assignments[0])?;
    verify_glyph_tiles(&page_pack[FONT_PAGE_SIZE..], &assignments[1])?;
    let font_regions = vec![bind_preserved_region(
        "class_profile_font_page_pair",
        page_pack_offset,
        2 * FONT_PAGE_SIZE,
        inputs.cumulative,
        inputs.integrated,
        Some(&report.automatic_class_profiles.font_page_pack_sha1),
    )?];
    let mut route_ids = bind_installed_consumers(inputs.integrated)?;
    route_ids.extend([
        "0D:82ED:class_profile_title_page_hook",
        "0D:BE3C:class_profile_page_selector",
    ]);
    let consumer_regions = vec![
        bind_expected_region(
            "class_profile_page_selector",
            switchable_bank_file_offset(CLASS_PROFILE_PRG_BANK, PROFILE_PAGE_SELECTOR_ADDRESS)?,
            &build_profile_page_selector(report.automatic_class_profiles.font_mapper_registers)?,
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_expected_region(
            "class_profile_title_page_hook",
            switchable_bank_file_offset(CLASS_PROFILE_PRG_BANK, TITLE_COMPOSER_HOOK_ADDRESS)?,
            &build_title_composer_hook()?,
            inputs.cumulative,
            inputs.integrated,
        )?,
    ];
    Ok(complete_domain(
        "class_profiles",
        22,
        vec!["class_profile"],
        plan.review_complete,
        storage_regions,
        font_regions,
        consumer_regions,
        route_ids,
    ))
}

fn inspect_title(
    inputs: &CarriedUiDomainInputs<'_>,
    report: &CumulativeReport,
) -> Result<CarriedUiDomain> {
    let plan = plan_title_graphics(inputs.source, inputs.title_graphics_localization_path)?;
    let asset_bytes = fs::read(inputs.title_logo_asset_path)
        .with_context(|| format!("read {}", inputs.title_logo_asset_path.display()))?;
    ensure!(
        plan.translated_surface_count == 1
            && report.title_logo.workspace_sha1 == plan.workspace_sha1
            && report.title_logo.asset_sha1 == sha1_hex(&asset_bytes)
            && report.title_logo.installed_unique_tile_count > 0
            && report.title_logo.installed_tilemap_cell_count
                >= report.title_logo.installed_unique_tile_count
            && report.title_logo.preserved_title_stream_bytes_unchanged
            && report
                .title_logo
                .preserved_runtime_completion_control_bytes_unchanged
            && report.title_logo.unassigned_title_chr_patterns_unchanged
            && report
                .title_logo
                .source_sword_sprite_tm_and_copyright_assets_unchanged,
        "cumulative title report no longer matches its source-bound inputs"
    );
    let stream_offset = title_stream_file_offset();
    let storage_regions = vec![
        bind_preserved_region(
            "title_logo_stream",
            stream_offset,
            TITLE_STREAM_BYTE_COUNT,
            inputs.cumulative,
            inputs.integrated,
            Some(&report.title_logo.installed_stream_sha1),
        )?,
        bind_preserved_region(
            "title_logo_runtime_completion_stream",
            stream_offset + TITLE_STREAM_BYTE_COUNT,
            TITLE_RUNTIME_COMPLETION_STREAM_BYTE_COUNT,
            inputs.cumulative,
            inputs.integrated,
            Some(&report.title_logo.installed_runtime_completion_stream_sha1),
        )?,
    ];
    let font_regions = vec![bind_preserved_region(
        "title_logo_chr_page",
        chr_file_offset(inputs.cumulative, report.title_logo.physical_chr_page)?,
        FONT_PAGE_SIZE,
        inputs.cumulative,
        inputs.integrated,
        Some(&report.title_logo.installed_chr_page_sha1),
    )?];
    let route_ids = bind_installed_title_consumer_route(inputs.source, inputs.integrated)?;
    let consumer_regions = TITLE_ROUTE_RANGES
        .iter()
        .map(|(address, byte_count)| {
            let offset = switchable_bank_file_offset(TITLE_ROUTE_PRG_BANK, *address)?;
            let expected = inputs
                .source
                .data()
                .get(offset..offset + *byte_count)
                .context("title consumer route is outside the source")?;
            bind_expected_region(
                "title_logo_consumer_route",
                offset,
                expected,
                inputs.cumulative,
                inputs.integrated,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(complete_domain(
        "title_graphics",
        1,
        vec!["title"],
        plan.review_complete,
        storage_regions,
        font_regions,
        consumer_regions,
        route_ids,
    ))
}

fn complete_domain(
    id: &'static str,
    target_unit_count: usize,
    screen_roles: Vec<&'static str>,
    review_complete: bool,
    storage_regions: Vec<FinalRegionBinding>,
    font_regions: Vec<FinalRegionBinding>,
    consumer_regions: Vec<FinalRegionBinding>,
    consumer_route_binding_ids: Vec<&'static str>,
) -> CarriedUiDomain {
    CarriedUiDomain {
        id,
        target_unit_count,
        screen_roles,
        translation_input_bound: true,
        review_complete,
        complete_for_declared_domain_plan: target_unit_count > 0
            && !storage_regions.is_empty()
            && !font_regions.is_empty()
            && !consumer_regions.is_empty()
            && !consumer_route_binding_ids.is_empty(),
        storage_regions,
        font_regions,
        consumer_regions,
        consumer_route_binding_ids,
    }
}

fn bind_expected_region(
    role: &'static str,
    offset: usize,
    expected: &[u8],
    cumulative: &Rom,
    integrated: &Rom,
) -> Result<FinalRegionBinding> {
    let expected_sha1 = sha1_hex(expected);
    let binding = bind_preserved_region(
        role,
        offset,
        expected.len(),
        cumulative,
        integrated,
        Some(&expected_sha1),
    )?;
    ensure!(
        integrated.data()[offset..offset + expected.len()] == *expected,
        "integrated {role} does not match its recomputed bytes"
    );
    Ok(binding)
}

fn bind_final_expected_region(
    role: &'static str,
    offset: usize,
    expected: &[u8],
    integrated: &Rom,
) -> Result<FinalRegionBinding> {
    ensure!(!expected.is_empty(), "{role} final route region is empty");
    let actual = integrated
        .data()
        .get(offset..offset + expected.len())
        .with_context(|| format!("integrated {role} is outside the artifact"))?;
    ensure!(
        actual == expected,
        "integrated {role} does not match its generated final route"
    );
    Ok(FinalRegionBinding {
        role,
        binding_kind: "integrated_route_replacement",
        file_offset_hex: format!("0x{offset:06X}"),
        byte_count: expected.len(),
        sha1: sha1_hex(actual),
        final_bytes_match_binding: true,
    })
}

fn bind_replaced_region(
    role: &'static str,
    offset: usize,
    expected_before: &[u8],
    expected_after: &[u8],
    cumulative: &Rom,
    integrated: &Rom,
) -> Result<FinalRegionBinding> {
    ensure!(
        !expected_before.is_empty() && expected_before.len() == expected_after.len(),
        "{role} replacement extent changed"
    );
    let before = cumulative
        .data()
        .get(offset..offset + expected_before.len())
        .with_context(|| format!("cumulative {role} is outside the artifact"))?;
    let after = integrated
        .data()
        .get(offset..offset + expected_after.len())
        .with_context(|| format!("integrated {role} is outside the artifact"))?;
    ensure!(
        before == expected_before && after == expected_after && before != after,
        "integrated {role} does not replace its exact cumulative source"
    );
    Ok(FinalRegionBinding {
        role,
        binding_kind: "integrated_route_replacement",
        file_offset_hex: format!("0x{offset:06X}"),
        byte_count: expected_after.len(),
        sha1: sha1_hex(after),
        final_bytes_match_binding: true,
    })
}

fn bind_preserved_region(
    role: &'static str,
    offset: usize,
    byte_count: usize,
    cumulative: &Rom,
    integrated: &Rom,
    expected_sha1: Option<&str>,
) -> Result<FinalRegionBinding> {
    ensure!(byte_count > 0, "{role} region is empty");
    let before = cumulative
        .data()
        .get(offset..offset + byte_count)
        .with_context(|| format!("cumulative {role} is outside the artifact"))?;
    let after = integrated
        .data()
        .get(offset..offset + byte_count)
        .with_context(|| format!("integrated {role} is outside the artifact"))?;
    ensure!(
        before == after,
        "integrated {role} changed after cumulative installation"
    );
    let digest = sha1_hex(after);
    if let Some(expected) = expected_sha1 {
        ensure!(digest == expected, "integrated {role} digest changed");
    }
    Ok(FinalRegionBinding {
        role,
        binding_kind: "cumulative_bytes_preserved",
        file_offset_hex: format!("0x{offset:06X}"),
        byte_count,
        sha1: digest,
        final_bytes_match_binding: true,
    })
}

fn verify_glyph_tiles(page: &[u8], assignments: &BTreeMap<char, u8>) -> Result<()> {
    ensure!(
        page.len() == FONT_PAGE_SIZE,
        "installed glyph page size changed"
    );
    let font = load_dalmoori()?;
    for (glyph, code) in assignments {
        let start = usize::from(*code) * FONT_TILE_SIZE;
        let expected = rasterize_glyph(&font, *glyph)?;
        ensure!(
            page[start..start + FONT_TILE_SIZE] == expected,
            "installed glyph {glyph:?} tile at {code:02X} changed"
        );
    }
    Ok(())
}

fn chr_file_offset(rom: &Rom, physical_page: u8) -> Result<usize> {
    let offset = HEADER_SIZE + rom.prg().len() + usize::from(physical_page) * FONT_PAGE_SIZE;
    ensure!(
        offset + FONT_PAGE_SIZE <= rom.data().len(),
        "physical CHR page {physical_page} is outside the cumulative artifact"
    );
    Ok(offset)
}

fn active_fixed_file_offset(rom: &Rom, cpu_address: u16) -> Result<usize> {
    ensure!(
        rom.prg().len() >= 0x4000 && (0xC000..=0xFFFF).contains(&cpu_address),
        "active fixed-bank address is outside the mapper CPU window"
    );
    Ok(HEADER_SIZE + rom.prg().len() - 0x4000 + usize::from(cpu_address - 0xC000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserved_region_rejects_a_later_integration_mutation() {
        let mut base = vec![0_u8; HEADER_SIZE + 0x4000 + 2 * FONT_PAGE_SIZE];
        base[..4].copy_from_slice(b"NES\x1A");
        base[4] = 1;
        base[5] = 1;
        let cumulative = Rom::parse(base.clone()).unwrap();
        base[HEADER_SIZE + 4] = 1;
        let integrated = Rom::parse(base).unwrap();

        assert!(
            bind_preserved_region("test", HEADER_SIZE, 8, &cumulative, &integrated, None,)
                .unwrap_err()
                .to_string()
                .contains("changed after cumulative installation")
        );
    }

    #[test]
    fn glyph_binding_rejects_a_wrong_final_tile() {
        let mut page = vec![0_u8; FONT_PAGE_SIZE];
        let assignments = BTreeMap::from([('한', 0x01)]);
        let font = load_dalmoori().unwrap();
        page[FONT_TILE_SIZE..2 * FONT_TILE_SIZE]
            .copy_from_slice(&rasterize_glyph(&font, '한').unwrap());
        verify_glyph_tiles(&page, &assignments).unwrap();
        page[FONT_TILE_SIZE] ^= 1;
        assert!(verify_glyph_tiles(&page, &assignments).is_err());
    }
}
