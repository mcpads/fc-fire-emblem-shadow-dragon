use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    rom::{EXPECTED_SOURCE_SHA1, Rom},
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
    pair("chapter2_intro", PatternWindow::Left, 0x13, 0x13),
    pair("chapter2_intro", PatternWindow::Right, 0x00, 0x18),
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
    next_screen_role: String,
    screens: Vec<ScreenContractSeed>,
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

#[derive(Debug, Serialize)]
struct ScreenContractReport {
    schema: u32,
    source_sha1: &'static str,
    registry_sha1: String,
    coverage_dimensions: [&'static str; 8],
    screen_count: usize,
    runtime_observed_screen_count: usize,
    chr_pair_observed_screen_count: usize,
    mixed_original_latin_screen_count: usize,
    page_switch_verified_screen_count: usize,
    mixed_text_page_verified_screen_count: usize,
    next_screen_role: String,
    screens: Vec<ScreenContract>,
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
    pub next_screen_role: String,
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
        next_screen_role: report.next_screen_role,
    })
}

fn build_report(
    registry_json: &str,
    observed_chr_pairs: &[ObservedChrPair],
) -> Result<ScreenContractReport> {
    let registry: ScreenContractRegistry =
        serde_json::from_str(registry_json).context("parse screen-contract registry")?;
    ensure!(
        registry.schema == 1,
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

    roles
        .get(registry.next_screen_role.as_str())
        .context("next screen role is absent from registry")?;

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
    let unresolved_surface_families = screens
        .iter()
        .filter(|screen| !screen.runtime_observed)
        .map(|screen| screen.surface_family.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    Ok(ScreenContractReport {
        schema: 1,
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
        page_switch_verified_screen_count: screens
            .iter()
            .filter(|screen| screen.contract_state == ContractState::PageSwitchVerified)
            .count(),
        mixed_text_page_verified_screen_count: screens
            .iter()
            .filter(|screen| screen.contract_state == ContractState::MixedTextPageVerified)
            .count(),
        next_screen_role: registry.next_screen_role,
        screens,
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

        assert_eq!(report.screen_count, 30);
        assert_eq!(report.runtime_observed_screen_count, 27);
        assert_eq!(report.chr_pair_observed_screen_count, 25);
        assert_eq!(report.mixed_original_latin_screen_count, 15);
        assert_eq!(report.page_switch_verified_screen_count, 1);
        assert_eq!(report.mixed_text_page_verified_screen_count, 1);
    }

    #[test]
    fn command_menu_keeps_unobserved_labels_and_action_dispatch_as_open_work() {
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
        assert!(command_menu.next_gate.contains("action dispatch"));
        assert!(
            command_menu
                .unresolved_focus
                .iter()
                .any(|focus| focus.contains("remaining 11 command labels"))
        );
        assert!(
            !command_menu
                .unresolved_focus
                .iter()
                .any(|focus| focus.contains("00/19"))
        );
        assert!(
            command_menu
                .known_focus
                .iter()
                .any(|focus| focus.contains("00/19"))
        );
        assert!(
            command_menu
                .known_focus
                .iter()
                .any(|focus| focus.contains("C9C2") && focus.contains("00/15"))
        );
    }

    #[test]
    fn next_screen_moves_from_the_closed_shop_family_to_item_flow() {
        let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();
        let next = report
            .screens
            .iter()
            .find(|screen| screen.screen_role == report.next_screen_role)
            .unwrap();

        assert_eq!(next.screen_role, "item_flow");
        assert!(!next.runtime_observed);
        assert_eq!(next.contract_state, ContractState::Unobserved);
        assert!(next.next_gate.contains("split item list"));
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
