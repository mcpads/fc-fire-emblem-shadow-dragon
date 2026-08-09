use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    rom::{Rom, EXPECTED_SOURCE_SHA1},
    sha1_hex,
};

const REGISTRY_JSON: &str = include_str!("../../../assets/structure/screen-contracts.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PatternWindow {
    Left,
    Right,
}

impl PatternWindow {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Left => "ppu_0000",
            Self::Right => "ppu_1000",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ObservedChrPair {
    pub(crate) screen_role: &'static str,
    pub(crate) pattern_window: PatternWindow,
    pub(crate) fd_source_page: u8,
    pub(crate) fe_source_page: u8,
}

impl ObservedChrPair {
    pub(crate) const fn new(
        screen_role: &'static str,
        pattern_window: PatternWindow,
        fd_source_page: u8,
        fe_source_page: u8,
    ) -> Self {
        Self {
            screen_role,
            pattern_window,
            fd_source_page,
            fe_source_page,
        }
    }
}

pub(crate) const OBSERVED_CHR_PAIRS: &[ObservedChrPair] = &[
    pair("title", PatternWindow::Left, 0x14, 0x14),
    pair("title", PatternWindow::Right, 0x00, 0x14),
    pair("new_game_choice", PatternWindow::Left, 0x1A, 0x1A),
    pair("new_game_choice", PatternWindow::Right, 0x00, 0x00),
    pair("intro_terrain", PatternWindow::Left, 0x1A, 0x1A),
    pair("intro_terrain", PatternWindow::Right, 0x15, 0x15),
    pair("class_profile", PatternWindow::Left, 0x14, 0x14),
    pair("class_profile", PatternWindow::Right, 0x00, 0x14),
    pair("intro_dialogue", PatternWindow::Left, 0x07, 0x07),
    pair("intro_dialogue", PatternWindow::Right, 0x00, 0x18),
    pair("game_over", PatternWindow::Left, 0x07, 0x07),
    pair("game_over", PatternWindow::Right, 0x00, 0x18),
    pair("later_intro_dialogue", PatternWindow::Left, 0x11, 0x11),
    pair("later_intro_dialogue", PatternWindow::Right, 0x00, 0x18),
    pair("map_idle", PatternWindow::Left, 0x1A, 0x1A),
    pair("map_idle", PatternWindow::Right, 0x15, 0x15),
    pair("map_idle", PatternWindow::Right, 0x18, 0x18),
    pair("map_idle", PatternWindow::Right, 0x19, 0x19),
    pair("unit_summary", PatternWindow::Left, 0x1A, 0x1A),
    pair("unit_summary", PatternWindow::Right, 0x00, 0x15),
    pair("unit_summary", PatternWindow::Right, 0x00, 0x18),
    pair("unit_summary", PatternWindow::Right, 0x00, 0x19),
    pair("unit_command_menu", PatternWindow::Left, 0x1A, 0x1A),
    pair("unit_command_menu", PatternWindow::Right, 0x00, 0x15),
    pair("unit_command_menu", PatternWindow::Right, 0x00, 0x18),
    pair("unit_command_menu", PatternWindow::Right, 0x00, 0x19),
    pair("unit_status", PatternWindow::Left, 0x13, 0x13),
    pair("unit_status", PatternWindow::Right, 0x00, 0x15),
    pair("unit_status", PatternWindow::Right, 0x00, 0x18),
    pair("unit_status", PatternWindow::Right, 0x00, 0x19),
    pair("item_inventory_list", PatternWindow::Left, 0x1A, 0x1A),
    pair("item_inventory_list", PatternWindow::Right, 0x00, 0x15),
    pair("item_action_menu", PatternWindow::Left, 0x1A, 0x1A),
    pair("item_action_menu", PatternWindow::Right, 0x00, 0x15),
    pair("item_equip_result", PatternWindow::Left, 0x1A, 0x1A),
    pair("item_equip_result", PatternWindow::Right, 0x00, 0x15),
    pair("item_use_result", PatternWindow::Left, 0x1A, 0x1A),
    pair("item_use_result", PatternWindow::Right, 0x00, 0x15),
    pair(
        "item_transfer_target_selection",
        PatternWindow::Left,
        0x1A,
        0x1A,
    ),
    pair(
        "item_transfer_target_selection",
        PatternWindow::Right,
        0x15,
        0x15,
    ),
    pair("map_menu", PatternWindow::Left, 0x1A, 0x1A),
    pair("map_menu", PatternWindow::Right, 0x00, 0x19),
    pair("options", PatternWindow::Left, 0x1A, 0x1A),
    pair("options", PatternWindow::Right, 0x00, 0x15),
    pair("unit_roster", PatternWindow::Left, 0x18, 0x18),
    pair("unit_roster", PatternWindow::Right, 0x00, 0x15),
    pair("unit_roster", PatternWindow::Right, 0x00, 0x18),
    pair("unit_roster", PatternWindow::Right, 0x00, 0x19),
    pair("battle_animation", PatternWindow::Left, 0x02, 0x06),
    pair("battle_animation", PatternWindow::Left, 0x06, 0x06),
    pair("battle_animation", PatternWindow::Right, 0x02, 0x06),
    pair(
        "chapter_clear_epilogue_dialogue",
        PatternWindow::Left,
        0x11,
        0x11,
    ),
    pair(
        "chapter_clear_epilogue_dialogue",
        PatternWindow::Right,
        0x00,
        0x18,
    ),
    pair("next_story_banner", PatternWindow::Left, 0x1B, 0x1B),
    pair("next_story_banner", PatternWindow::Right, 0x00, 0x18),
    pair("chapter_save_offer", PatternWindow::Left, 0x1B, 0x1B),
    pair("chapter_save_offer", PatternWindow::Right, 0x00, 0x18),
    pair(
        "chapter_save_complete_continue_prompt",
        PatternWindow::Left,
        0x1C,
        0x1C,
    ),
    pair(
        "chapter_save_complete_continue_prompt",
        PatternWindow::Right,
        0x00,
        0x18,
    ),
    pair(
        "chapter_save_complete_power_off_notice",
        PatternWindow::Left,
        0x1C,
        0x1C,
    ),
    pair(
        "chapter_save_complete_power_off_notice",
        PatternWindow::Right,
        0x00,
        0x18,
    ),
    pair("sound_test", PatternWindow::Left, 0x1C, 0x1C),
    pair("sound_test", PatternWindow::Right, 0x00, 0x18),
    pair(
        "ending_opening_and_cast_scroll",
        PatternWindow::Left,
        0x1C,
        0x1C,
    ),
    pair(
        "ending_opening_and_cast_scroll",
        PatternWindow::Right,
        0x00,
        0x00,
    ),
    pair(
        "ending_chapter_record_scroll",
        PatternWindow::Left,
        0x1C,
        0x1C,
    ),
    pair(
        "ending_chapter_record_scroll",
        PatternWindow::Right,
        0x00,
        0x00,
    ),
    pair("ending_staff_credits", PatternWindow::Left, 0x1C, 0x1C),
    pair("ending_staff_credits", PatternWindow::Right, 0x00, 0x00),
    pair(
        "ending_character_epilogue",
        PatternWindow::Left,
        0x1D,
        0x1D,
    ),
    pair(
        "ending_character_epilogue",
        PatternWindow::Left,
        0x10,
        0x10,
    ),
    pair(
        "ending_character_epilogue",
        PatternWindow::Left,
        0x13,
        0x13,
    ),
    pair(
        "ending_character_epilogue",
        PatternWindow::Right,
        0x00,
        0x00,
    ),
    pair("ending_final_signature", PatternWindow::Left, 0x1C, 0x1C),
    pair("ending_final_signature", PatternWindow::Right, 0x18, 0x00),
    pair(
        "chapter_transition_blackout",
        PatternWindow::Left,
        0x1A,
        0x1A,
    ),
    pair(
        "chapter_transition_blackout",
        PatternWindow::Left,
        0x1B,
        0x1B,
    ),
    pair(
        "chapter_transition_blackout",
        PatternWindow::Right,
        0x18,
        0x18,
    ),
    pair(
        "chapter_intro_title_dialogue_composite",
        PatternWindow::Left,
        0x13,
        0x13,
    ),
    pair(
        "chapter_intro_title_dialogue_composite",
        PatternWindow::Right,
        0x00,
        0x18,
    ),
    pair(
        "chapter_intro_title_dialogue_composite",
        PatternWindow::Left,
        0x0F,
        0x0F,
    ),
    pair(
        "chapter_intro_title_dialogue_composite",
        PatternWindow::Left,
        0x1A,
        0x1A,
    ),
    pair("weapon_shop_item_list", PatternWindow::Left, 0x1E, 0x1E),
    pair("weapon_shop_item_list", PatternWindow::Right, 0x00, 0x15),
    pair(
        "weapon_shop_purchase_confirmation",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_purchase_confirmation",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
    pair(
        "weapon_shop_purchase_result",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_purchase_result",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
    pair("weapon_shop_exit_message", PatternWindow::Left, 0x1E, 0x1E),
    pair("weapon_shop_exit_message", PatternWindow::Right, 0x00, 0x15),
    pair(
        "weapon_shop_inventory_full_message",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_inventory_full_message",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
    pair(
        "weapon_shop_insufficient_funds_message",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_insufficient_funds_message",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
    pair(
        "weapon_shop_item_restriction_confirmation",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_item_restriction_confirmation",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
    pair(
        "weapon_shop_declined_continue_prompt",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_declined_continue_prompt",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
    pair(
        "weapon_shop_purchase_inventory_full_exit",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_purchase_inventory_full_exit",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
];

const fn pair(
    screen_role: &'static str,
    pattern_window: PatternWindow,
    fd_source_page: u8,
    fe_source_page: u8,
) -> ObservedChrPair {
    ObservedChrPair::new(screen_role, pattern_window, fd_source_page, fe_source_page)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum InputBehavior {
    Automatic,
    InputWait,
    Mixed,
    TerminalInstruction,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum TranslationScope {
    NoText,
    JapaneseOnly,
    JapaneseWithPreservedOriginalLatin,
    PreservedOriginalOnly,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ContractState {
    PageSwitchVerified,
    MixedTextPageVerified,
    ObservedPartial,
    Unobserved,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScreenContractRegistry {
    schema: u32,
    next_observation_gate: ScreenObservationGate,
    screens: Vec<ScreenContractSeed>,
    unpartitioned_surface_families: Vec<UnpartitionedSurfaceFamily>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ObservationGateKind {
    ScreenSequence,
    ScreenPartition,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScreenObservationGate {
    gate_role: String,
    gate_kind: ObservationGateKind,
    focus_screen_roles: Vec<String>,
    known_focus: Vec<String>,
    unresolved_focus: Vec<String>,
    next_action: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScreenContractSeed {
    screen_role: String,
    surface_family: String,
    runtime_observed: bool,
    input_behavior: InputBehavior,
    translation_scope: TranslationScope,
    temporal_behavior: String,
    known_focus: Vec<String>,
    unresolved_focus: Vec<String>,
    next_gate: String,
    contract_state: ContractState,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UnpartitionedSurfaceFamily {
    family_role: String,
    surface_family: String,
    entry_condition: String,
    source_bound: bool,
    known_focus: Vec<String>,
    unresolved_focus: Vec<String>,
    next_gate: String,
}

#[derive(Debug, Serialize)]
struct ScreenContractReport {
    schema: u32,
    source_sha1: &'static str,
    registry_sha1: String,
    coverage_dimensions: [&'static str; 8],
    screen_count: usize,
    unpartitioned_surface_family_count: usize,
    runtime_observed_screen_count: usize,
    chr_pair_observed_screen_count: usize,
    mixed_original_latin_screen_count: usize,
    preserved_original_only_screen_count: usize,
    page_switch_verified_screen_count: usize,
    mixed_text_page_verified_screen_count: usize,
    next_observation_gate: ScreenObservationGate,
    screens: Vec<ScreenContract>,
    unpartitioned_surface_families: Vec<UnpartitionedSurfaceFamily>,
    unresolved_surface_families: Vec<String>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct ScreenContract {
    screen_role: String,
    surface_family: String,
    runtime_observed: bool,
    chr_pair_observed: bool,
    input_behavior: InputBehavior,
    translation_scope: TranslationScope,
    temporal_behavior: String,
    known_focus: Vec<String>,
    unresolved_focus: Vec<String>,
    next_gate: String,
    contract_state: ContractState,
}

pub struct ScreenContractSummary {
    pub report_sha1: String,
    pub screen_count: usize,
    pub runtime_observed_screen_count: usize,
    pub mixed_original_latin_screen_count: usize,
    pub next_observation_gate_role: String,
}

pub fn analyze_screen_contracts(
    source_path: &Path,
    report_path: &Path,
) -> Result<ScreenContractSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS)?;
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize screen-contract report")?;
    let report_sha1 = sha1_hex(&report_bytes);

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(ScreenContractSummary {
        report_sha1,
        screen_count: report.screen_count,
        runtime_observed_screen_count: report.runtime_observed_screen_count,
        mixed_original_latin_screen_count: report.mixed_original_latin_screen_count,
        next_observation_gate_role: report.next_observation_gate.gate_role,
    })
}

fn build_report(
    registry_json: &str,
    observed_chr_pairs: &[ObservedChrPair],
) -> Result<ScreenContractReport> {
    let registry: ScreenContractRegistry =
        serde_json::from_str(registry_json).context("parse screen-contract registry")?;
    ensure!(
        registry.schema == 2,
        "unsupported screen-contract registry schema"
    );
    ensure!(!registry.screens.is_empty(), "no screen contracts supplied");
    let mut roles = BTreeMap::new();
    for seed in &registry.screens {
        ensure!(
            roles.insert(seed.screen_role.as_str(), seed).is_none(),
            "duplicate screen role {}",
            seed.screen_role
        );
    }

    let mut family_roles = BTreeSet::new();
    for family in &registry.unpartitioned_surface_families {
        ensure!(
            family_roles.insert(family.family_role.as_str()),
            "duplicate unpartitioned surface family {}",
            family.family_role
        );
        ensure!(
            !roles.contains_key(family.family_role.as_str()),
            "unpartitioned surface family {} masquerades as a screen role",
            family.family_role
        );
        ensure!(
            !family.entry_condition.is_empty(),
            "unpartitioned surface family {} has no entry condition",
            family.family_role
        );
        ensure!(
            !family.known_focus.is_empty() && !family.unresolved_focus.is_empty(),
            "unpartitioned surface family {} lacks focus boundaries",
            family.family_role
        );
        ensure!(
            !family.next_gate.is_empty(),
            "unpartitioned surface family {} has no next gate",
            family.family_role
        );
    }

    let chr_pair_roles = observed_chr_pairs
        .iter()
        .map(|pair| pair.screen_role)
        .collect::<BTreeSet<_>>();
    for role in &chr_pair_roles {
        let seed = roles
            .get(role)
            .with_context(|| format!("CHR pair references unknown screen role {role}"))?;
        ensure!(
            seed.runtime_observed,
            "CHR pair references unobserved screen role {role}"
        );
    }

    ensure!(
        !roles.contains_key(registry.next_observation_gate.gate_role.as_str()),
        "observation gate {} must not masquerade as a screen role",
        registry.next_observation_gate.gate_role
    );
    ensure!(
        !registry.next_observation_gate.focus_screen_roles.is_empty(),
        "observation gate has no focus screen roles"
    );
    let mut focus_roles = BTreeSet::new();
    for role in &registry.next_observation_gate.focus_screen_roles {
        ensure!(
            focus_roles.insert(role.as_str()),
            "observation gate repeats focus screen role {role}"
        );
        roles
            .get(role.as_str())
            .with_context(|| format!("observation gate references unknown screen role {role}"))?;
    }
    ensure!(
        !registry.next_observation_gate.unresolved_focus.is_empty(),
        "observation gate has no unresolved focus"
    );
    ensure!(
        !registry.next_observation_gate.next_action.is_empty(),
        "observation gate has no next action"
    );

    let screens = registry
        .screens
        .into_iter()
        .map(|seed| ScreenContract {
            chr_pair_observed: chr_pair_roles.contains(seed.screen_role.as_str()),
            screen_role: seed.screen_role,
            surface_family: seed.surface_family,
            runtime_observed: seed.runtime_observed,
            input_behavior: seed.input_behavior,
            translation_scope: seed.translation_scope,
            temporal_behavior: seed.temporal_behavior,
            known_focus: seed.known_focus,
            unresolved_focus: seed.unresolved_focus,
            next_gate: seed.next_gate,
            contract_state: seed.contract_state,
        })
        .collect::<Vec<_>>();
    let mut unresolved_surface_families = screens
        .iter()
        .filter(|screen| !screen.runtime_observed)
        .map(|screen| screen.surface_family.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<BTreeSet<_>>();
    unresolved_surface_families.extend(
        registry
            .unpartitioned_surface_families
            .iter()
            .map(|family| family.surface_family.clone()),
    );
    let unresolved_surface_families = unresolved_surface_families.into_iter().collect::<Vec<_>>();

    Ok(ScreenContractReport {
        schema: 2,
        source_sha1: EXPECTED_SOURCE_SHA1,
        registry_sha1: sha1_hex(registry_json.as_bytes()),
        coverage_dimensions: [
            "Japanese text target",
            "preserved original Latin and digits",
            "FD text tiles",
            "FE background graphics",
            "sprite UI",
            "temporal or flashing UI",
            "input wait versus automatic transition",
            "font-page entry, exit, and re-entry lifetime",
        ],
        screen_count: screens.len(),
        unpartitioned_surface_family_count: registry.unpartitioned_surface_families.len(),
        runtime_observed_screen_count: screens
            .iter()
            .filter(|screen| screen.runtime_observed)
            .count(),
        chr_pair_observed_screen_count: chr_pair_roles.len(),
        mixed_original_latin_screen_count: screens
            .iter()
            .filter(|screen| {
                screen.translation_scope == TranslationScope::JapaneseWithPreservedOriginalLatin
            })
            .count(),
        preserved_original_only_screen_count: screens
            .iter()
            .filter(|screen| screen.translation_scope == TranslationScope::PreservedOriginalOnly)
            .count(),
        page_switch_verified_screen_count: screens
            .iter()
            .filter(|screen| screen.contract_state == ContractState::PageSwitchVerified)
            .count(),
        mixed_text_page_verified_screen_count: screens
            .iter()
            .filter(|screen| screen.contract_state == ContractState::MixedTextPageVerified)
            .count(),
        next_observation_gate: registry.next_observation_gate,
        screens,
        unpartitioned_surface_families: registry.unpartitioned_surface_families,
        unresolved_surface_families,
        release_eligible: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_every_observed_chr_pair() {
        let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();

        assert_eq!(report.screen_count, 45);
        assert_eq!(report.unpartitioned_surface_family_count, 0);
        assert_eq!(report.runtime_observed_screen_count, 45);
        assert_eq!(report.chr_pair_observed_screen_count, 42);
        assert_eq!(report.mixed_original_latin_screen_count, 19);
        assert_eq!(report.preserved_original_only_screen_count, 5);
        assert_eq!(report.page_switch_verified_screen_count, 1);
        assert_eq!(report.mixed_text_page_verified_screen_count, 1);
        assert!(report.unresolved_surface_families.is_empty());
        assert!(report.unpartitioned_surface_families.is_empty());
        assert!(report.screens.iter().any(|screen| {
            screen.screen_role == "ending_character_epilogue"
                && screen.runtime_observed
                && screen.input_behavior == InputBehavior::Automatic
        }));
    }

    #[test]
    fn command_menu_keeps_remaining_labels_and_actions_as_open_work() {
        let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();
        let command_menu = report
            .screens
            .iter()
            .find(|screen| screen.screen_role == "unit_command_menu")
            .unwrap();

        assert_eq!(command_menu.screen_role, "unit_command_menu");
        assert!(command_menu.runtime_observed);
        assert!(command_menu.chr_pair_observed);
        assert_eq!(
            command_menu.translation_scope,
            TranslationScope::JapaneseOnly
        );
        assert!(command_menu.next_gate.contains("expected state effect"));
        assert!(command_menu
            .unresolved_focus
            .iter()
            .any(|focus| focus.contains("remaining 9 command labels")));
        assert!(!command_menu
            .unresolved_focus
            .iter()
            .any(|focus| focus.contains("00/19")));
        assert!(command_menu
            .known_focus
            .iter()
            .any(|focus| focus.contains("00/19")));
        assert!(command_menu
            .known_focus
            .iter()
            .any(|focus| focus.contains("C9C2") && focus.contains("00/15")));
        assert!(command_menu
            .known_focus
            .iter()
            .any(|focus| focus.contains("こうげき") && focus.contains("しろ")));
    }

    #[test]
    fn next_observation_gate_reuses_real_screen_roles_without_becoming_a_screen() {
        let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();

        assert_eq!(
            report.next_observation_gate.gate_role,
            "battle_and_ending_temporal_glyph_variant_union"
        );
        assert_eq!(
            report.next_observation_gate.gate_kind,
            ObservationGateKind::ScreenSequence
        );
        assert_eq!(
            report.next_observation_gate.focus_screen_roles,
            [
                "battle_animation",
                "ending_chapter_record_scroll",
                "ending_character_epilogue",
            ]
        );
        assert!(report
            .next_observation_gate
            .focus_screen_roles
            .iter()
            .all(|role| report
                .screens
                .iter()
                .any(|screen| &screen.screen_role == role)));
        assert!(!report
            .screens
            .iter()
            .any(|screen| { screen.screen_role == report.next_observation_gate.gate_role }));
    }

    #[test]
    fn observation_gate_cannot_masquerade_as_a_screen_role() {
        let invalid_registry = REGISTRY_JSON.replacen(
            "\"gate_role\": \"battle_and_ending_temporal_glyph_variant_union\"",
            "\"gate_role\": \"title\"",
            1,
        );

        let error = build_report(&invalid_registry, OBSERVED_CHR_PAIRS)
            .unwrap_err()
            .to_string();
        assert!(error.contains("must not masquerade as a screen role"));
    }

    #[test]
    fn ending_lifetimes_keep_translation_scopes_and_static_terminal_distinct() {
        let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();
        let ending = report
            .screens
            .iter()
            .filter(|screen| screen.surface_family == "ending")
            .collect::<Vec<_>>();

        assert_eq!(ending.len(), 5);
        assert!(ending.iter().any(|screen| {
            screen.screen_role == "ending_opening_and_cast_scroll"
                && screen.translation_scope == TranslationScope::PreservedOriginalOnly
        }));
        assert!(ending.iter().any(|screen| {
            screen.screen_role == "ending_chapter_record_scroll"
                && screen.translation_scope
                    == TranslationScope::JapaneseWithPreservedOriginalLatin
        }));
        assert!(ending.iter().any(|screen| {
            screen.screen_role == "ending_staff_credits"
                && screen.translation_scope == TranslationScope::PreservedOriginalOnly
        }));
        assert!(ending.iter().any(|screen| {
            screen.screen_role == "ending_character_epilogue"
                && screen.translation_scope == TranslationScope::JapaneseOnly
        }));
        assert!(ending.iter().any(|screen| {
            screen.screen_role == "ending_final_signature"
                && screen
                    .temporal_behavior
                    .contains("keeps the original signature")
                && screen.unresolved_focus.is_empty()
        }));
    }

    #[test]
    fn chapter_transition_screens_keep_distinct_lifetimes_and_translation_scopes() {
        let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();
        let chapter_screens = report
            .screens
            .iter()
            .filter(|screen| screen.surface_family == "chapter_transition")
            .collect::<Vec<_>>();

        assert_eq!(chapter_screens.len(), 8);
        for role in [
            "chapter_clear_epilogue_dialogue",
            "next_story_banner",
            "chapter_save_offer",
            "chapter_save_complete_continue_prompt",
            "chapter_save_complete_power_off_notice",
            "sound_test",
            "chapter_transition_blackout",
            "chapter_intro_title_dialogue_composite",
        ] {
            assert!(chapter_screens
                .iter()
                .any(|screen| screen.screen_role == role && screen.runtime_observed));
        }
        let next_story = chapter_screens
            .iter()
            .find(|screen| screen.screen_role == "next_story_banner")
            .unwrap();
        assert_eq!(
            next_story.translation_scope,
            TranslationScope::PreservedOriginalOnly
        );
        assert!(chapter_screens
            .iter()
            .all(|screen| screen.chr_pair_observed));
        let blackout = chapter_screens
            .iter()
            .find(|screen| screen.screen_role == "chapter_transition_blackout")
            .unwrap();
        assert_eq!(blackout.input_behavior, InputBehavior::Automatic);
        assert_eq!(blackout.translation_scope, TranslationScope::NoText);
        assert!(blackout
            .known_focus
            .iter()
            .any(|focus| focus.contains("outer state 01") && focus.contains("1B/1B")));
        assert!(OBSERVED_CHR_PAIRS.iter().any(|pair| {
            pair.screen_role == "chapter_transition_blackout"
                && pair.pattern_window == PatternWindow::Left
                && pair.fd_source_page == 0x1B
                && pair.fe_source_page == 0x1B
        }));
        let save_offer = chapter_screens
            .iter()
            .find(|screen| screen.screen_role == "chapter_save_offer")
            .unwrap();
        assert!(save_offer
            .known_focus
            .iter()
            .any(|focus| focus.contains("7FF4") && focus.contains("01 to 02")));
        assert!(save_offer
            .unresolved_focus
            .iter()
            .all(|focus| !focus.contains("no-choice")));
        let save_complete = chapter_screens
            .iter()
            .find(|screen| screen.screen_role == "chapter_save_complete_continue_prompt")
            .unwrap();
        assert!(save_complete
            .unresolved_focus
            .iter()
            .all(|focus| !focus.contains("no-choice")));
        let power_off_notice = chapter_screens
            .iter()
            .find(|screen| screen.screen_role == "chapter_save_complete_power_off_notice")
            .unwrap();
        assert_eq!(
            power_off_notice.input_behavior,
            InputBehavior::TerminalInstruction
        );
        let sound_test = chapter_screens
            .iter()
            .find(|screen| screen.screen_role == "sound_test")
            .unwrap();
        assert_eq!(
            sound_test.translation_scope,
            TranslationScope::PreservedOriginalOnly
        );
        let intro = chapter_screens
            .iter()
            .find(|screen| screen.screen_role == "chapter_intro_title_dialogue_composite")
            .unwrap();
        assert!(intro.chr_pair_observed);
        assert_eq!(
            intro.translation_scope,
            TranslationScope::JapaneseWithPreservedOriginalLatin
        );
    }

    #[test]
    fn item_action_results_remain_distinct_screen_roles() {
        let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();
        let item_screens = report
            .screens
            .iter()
            .filter(|screen| screen.surface_family == "item")
            .collect::<Vec<_>>();

        assert_eq!(item_screens.len(), 7);
        assert!(item_screens
            .iter()
            .all(|screen| screen.screen_role != "item_action_result"));
        for role in [
            "item_equip_result",
            "item_use_result",
            "item_transfer_target_selection",
            "item_transfer_result",
            "item_discard_result",
        ] {
            assert!(item_screens
                .iter()
                .any(|screen| screen.screen_role == role && screen.runtime_observed));
        }
    }

    #[test]
    fn observed_shop_screens_keep_japanese_and_preserved_latin_separate() {
        let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();
        let shop_screens = report
            .screens
            .iter()
            .filter(|screen| screen.surface_family == "weapon_shop")
            .collect::<Vec<_>>();

        assert_eq!(shop_screens.len(), 9);
        assert_eq!(
            shop_screens
                .iter()
                .filter(|screen| screen.runtime_observed)
                .count(),
            9
        );
        assert!(shop_screens.iter().all(|screen| screen.chr_pair_observed));
        assert_eq!(
            shop_screens
                .iter()
                .filter(|screen| {
                    screen.translation_scope == TranslationScope::JapaneseWithPreservedOriginalLatin
                })
                .count(),
            8
        );
        let declined_prompt = shop_screens
            .iter()
            .find(|screen| screen.screen_role == "weapon_shop_declined_continue_prompt")
            .unwrap();
        assert_eq!(
            declined_prompt.translation_scope,
            TranslationScope::JapaneseOnly
        );
    }

    #[test]
    fn unknown_chr_pair_role_is_rejected() {
        let unknown = [ObservedChrPair::new("unknown", PatternWindow::Right, 0, 0)];

        assert!(build_report(REGISTRY_JSON, &unknown).is_err());
    }
}
