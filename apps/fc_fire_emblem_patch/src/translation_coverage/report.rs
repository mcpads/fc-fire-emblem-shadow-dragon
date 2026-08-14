use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceBindingState {
    Bound,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TranslationInputState {
    Complete,
    Partial,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapacityState {
    NotEvaluatedInGlobalContext,
}

#[derive(Debug, Clone)]
pub(crate) struct DomainPopulation {
    pub(crate) source_binding: SourceBindingState,
    pub(crate) target_unit_count: Option<usize>,
    pub(crate) translated_target_unit_count: usize,
    pub(crate) translation_input: TranslationInputState,
    pub(crate) review_complete: bool,
    pub(crate) translation_input_sha1: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DomainInstallation {
    pub(crate) installed_target_unit_count: usize,
    pub(crate) installed_screen_roles: Vec<String>,
    /// 전역 도메인 개수와 달라도 원천 소비자 부분집합 전체가 설치된 화면이다.
    pub(crate) consumer_complete_screen_roles: Vec<String>,
    pub(crate) runtime_bound_screen_roles: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct GlobalTranslationCoverageReport {
    pub(crate) schema: u8,
    pub(crate) source_sha1: &'static str,
    pub(crate) build_output_sha1: String,
    pub(crate) screen_population: ScreenPopulationReport,
    pub(crate) domains: Vec<TranslationDomainReport>,
    pub(crate) lifetime_demands: Vec<TranslationLifetimeDemandReport>,
    pub(crate) unmeasured_screen_roles: Vec<String>,
    pub(crate) strongest_lifetime: StrongestLifetimeReport,
    pub(crate) summary: CoverageSummary,
    pub(crate) translation_text_emitted: bool,
    pub(crate) glyph_characters_emitted: bool,
    pub(crate) release_eligible: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ScreenPopulationReport {
    pub(crate) screen_count: usize,
    pub(crate) japanese_bearing_screen_count: usize,
    pub(crate) preserved_original_only_screen_count: usize,
    pub(crate) no_text_screen_count: usize,
    pub(crate) mapped_japanese_bearing_screen_count: usize,
    pub(crate) unmapped_japanese_bearing_screen_roles: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TranslationDomainReport {
    pub(crate) id: &'static str,
    pub(crate) target_unit: &'static str,
    pub(crate) source_binding: SourceBindingState,
    pub(crate) target_unit_count: Option<usize>,
    pub(crate) translated_target_unit_count: usize,
    pub(crate) translation_input: TranslationInputState,
    pub(crate) review_complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) translation_input_sha1: Option<String>,
    pub(crate) installed_target_unit_count: usize,
    pub(crate) target_screen_roles: Vec<String>,
    pub(crate) installed_screen_roles: Vec<String>,
    pub(crate) runtime_bound_screen_roles: Vec<String>,
    pub(crate) all_target_units_installed: bool,
    pub(crate) all_consumers_installed: bool,
    pub(crate) all_consumers_runtime_bound: bool,
    pub(crate) capacity_state: CapacityState,
}

#[derive(Debug, Serialize)]
pub(crate) struct StrongestLifetimeReport {
    pub(crate) state: &'static str,
    pub(crate) compared_lifetime_count: usize,
    pub(crate) japanese_bearing_screen_count: usize,
    pub(crate) selected_screen_role: Option<&'static str>,
    pub(crate) selected_slot_demand: Option<usize>,
    pub(crate) main_dialogue_maximum_target_glyph_count: usize,
    pub(crate) main_dialogue_maximum_screen_bound: bool,
    pub(crate) next_gate: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct TranslationLifetimeDemandReport {
    pub(crate) screen_role: &'static str,
    pub(crate) measurement_basis: &'static str,
    pub(crate) target_glyph_count: usize,
    pub(crate) preserved_active_source_code_count: usize,
    pub(crate) additional_target_glyph_reservation_count: usize,
    pub(crate) total_slot_demand: usize,
    pub(crate) active_slot_count: usize,
    pub(crate) fits_active_page: bool,
    pub(crate) evidence_report_sha1: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CoverageSummary {
    pub(crate) domain_count: usize,
    pub(crate) source_bound_domain_count: usize,
    pub(crate) translation_input_complete_domain_count: usize,
    pub(crate) review_complete_domain_count: usize,
    pub(crate) all_consumers_installed_domain_count: usize,
    pub(crate) all_consumers_runtime_bound_domain_count: usize,
    pub(crate) unresolved_source_domain_ids: Vec<&'static str>,
    pub(crate) incomplete_translation_input_domain_ids: Vec<&'static str>,
    pub(crate) pending_review_domain_ids: Vec<&'static str>,
    pub(crate) incomplete_installation_domain_ids: Vec<&'static str>,
}

pub(crate) struct TranslationCoverageSummary {
    pub(crate) report_sha1: String,
    pub(crate) japanese_bearing_screen_count: usize,
    pub(crate) domain_count: usize,
    pub(crate) unresolved_source_domain_count: usize,
    pub(crate) all_consumers_installed_domain_count: usize,
}
