//! 고정 화면 소비자들이 공유할 물리 글꼴 코드와 CHR 페이지를 한 번에 정한다.
//!
//! 전체 번역 글리프의 단순 합집합은 210슬롯을 넘지만, 서로 다른 화면 수명은 같은
//! 물리 코드를 재사용할 수 있다. 따라서 글리프를 정점, 같은 수명에서 보이는 두
//! 글리프를 간선으로 하는 충돌 그래프를 만들고, 대사 동적 문자열과 장 제목에서 이미
//! 저장 바이트로 확정한 코드는 선색칠한다. 가변 카탈로그는 별도 런타임 투영기가 맡는다.

mod assignment;
mod lifetimes;

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::{ChapterTitlePlan, TransitionTranslationPlans},
    choice_labels::ChoiceLabelPlan,
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, FONT_PAGE_SIZE, active_hangul_codes},
    map_menu::MapMenuPlan,
    mapper165::{
        MAXIMUM_CHR_PAGE_COUNT, dialogue_font_page::build_font_page_by_code,
        encode_chr_page_register, font_pair_projection::RightFontPageProjection,
    },
    semantic_translation::SemanticTranslationPlan,
    sha1_hex,
    text_inventory::{FixedTextLogicalByte, FixedTextPlan},
    unit_names::UnitNamePlan,
};

use self::{
    assignment::{
        ConflictGraph, assign_codes, assignment_sha1, merge_preassignments, verify_assignment,
    },
    lifetimes::{build_lifetimes, forbidden_codes_by_glyph},
};
use super::{
    chapter_intro_residency::ChapterIntroResidencyPlan, dynamic_inputs::DynamicDialogueInputPlan,
    unit_selection_help_residency::UnitSelectionHelpLifetimePlan,
};

pub(super) struct ConsumerCodebookInputs<'a> {
    pub(super) source_font_page: &'a [u8],
    pub(super) source_chr: &'a [u8],
    pub(super) first_physical_page: u8,
    pub(super) available_page_count: usize,
    pub(super) dynamic_inputs: &'a DynamicDialogueInputPlan,
    pub(super) chapter_intro: &'a ChapterIntroResidencyPlan,
    pub(super) fixed: &'a FixedTextPlan,
    pub(super) unit_names: &'a UnitNamePlan,
    pub(super) chapter_titles: &'a ChapterTitlePlan,
    pub(super) choices: &'a ChoiceLabelPlan,
    pub(super) choice_glyph_codes: &'a BTreeMap<char, u8>,
    pub(super) unit_selection_help: &'a UnitSelectionHelpLifetimePlan,
    pub(super) map_menu: &'a MapMenuPlan,
    pub(super) unit_ui: &'a SemanticTranslationPlan,
    pub(super) item_actions: &'a SemanticTranslationPlan,
    pub(super) fixed_menu_labels: &'a SemanticTranslationPlan,
    pub(super) options_glyph_codes: &'a BTreeMap<char, u8>,
    pub(super) transitions: &'a TransitionTranslationPlans,
}

#[derive(Serialize)]
pub(super) struct ConsumerCodebookPlan {
    schema: u8,
    strategy: &'static str,
    glyph_count: usize,
    conflict_edge_count: usize,
    preassigned_glyph_count: usize,
    canonical_dynamic_glyph_count: usize,
    chapter_title_fixed_glyph_count: usize,
    physical_code_count: usize,
    active_code_ceiling: usize,
    constraint_count: usize,
    maximum_constraint_slot_demand: usize,
    static_page_count: usize,
    first_physical_page: u8,
    available_page_count: usize,
    assignment_sha1: String,
    coloring_strategy: &'static str,
    color_split_count: usize,
    pages: Vec<StaticConsumerPage>,
    every_preassignment_preserved: bool,
    every_constraint_is_injective: bool,
    every_preserved_code_avoided: bool,
    static_pages_fit_reclaimable_tail: bool,
    page_bytes_planned: bool,
}

impl ConsumerCodebookPlan {
    pub(super) fn pages(&self) -> &[StaticConsumerPage] {
        &self.pages
    }

    pub(super) fn next_physical_page(&self) -> Result<u8> {
        self.first_physical_page
            .checked_add(u8::try_from(self.pages.len()).context("consumer page count exceeds u8")?)
            .context("consumer physical page range overflow")
    }

    pub(super) fn remaining_page_count(&self) -> Result<usize> {
        self.available_page_count
            .checked_sub(self.pages.len())
            .context("consumer codebook exhausted the available page range")
    }

    pub(super) fn mapper_route_for(&self, page_id: &str) -> Result<u8> {
        self.pages
            .iter()
            .find(|page| page.id == page_id)
            .map(StaticConsumerPage::mapper_route)
            .with_context(|| format!("consumer codebook has no {page_id} page"))
    }

    pub(super) fn encode_fixed_ui_for(
        &self,
        page_id: &str,
        logical: &[FixedTextLogicalByte],
    ) -> Result<Vec<u8>> {
        self.encode_for(page_id, CodeOwner::FixedUi, logical)
    }

    pub(super) fn fixed_ui_glyph_codes_for(
        &self,
        page_id: &str,
        glyphs: &BTreeSet<char>,
    ) -> Result<BTreeMap<char, u8>> {
        let page = self
            .pages
            .iter()
            .find(|page| page.id == page_id)
            .with_context(|| format!("consumer codebook has no {page_id} page"))?;
        glyphs
            .iter()
            .map(|glyph| {
                page.assignments
                    .get(&GlyphKey {
                        owner: CodeOwner::FixedUi,
                        glyph: *glyph,
                    })
                    .copied()
                    .with_context(|| {
                        format!("consumer page {page_id} has no fixed-UI code for {glyph:?}")
                    })
                    .map(|code| (*glyph, code))
            })
            .collect()
    }

    pub(super) fn encode_chapter_title_for(
        &self,
        page_id: &str,
        logical: &[FixedTextLogicalByte],
    ) -> Result<Vec<u8>> {
        self.encode_for(page_id, CodeOwner::ChapterTitle, logical)
    }

    pub(super) fn validate_unit_command_residency(
        &self,
        required_fixed_ui_glyphs: &BTreeSet<char>,
        options_glyph_codes: &BTreeMap<char, u8>,
    ) -> Result<()> {
        let page = self
            .pages
            .iter()
            .find(|page| page.id == "unit_command_menu")
            .context("consumer codebook lost the unit-command page")?;
        ensure_unit_command_assignments(
            &page.assignments,
            required_fixed_ui_glyphs,
            options_glyph_codes,
        )?;
        Ok(())
    }

    pub(super) fn validate_static_page_residency(
        &self,
        page_id: &str,
        required_fixed_ui_glyphs: &BTreeSet<char>,
        required_chapter_title_glyphs: &BTreeSet<char>,
    ) -> Result<()> {
        let page = self
            .pages
            .iter()
            .find(|page| page.id == page_id)
            .with_context(|| format!("consumer codebook lost the {page_id} page"))?;
        ensure_owned_glyphs(
            &page.assignments,
            CodeOwner::FixedUi,
            required_fixed_ui_glyphs,
            page_id,
        )?;
        ensure_owned_glyphs(
            &page.assignments,
            CodeOwner::ChapterTitle,
            required_chapter_title_glyphs,
            page_id,
        )?;
        Ok(())
    }

    fn encode_for(
        &self,
        page_id: &str,
        owner: CodeOwner,
        logical: &[FixedTextLogicalByte],
    ) -> Result<Vec<u8>> {
        let page = self
            .pages
            .iter()
            .find(|page| page.id == page_id)
            .with_context(|| format!("consumer codebook has no {page_id} page"))?;
        logical
            .iter()
            .map(|byte| match byte {
                FixedTextLogicalByte::Encoded(value) => Ok(*value),
                FixedTextLogicalByte::TargetGlyph(glyph) => page
                    .assignments
                    .get(&GlyphKey {
                        owner,
                        glyph: *glyph,
                    })
                    .copied()
                    .with_context(|| {
                        format!("consumer page {page_id} has no {owner:?} code for {glyph:?}")
                    }),
            })
            .collect()
    }
}

#[derive(Serialize)]
pub(super) struct StaticConsumerPage {
    pub(super) id: &'static str,
    variant: &'static str,
    screen_roles: Vec<&'static str>,
    domain_ids: Vec<&'static str>,
    target_glyph_count: usize,
    preserved_active_code_count: usize,
    slot_demand: usize,
    physical_page: u8,
    mapper_register: u8,
    mapper_route: u8,
    assignment_sha1: String,
    page_sha1: String,
    #[serde(skip)]
    assignments: BTreeMap<GlyphKey, u8>,
    #[serde(skip)]
    pub(super) bytes: Vec<u8>,
}

impl StaticConsumerPage {
    pub(super) fn physical_page(&self) -> u8 {
        self.physical_page
    }

    pub(super) fn assignment_count(&self) -> usize {
        self.assignments.len()
    }

    pub(super) fn mapper_route(&self) -> u8 {
        self.mapper_route
    }
}

#[derive(Clone)]
struct Lifetime {
    id: &'static str,
    variant: &'static str,
    screen_roles: Vec<&'static str>,
    domain_ids: Vec<&'static str>,
    target_glyphs: BTreeSet<GlyphKey>,
    preserved_active_codes: BTreeSet<u8>,
    emit_static_page: bool,
}

/// 같은 모양이라도 저장 바이트를 생산하는 표가 다르면 독립된 코드 계약이다.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CodeOwner {
    DialogueDynamic,
    ChapterTitle,
    FixedUi,
    OptionsTable,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct GlyphKey {
    owner: CodeOwner,
    glyph: char,
}

pub(super) fn plan_consumer_codebook(
    inputs: ConsumerCodebookInputs<'_>,
) -> Result<ConsumerCodebookPlan> {
    ensure!(
        inputs.source_font_page.len() == FONT_PAGE_SIZE,
        "consumer codebook source font page is not 4 KiB"
    );
    let lifetimes = build_lifetimes(&inputs)?;
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let maximum_constraint = lifetimes
        .iter()
        .max_by_key(|lifetime| lifetime.target_glyphs.len() + lifetime.preserved_active_codes.len())
        .context("consumer codebook has no lifetime constraints")?;
    let maximum_constraint_slot_demand =
        maximum_constraint.target_glyphs.len() + maximum_constraint.preserved_active_codes.len();
    ensure!(
        maximum_constraint_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "consumer lifetime {} ({}) needs {maximum_constraint_slot_demand} slots but only {ACTIVE_HANGUL_SLOT_COUNT} are active",
        maximum_constraint.id,
        maximum_constraint.variant,
    );

    let mut preassigned = BTreeMap::new();
    merge_preassignments(
        &mut preassigned,
        CodeOwner::DialogueDynamic,
        inputs.dynamic_inputs.canonical_dynamic_codes(),
        "canonical dynamic strings",
    )?;
    merge_preassignments(
        &mut preassigned,
        CodeOwner::ChapterTitle,
        &inputs.chapter_intro.title_glyph_codes,
        "chapter titles",
    )?;
    merge_preassignments(
        &mut preassigned,
        CodeOwner::FixedUi,
        inputs.choice_glyph_codes,
        "resident choice labels",
    )?;
    merge_preassignments(
        &mut preassigned,
        CodeOwner::FixedUi,
        inputs.unit_selection_help.preassigned_glyph_codes(),
        "unit-selection help codes fixed by its dialogue",
    )?;
    merge_preassignments(
        &mut preassigned,
        CodeOwner::OptionsTable,
        inputs.options_glyph_codes,
        "resident options labels",
    )?;
    ensure!(
        preassigned.values().all(|code| active_codes.contains(code)),
        "consumer codebook preassigns a reserved font code"
    );

    let graph = ConflictGraph::from_lifetimes(&lifetimes, preassigned.keys().copied());
    let mut forbidden = forbidden_codes_by_glyph(&lifetimes);
    for (glyph, codes) in inputs.unit_selection_help.forbidden_codes_by_glyph() {
        forbidden
            .entry(GlyphKey {
                owner: CodeOwner::FixedUi,
                glyph: *glyph,
            })
            .or_default()
            .extend(codes);
    }
    let (assignments, coloring_strategy, color_split_count) =
        assign_codes(&graph, &forbidden, &preassigned, &active_codes)?;
    verify_assignment(&graph, &forbidden, &preassigned, &assignments)?;

    let static_lifetimes = lifetimes
        .iter()
        .filter(|lifetime| lifetime.emit_static_page)
        .collect::<Vec<_>>();
    ensure!(
        static_lifetimes.len() <= inputs.available_page_count,
        "consumer codebook needs {} static pages but only {} reclaimable pages remain",
        static_lifetimes.len(),
        inputs.available_page_count
    );
    let last_page_exclusive = usize::from(inputs.first_physical_page)
        .checked_add(static_lifetimes.len())
        .context("consumer static page range overflow")?;
    ensure!(
        last_page_exclusive <= usize::from(MAXIMUM_CHR_PAGE_COUNT),
        "consumer static pages exceed mapper 165 CHR capacity"
    );

    let pages = static_lifetimes
        .into_iter()
        .enumerate()
        .map(|(index, lifetime)| {
            let physical_page = inputs
                .first_physical_page
                .checked_add(u8::try_from(index).context("consumer page index overflow")?)
                .context("consumer physical page overflow")?;
            let page_assignments = lifetime
                .target_glyphs
                .iter()
                .map(|glyph| {
                    Ok((
                        *glyph,
                        assignments
                            .get(glyph)
                            .copied()
                            .with_context(|| format!("consumer assignment lost {glyph:?}"))?,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>>>()?;
            let glyphs_by_code = page_assignments
                .iter()
                .map(|(key, code)| (*code, key.glyph))
                .collect::<BTreeMap<_, _>>();
            ensure!(
                glyphs_by_code.len() == page_assignments.len(),
                "consumer page {} assigns one code to multiple visible glyph sources",
                lifetime.id
            );
            let mut bytes = build_font_page_by_code(inputs.source_font_page, &glyphs_by_code)?;
            let pair_projection = RightFontPageProjection::for_screen_roles(
                inputs.source_chr,
                &lifetime.screen_roles,
                0,
            )?;
            pair_projection.apply_to_page(&mut bytes)?;
            let mapper_register = encode_chr_page_register(physical_page)?;
            Ok(StaticConsumerPage {
                id: lifetime.id,
                variant: lifetime.variant,
                screen_roles: lifetime.screen_roles.clone(),
                domain_ids: lifetime.domain_ids.clone(),
                target_glyph_count: lifetime.target_glyphs.len(),
                preserved_active_code_count: lifetime.preserved_active_codes.len(),
                slot_demand: lifetime.target_glyphs.len() + lifetime.preserved_active_codes.len(),
                physical_page,
                mapper_register,
                mapper_route: pair_projection.encode_mapper_route(mapper_register)?,
                assignment_sha1: assignment_sha1(&page_assignments),
                page_sha1: sha1_hex(&bytes),
                assignments: page_assignments,
                bytes,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let fixed_menu_page = pages
        .iter()
        .find(|page| page.id == "unit_command_menu")
        .context("consumer codebook lost the fixed-menu page")?;
    ensure_options_parent_assignments(&fixed_menu_page.assignments, inputs.options_glyph_codes)?;
    let physical_code_count = assignments.values().copied().collect::<BTreeSet<_>>().len();

    Ok(ConsumerCodebookPlan {
        schema: 1,
        strategy: "prebuild fixed-content command, fixed-menu, map-menu, chapter-save, and ending pages; keep dialogue-dynamic, chapter-title, and save-choice producer codes fixed, and require KTX1 runtime projection only for variable unit, item, and shop consumers",
        glyph_count: graph.glyph_count(),
        conflict_edge_count: graph.edge_count(),
        preassigned_glyph_count: preassigned.len(),
        canonical_dynamic_glyph_count: inputs.dynamic_inputs.canonical_dynamic_codes().len(),
        chapter_title_fixed_glyph_count: inputs.chapter_intro.title_glyph_codes.len(),
        physical_code_count,
        active_code_ceiling: ACTIVE_HANGUL_SLOT_COUNT,
        constraint_count: lifetimes.len(),
        maximum_constraint_slot_demand,
        static_page_count: pages.len(),
        first_physical_page: inputs.first_physical_page,
        available_page_count: inputs.available_page_count,
        assignment_sha1: assignment_sha1(&assignments),
        coloring_strategy,
        color_split_count,
        pages,
        every_preassignment_preserved: true,
        every_constraint_is_injective: true,
        every_preserved_code_avoided: true,
        static_pages_fit_reclaimable_tail: true,
        page_bytes_planned: true,
    })
}

/// The speed selector remains over its parent options window.  Rebinding only the new fast/slow
/// labels would make the already-installed parent strings decode through unrelated fixed-UI
/// codes, so every resident options glyph must survive at its original table code.
fn ensure_options_parent_assignments(
    assignments: &BTreeMap<GlyphKey, u8>,
    expected: &BTreeMap<char, u8>,
) -> Result<()> {
    ensure!(
        expected.iter().all(|(glyph, code)| {
            assignments.get(&GlyphKey {
                owner: CodeOwner::OptionsTable,
                glyph: *glyph,
            }) == Some(code)
        }),
        "fixed-menu page lost a resident options-label code assignment"
    );
    Ok(())
}

fn ensure_unit_command_assignments(
    assignments: &BTreeMap<GlyphKey, u8>,
    required_fixed_ui_glyphs: &BTreeSet<char>,
    options_glyph_codes: &BTreeMap<char, u8>,
) -> Result<()> {
    ensure_owned_glyphs(
        assignments,
        CodeOwner::FixedUi,
        required_fixed_ui_glyphs,
        "unit-command",
    )?;
    ensure_options_parent_assignments(assignments, options_glyph_codes)?;
    Ok(())
}

fn ensure_owned_glyphs(
    assignments: &BTreeMap<GlyphKey, u8>,
    owner: CodeOwner,
    required: &BTreeSet<char>,
    page_id: &str,
) -> Result<()> {
    ensure!(
        required
            .iter()
            .all(|glyph| assignments.contains_key(&GlyphKey {
                owner,
                glyph: *glyph,
            })),
        "consumer page {page_id} lost a required {owner:?} glyph"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_menu_page_retains_resident_options_code_assignments() {
        let expected = BTreeMap::from([('니', 0x3A), ('메', 0x3B)]);
        let assignments = BTreeMap::from([
            (
                GlyphKey {
                    owner: CodeOwner::OptionsTable,
                    glyph: '니',
                },
                0x3A,
            ),
            (
                GlyphKey {
                    owner: CodeOwner::OptionsTable,
                    glyph: '메',
                },
                0x3B,
            ),
            // The same visible glyph may legitimately have a different code in another storage
            // producer.  It must not replace the options-table ownership above.
            (
                GlyphKey {
                    owner: CodeOwner::FixedUi,
                    glyph: '니',
                },
                0x0B,
            ),
        ]);

        ensure_options_parent_assignments(&assignments, &expected).unwrap();

        let only_unrelated_owner = BTreeMap::from([(
            GlyphKey {
                owner: CodeOwner::FixedUi,
                glyph: '니',
            },
            0x0B,
        )]);
        assert!(
            ensure_options_parent_assignments(&only_unrelated_owner, &expected)
                .unwrap_err()
                .to_string()
                .contains("resident options-label")
        );
    }

    #[test]
    fn unit_command_page_rejects_a_missing_fixed_ui_surface_glyph() {
        let required = BTreeSet::from(['공', '격']);
        let assignments = BTreeMap::from([(
            GlyphKey {
                owner: CodeOwner::FixedUi,
                glyph: '공',
            },
            0x20,
        )]);

        assert!(
            ensure_unit_command_assignments(&assignments, &required, &BTreeMap::new())
                .unwrap_err()
                .to_string()
                .contains("lost a required FixedUi glyph")
        );
    }

    #[test]
    fn ending_page_rejects_a_missing_chapter_title_glyph() {
        let assignments = BTreeMap::from([(
            GlyphKey {
                owner: CodeOwner::FixedUi,
                glyph: '턴',
            },
            0x20,
        )]);

        assert!(
            ensure_owned_glyphs(
                &assignments,
                CodeOwner::ChapterTitle,
                &BTreeSet::from(['장']),
                "ending",
            )
            .unwrap_err()
            .to_string()
            .contains("lost a required ChapterTitle glyph")
        );
    }
}
