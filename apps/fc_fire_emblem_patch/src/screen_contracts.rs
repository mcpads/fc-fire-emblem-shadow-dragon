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

mod observed_chr;
#[cfg(test)]
mod tests;

pub(crate) use observed_chr::{OBSERVED_CHR_PAIRS, ObservedChrPair, PatternWindow};

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
