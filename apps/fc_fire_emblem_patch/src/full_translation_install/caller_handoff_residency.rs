//! 원본 호출자가 E7 뒤에 다른 대사 레코드를 고를 때 남아 있는 화면 행의 수명이다.
//!
//! E7은 대사 해석을 끝낼 뿐 PPU 네임테이블을 지우지 않는다. 아이템 결과처럼 앞
//! 레코드의 줄을 계속 보여 주는 경로는 후속 코드북에서도 그 글리프 코드를 보존한다.
//! 상점·보관소처럼 새 레코드가 앞줄을 대체하는 경로는 그 합집합을 코드북에 억지로
//! 넣지 않고, 런타임이 새 글꼴 합성을 마친 뒤 앞줄을 지우도록 레코드 모집단을 넘긴다.
//! 반대로 저장 완료 안내처럼 직접 진입하면서 원본의 선택창·초상화를 유지하는 레코드는
//! 같은 디렉터리 정책으로 앞줄 삭제를 막는다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::bind_save_complete_dialogue_records,
    dialogue_assets::MainDialogueDisplayPlan,
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, active_hangul_codes},
    item_flow::{item_use_result_dialogue_sequences, validate_item_lifetime_source},
    mapper165::battle_codebook_plan::{GlyphWorkset, GlyphWorksetPagePlan},
    rom::Rom,
    shop_flow::ShopItemCompositionSource,
};

use super::{storage_residency::StorageDialogueSourcePlan, transition_residency::merge_worksets};

const SHOP_AND_ITEM_DIALOGUE_TABLE_ID: &str = "shop-and-item-dialogue";

#[derive(Clone, Debug)]
pub(super) struct CallerHandoffLifetimeWorksets {
    pub(super) role: &'static str,
    pub(super) record_ids: Vec<String>,
    pub(super) workset_indices: Vec<usize>,
    pub(super) kind: CallerHandoffLifetimeKind,
    pub(super) record_workset_indices: Vec<Vec<usize>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CallerHandoffLifetimeKind {
    OrderedPath,
    ReplacePreviousRows,
}

#[derive(Clone)]
struct CallerHandoffIdentityRequirement {
    role: &'static str,
    workset_indices: Vec<usize>,
    retained_glyphs: BTreeSet<char>,
}

pub(super) struct CallerHandoffLifetimeBindings {
    lifetimes: Vec<CallerHandoffLifetimeWorksets>,
    record_line_policies: RecordLinePolicyBindings,
    item_use_lifetime_count: usize,
    shop_lifetime_count: usize,
    storage_lifetime_count: usize,
}

impl CallerHandoffLifetimeBindings {
    pub(super) fn lifetimes(&self) -> &[CallerHandoffLifetimeWorksets] {
        &self.lifetimes
    }

    pub(super) fn record_line_policies(&self) -> &RecordLinePolicyBindings {
        &self.record_line_policies
    }
}

/// 레코드가 열린 경로별로 직전 물리 행을 유지할지 교체할지를 한 곳에서 결속한다.
///
/// E7 호출자 전이는 기본적으로 앞줄을 유지하며, 상점·보관소 레코드만 교체한다.
/// 직접 진입은 기본적으로 앞줄을 교체하며, 원본 상태기가 표시 수명을 이어 가는
/// 레코드만 유지한다. 두 모집단은 빌드 때 레코드 디렉터리의 서로 다른 비트가 된다.
pub(super) struct RecordLinePolicyBindings {
    by_record_id: BTreeMap<String, RecordLinePolicy>,
}

impl RecordLinePolicyBindings {
    fn bind_complete_population(
        display: &MainDialogueDisplayPlan,
        caller_handoff_replacement_record_ids: &BTreeSet<String>,
        direct_entry_retention_record_ids: &BTreeSet<String>,
    ) -> Result<Self> {
        let dialogue_record_ids = display.record_ids.iter().cloned().collect::<BTreeSet<_>>();
        ensure!(
            dialogue_record_ids.len() == display.record_ids.len(),
            "dialogue record population contains a duplicate identity"
        );
        ensure!(
            caller_handoff_replacement_record_ids.is_subset(&dialogue_record_ids),
            "caller-handoff row-replacement policy contains a record outside the dialogue population"
        );
        ensure!(
            direct_entry_retention_record_ids.is_subset(&dialogue_record_ids),
            "direct-entry row-retention policy contains a record outside the dialogue population"
        );
        let by_record_id = display
            .record_ids
            .iter()
            .map(|record_id| {
                (
                    record_id.clone(),
                    RecordLinePolicy {
                        direct_entry: if direct_entry_retention_record_ids.contains(record_id) {
                            PreviousPhysicalRows::Retain
                        } else {
                            PreviousPhysicalRows::Replace
                        },
                        caller_handoff: if caller_handoff_replacement_record_ids.contains(record_id)
                        {
                            PreviousPhysicalRows::Replace
                        } else {
                            PreviousPhysicalRows::Retain
                        },
                        // E4/E6는 원본 대사 상태기 안의 명시적 연결이며, 그려 둔 앞줄을
                        // 다음 레코드가 이어 쓴다. 이 모드도 타입에 넣어 암묵 기본값으로
                        // 남기지 않는다.
                        published_transition: PreviousPhysicalRows::Retain,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        ensure!(
            by_record_id.keys().cloned().collect::<BTreeSet<_>>() == dialogue_record_ids,
            "record-line policy does not cover the complete dialogue record population"
        );
        Ok(Self { by_record_id })
    }

    pub(super) fn by_record_id(&self) -> &BTreeMap<String, RecordLinePolicy> {
        &self.by_record_id
    }

    fn count(
        &self,
        mode: fn(RecordLinePolicy) -> PreviousPhysicalRows,
        rows: PreviousPhysicalRows,
    ) -> usize {
        self.by_record_id
            .values()
            .copied()
            .filter(|policy| mode(*policy) == rows)
            .count()
    }

    pub(super) fn direct_entry_retention_count(&self) -> usize {
        self.count(|policy| policy.direct_entry, PreviousPhysicalRows::Retain)
    }

    pub(super) fn caller_handoff_replacement_count(&self) -> usize {
        self.count(
            |policy| policy.caller_handoff,
            PreviousPhysicalRows::Replace,
        )
    }

    pub(super) fn published_transition_replacement_count(&self) -> usize {
        self.count(
            |policy| policy.published_transition,
            PreviousPhysicalRows::Replace,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PreviousPhysicalRows {
    Retain,
    Replace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RecordLinePolicy {
    pub(super) direct_entry: PreviousPhysicalRows,
    pub(super) caller_handoff: PreviousPhysicalRows,
    pub(super) published_transition: PreviousPhysicalRows,
}

#[derive(Serialize)]
pub(super) struct CallerHandoffResidencyPlan {
    strategy: &'static str,
    source_lifetime_count: usize,
    item_use_lifetime_count: usize,
    shop_lifetime_count: usize,
    storage_lifetime_count: usize,
    source_record_count: usize,
    overlapping_workset_count: usize,
    maximum_lifetimes_per_workset: usize,
    maximum_lifetime_record_count: usize,
    maximum_lifetime_workset_count: usize,
    maximum_lifetime_slot_demand: usize,
    maximum_augmented_workset_slot_demand: usize,
    caller_handoff_replacement_record_count: usize,
    direct_entry_retention_record_count: usize,
    source_state_machine_lifetimes_bound: bool,
    every_retained_successor_contains_predecessor_visible_demand: bool,
    every_selected_page_preserves_retained_code_identity: bool,
    replacement_records_are_excluded_from_codebook_union: bool,
    #[serde(skip)]
    identity_requirements: Vec<CallerHandoffIdentityRequirement>,
    #[serde(skip)]
    pub(super) augmented_worksets: Vec<GlyphWorkset>,
}

impl CallerHandoffResidencyPlan {
    pub(super) fn bind_selected_codebook(&mut self, codebook: &GlyphWorksetPagePlan) -> Result<()> {
        ensure!(
            self.augmented_worksets.len() == codebook.workset_page_indices.len(),
            "caller-handoff residency lost a workset-to-codebook route"
        );
        for requirement in &self.identity_requirements {
            ensure!(
                !requirement.workset_indices.is_empty(),
                "{} has no visible workset",
                requirement.role
            );
            for glyph in &requirement.retained_glyphs {
                let mut expected = None;
                let mut containing_workset_count = 0;
                for workset_index in &requirement.workset_indices {
                    if !self.augmented_worksets[*workset_index]
                        .target_glyphs
                        .contains(glyph)
                    {
                        continue;
                    }
                    containing_workset_count += 1;
                    let page_index = codebook.workset_page_indices[*workset_index];
                    let assignments = codebook.page_assignments.get(page_index).with_context(|| {
                        format!(
                            "{} workset {workset_index} selects missing codebook page {page_index}",
                            requirement.role
                        )
                    })?;
                    let code = assignments.get(glyph).with_context(|| {
                        format!(
                            "{} selected page {page_index} lost caller-handoff glyph {glyph:?}",
                            requirement.role
                        )
                    })?;
                    let expected_code = expected.get_or_insert((*code, page_index));
                    ensure!(
                        expected_code.0 == *code,
                        "{} changes caller-handoff glyph {glyph:?} from code {:02X} between selected pages {} and {page_index}",
                        requirement.role,
                        expected_code.0,
                        expected_code.1,
                    );
                }
                ensure!(
                    containing_workset_count > 0,
                    "{} retained glyph {glyph:?} has no visible workset",
                    requirement.role
                );
            }
        }
        self.every_selected_page_preserves_retained_code_identity = true;
        Ok(())
    }

    pub(super) fn retained_rows_share_codebook(&self) -> bool {
        self.source_state_machine_lifetimes_bound
            && self.every_retained_successor_contains_predecessor_visible_demand
            && self.every_selected_page_preserves_retained_code_identity
            && self.replacement_records_are_excluded_from_codebook_union
    }
}

pub(super) fn bind_caller_handoff_lifetime_worksets(
    source: &Rom,
    display: &MainDialogueDisplayPlan,
    shop: &ShopItemCompositionSource,
    storage: &StorageDialogueSourcePlan,
) -> Result<CallerHandoffLifetimeBindings> {
    validate_item_lifetime_source(source)?;
    let item_sequences = item_use_result_dialogue_sequences();
    ensure!(
        item_sequences.len() == 18
            && item_sequences.contains(&vec![0x1A, 0x1D])
            && item_sequences
                .iter()
                .all(|sequence| sequence.first() == Some(&0x1A)),
        "item-use caller-handoff result population changed"
    );
    let item_use_lifetime_count = item_sequences.len();
    let mut record_groups = item_sequences
        .into_iter()
        .map(|sequence| CallerHandoffRecordGroup {
            role: "item-use result caller handoff",
            kind: CallerHandoffLifetimeKind::OrderedPath,
            record_ids: sequence
                .into_iter()
                .map(|index| shop_and_item_dialogue_record_id(usize::from(index)))
                .collect(),
        })
        .collect::<Vec<_>>();

    let shop_lifetime_count = shop.dialogue_lifetime_record_indices_by_facility().len();
    ensure!(
        shop_lifetime_count == shop.selling_facilities().len(),
        "shop caller-handoff lifetimes lost a selling facility"
    );
    record_groups.extend(
        shop.dialogue_lifetime_record_indices_by_facility()
            .values()
            .map(|record_indices| CallerHandoffRecordGroup {
                role: "selling-facility caller handoff",
                kind: CallerHandoffLifetimeKind::ReplacePreviousRows,
                record_ids: record_indices
                    .iter()
                    .copied()
                    .map(shop_and_item_dialogue_record_id)
                    .collect(),
            }),
    );

    let storage_groups = storage.caller_handoff_record_groups();
    let storage_lifetime_count = storage_groups.len();
    record_groups.extend(storage_groups.into_iter().map(|(role, record_ids)| {
        CallerHandoffRecordGroup {
            role,
            kind: CallerHandoffLifetimeKind::ReplacePreviousRows,
            record_ids: record_ids.iter().cloned().collect(),
        }
    }));

    let lifetimes = bind_record_groups_to_worksets(display, &record_groups)?;
    ensure!(
        lifetimes.len() == item_use_lifetime_count + shop_lifetime_count + storage_lifetime_count,
        "caller-handoff lifetime binding lost a source-derived group"
    );
    let retained_record_ids = lifetimes
        .iter()
        .filter(|lifetime| lifetime.kind == CallerHandoffLifetimeKind::OrderedPath)
        .flat_map(|lifetime| lifetime.record_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let replacement_record_ids = lifetimes
        .iter()
        .filter(|lifetime| lifetime.kind == CallerHandoffLifetimeKind::ReplacePreviousRows)
        .flat_map(|lifetime| lifetime.record_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    ensure!(
        retained_record_ids.is_disjoint(&replacement_record_ids),
        "a caller-handoff record cannot both retain and replace its previous rows"
    );
    let save_complete_records = bind_save_complete_dialogue_records(source)?;
    let direct_entry_retention_record_ids =
        BTreeSet::from([save_complete_records.power_off_notice.to_owned()]);
    let record_line_policies = RecordLinePolicyBindings::bind_complete_population(
        display,
        &replacement_record_ids,
        &direct_entry_retention_record_ids,
    )?;
    Ok(CallerHandoffLifetimeBindings {
        lifetimes,
        record_line_policies,
        item_use_lifetime_count,
        shop_lifetime_count,
        storage_lifetime_count,
    })
}

pub(super) fn plan_caller_handoff_residency(
    worksets: &[GlyphWorkset],
    bindings: &CallerHandoffLifetimeBindings,
) -> Result<CallerHandoffResidencyPlan> {
    ensure!(
        !worksets.is_empty(),
        "caller-handoff residency has no visible worksets"
    );
    let mut lifetimes_by_workset = vec![Vec::new(); worksets.len()];
    let mut augmentation_demands_by_workset = vec![Vec::new(); worksets.len()];
    let mut augmentation_requirements = Vec::new();
    let mut identity_requirements = Vec::new();
    let mut ordered_workset_indices = BTreeSet::new();
    let mut source_record_ids = BTreeSet::new();
    let mut maximum_lifetime_record_count = 0;
    let mut maximum_lifetime_workset_count = 0;
    let mut maximum_lifetime_slot_demand = 0;

    for (lifetime_index, lifetime) in bindings.lifetimes.iter().enumerate() {
        ensure!(
            !lifetime.record_ids.is_empty()
                && !lifetime.workset_indices.is_empty()
                && lifetime.record_ids.len() == lifetime.record_workset_indices.len()
                && lifetime
                    .record_workset_indices
                    .iter()
                    .all(|indices| !indices.is_empty()),
            "{} has no source record or visible workset",
            lifetime.role
        );
        maximum_lifetime_record_count =
            maximum_lifetime_record_count.max(lifetime.record_ids.len());
        maximum_lifetime_workset_count =
            maximum_lifetime_workset_count.max(lifetime.workset_indices.len());
        source_record_ids.extend(lifetime.record_ids.iter().cloned());
        for workset_index in &lifetime.workset_indices {
            let targets = lifetimes_by_workset
                .get_mut(*workset_index)
                .with_context(|| {
                    format!(
                        "{} selects missing visible workset {workset_index}",
                        lifetime.role
                    )
                })?;
            targets.push(lifetime_index);
        }
        match lifetime.kind {
            CallerHandoffLifetimeKind::ReplacePreviousRows => {}
            CallerHandoffLifetimeKind::OrderedPath => {
                let full_lifetime_demand = merge_worksets(
                    lifetime
                        .workset_indices
                        .iter()
                        .map(|workset_index| &worksets[*workset_index]),
                )
                .with_context(|| {
                    format!("merge {} records {:?}", lifetime.role, lifetime.record_ids)
                })?;
                let slot_demand = workset_slot_demand(&full_lifetime_demand);
                ensure!(
                    slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
                    "{} records {:?} need {slot_demand} active slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist",
                    lifetime.role,
                    lifetime.record_ids
                );
                maximum_lifetime_slot_demand = maximum_lifetime_slot_demand.max(slot_demand);
                let mut prefix_demand: Option<GlyphWorkset> = None;
                let mut prefix_workset_indices = Vec::new();
                for current_workset_indices in &lifetime.record_workset_indices {
                    let current_demand = merge_worksets(
                        current_workset_indices
                            .iter()
                            .map(|workset_index| &worksets[*workset_index]),
                    )
                    .with_context(|| format!("merge ordered {} record", lifetime.role))?;
                    let next_prefix_demand = match &prefix_demand {
                        Some(previous) => merge_worksets([previous, &current_demand].into_iter())?,
                        None => current_demand,
                    };
                    let prefix_slot_demand = workset_slot_demand(&next_prefix_demand);
                    ensure!(
                        prefix_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
                        "ordered {} prefix in {:?} needs {prefix_slot_demand} active slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist",
                        lifetime.role,
                        lifetime.record_ids
                    );
                    for workset_index in current_workset_indices {
                        augmentation_demands_by_workset[*workset_index]
                            .push(next_prefix_demand.clone());
                        augmentation_requirements
                            .push((*workset_index, next_prefix_demand.clone()));
                        ordered_workset_indices.insert(*workset_index);
                    }
                    let retained_glyphs = prefix_demand
                        .as_ref()
                        .map(|demand| demand.target_glyphs.clone())
                        .unwrap_or_else(|| next_prefix_demand.target_glyphs.clone());
                    let mut identity_worksets = prefix_workset_indices.clone();
                    identity_worksets.extend(current_workset_indices.iter().copied());
                    identity_requirements.push(CallerHandoffIdentityRequirement {
                        role: lifetime.role,
                        workset_indices: identity_worksets,
                        retained_glyphs,
                    });
                    prefix_workset_indices.extend(current_workset_indices.iter().copied());
                    prefix_demand = Some(next_prefix_demand);
                }
            }
        }
    }

    let overlapping_workset_count = lifetimes_by_workset
        .iter()
        .filter(|lifetimes| lifetimes.len() > 1)
        .count();
    let maximum_lifetimes_per_workset =
        lifetimes_by_workset.iter().map(Vec::len).max().unwrap_or(0);
    let mut augmented_worksets = worksets
        .iter()
        .enumerate()
        .map(|(workset_index, workset)| {
            merge_worksets(
                std::iter::once(workset)
                    .chain(augmentation_demands_by_workset[workset_index].iter()),
            )
            .with_context(|| format!("augment caller-handoff workset {workset_index}"))
        })
        .collect::<Result<Vec<_>>>()?;
    fix_ordered_path_code_identity(&mut augmented_worksets, &ordered_workset_indices)?;
    let (maximum_augmented_workset_index, maximum_augmented_workset_slot_demand) =
        augmented_worksets
            .iter()
            .enumerate()
            .map(|(index, workset)| (index, workset_slot_demand(workset)))
            .max_by_key(|(_, slot_demand)| *slot_demand)
            .unwrap_or((0, 0));
    let maximum_workset_lifetimes = lifetimes_by_workset[maximum_augmented_workset_index]
        .iter()
        .map(|lifetime_index| {
            let lifetime = &bindings.lifetimes[*lifetime_index];
            format!("{} {:?}", lifetime.role, lifetime.record_ids)
        })
        .collect::<Vec<_>>();
    ensure!(
        maximum_augmented_workset_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "overlapping caller-handoff lifetimes need {maximum_augmented_workset_slot_demand} active slots in workset {maximum_augmented_workset_index} from {maximum_workset_lifetimes:?}, but only {ACTIVE_HANGUL_SLOT_COUNT} exist"
    );
    for (workset_index, demand) in &augmentation_requirements {
        ensure!(
            workset_contains(&augmented_worksets[*workset_index], demand),
            "caller-handoff workset {workset_index} does not carry its path-specific visible demand"
        );
    }

    Ok(CallerHandoffResidencyPlan {
        strategy: "derive ordered item-result paths plus row-replacement and row-retention record families from their source state machines; propagate only retained path prefixes into successors; color retained glyphs once across overlapping paths; leave shop and storage families out of the codebook union so their source-bound caller-handoff policy can clear previous rows after font composition; encode the chapter-save prompt-to-notice successor as a direct-entry row-retention policy because its choice window and portrait remain physically visible",
        source_lifetime_count: bindings.lifetimes.len(),
        item_use_lifetime_count: bindings.item_use_lifetime_count,
        shop_lifetime_count: bindings.shop_lifetime_count,
        storage_lifetime_count: bindings.storage_lifetime_count,
        source_record_count: source_record_ids.len(),
        overlapping_workset_count,
        maximum_lifetimes_per_workset,
        maximum_lifetime_record_count,
        maximum_lifetime_workset_count,
        maximum_lifetime_slot_demand,
        maximum_augmented_workset_slot_demand,
        caller_handoff_replacement_record_count: bindings
            .record_line_policies
            .caller_handoff_replacement_count(),
        direct_entry_retention_record_count: bindings
            .record_line_policies
            .direct_entry_retention_count(),
        source_state_machine_lifetimes_bound: true,
        every_retained_successor_contains_predecessor_visible_demand: true,
        every_selected_page_preserves_retained_code_identity: false,
        replacement_records_are_excluded_from_codebook_union: true,
        identity_requirements,
        augmented_worksets,
    })
}

fn fix_ordered_path_code_identity(
    worksets: &mut [GlyphWorkset],
    ordered_workset_indices: &BTreeSet<usize>,
) -> Result<()> {
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let mut neighboring_glyphs = BTreeMap::<char, BTreeSet<char>>::new();
    let mut forbidden_codes = BTreeMap::<char, BTreeSet<u8>>::new();
    let mut assigned_codes = BTreeMap::<char, u8>::new();

    for workset_index in ordered_workset_indices {
        let workset = worksets.get(*workset_index).with_context(|| {
            format!("ordered caller-handoff workset {workset_index} is missing")
        })?;
        let glyphs = workset.target_glyphs.iter().copied().collect::<Vec<_>>();
        for glyph in &glyphs {
            neighboring_glyphs
                .entry(*glyph)
                .or_default()
                .extend(glyphs.iter().copied().filter(|neighbor| neighbor != glyph));
            forbidden_codes
                .entry(*glyph)
                .or_default()
                .extend(workset.preserved_active_codes.iter().copied());
        }
        for (glyph, code) in &workset.fixed_glyph_codes {
            if let Some(previous) = assigned_codes.insert(*glyph, *code) {
                ensure!(
                    previous == *code,
                    "ordered caller-handoff glyph {glyph:?} already uses code {previous:02X}, not {code:02X}"
                );
            }
        }
    }

    for (glyph, neighbors) in &neighboring_glyphs {
        if let Some(code) = assigned_codes.get(glyph) {
            ensure!(
                !forbidden_codes[glyph].contains(code)
                    && neighbors
                        .iter()
                        .all(|neighbor| assigned_codes.get(neighbor) != Some(code)),
                "ordered caller-handoff fixed code {code:02X} conflicts for glyph {glyph:?}"
            );
        }
    }

    let mut unassigned = neighboring_glyphs
        .keys()
        .copied()
        .filter(|glyph| !assigned_codes.contains_key(glyph))
        .collect::<Vec<_>>();
    unassigned.sort_by(|left, right| {
        neighboring_glyphs[right]
            .len()
            .cmp(&neighboring_glyphs[left].len())
            .then_with(|| {
                forbidden_codes[right]
                    .len()
                    .cmp(&forbidden_codes[left].len())
            })
            .then_with(|| left.cmp(right))
    });
    for glyph in unassigned {
        let code = active_codes
            .iter()
            .copied()
            .find(|code| {
                !forbidden_codes[&glyph].contains(code)
                    && neighboring_glyphs[&glyph]
                        .iter()
                        .all(|neighbor| assigned_codes.get(neighbor) != Some(code))
            })
            .with_context(|| {
                format!("ordered caller-handoff glyph {glyph:?} has no physical code")
            })?;
        assigned_codes.insert(glyph, code);
    }

    for workset_index in ordered_workset_indices {
        let workset = &mut worksets[*workset_index];
        for glyph in &workset.target_glyphs {
            let code = assigned_codes[glyph];
            if let Some(previous) = workset.fixed_glyph_codes.insert(*glyph, code) {
                ensure!(
                    previous == code,
                    "ordered caller-handoff workset {workset_index} changes glyph {glyph:?} code from {previous:02X} to {code:02X}"
                );
            }
        }
    }
    Ok(())
}

struct CallerHandoffRecordGroup {
    role: &'static str,
    kind: CallerHandoffLifetimeKind,
    record_ids: Vec<String>,
}

fn bind_record_groups_to_worksets(
    display: &MainDialogueDisplayPlan,
    groups: &[CallerHandoffRecordGroup],
) -> Result<Vec<CallerHandoffLifetimeWorksets>> {
    let mut worksets_by_record = BTreeMap::<&str, Vec<usize>>::new();
    for (workset_index, page) in display.page_worksets.iter().enumerate() {
        worksets_by_record
            .entry(page.record_id.as_str())
            .or_default()
            .push(workset_index);
    }
    ensure!(
        display.record_ids.iter().all(|record_id| worksets_by_record
            .get(record_id.as_str())
            .is_some_and(|indices| !indices.is_empty())),
        "caller-handoff residency has a dialogue record without a visible workset"
    );

    groups
        .iter()
        .map(|group| {
            let unique_record_ids = group.record_ids.iter().collect::<BTreeSet<_>>();
            ensure!(
                !group.record_ids.is_empty() && unique_record_ids.len() == group.record_ids.len(),
                "{} has an empty or duplicate record identity",
                group.role
            );
            let record_workset_indices = group
                .record_ids
                .iter()
                .map(|record_id| {
                    worksets_by_record
                        .get(record_id.as_str())
                        .cloned()
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            let workset_indices = record_workset_indices
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            for record_id in &group.record_ids {
                ensure!(
                    worksets_by_record.contains_key(record_id.as_str()),
                    "{} source record {record_id} is missing from the dialogue display plan",
                    group.role
                );
            }
            ensure!(
                !workset_indices.is_empty(),
                "{} has no visible page workset",
                group.role
            );
            Ok(CallerHandoffLifetimeWorksets {
                role: group.role,
                record_ids: group.record_ids.clone(),
                workset_indices,
                kind: group.kind,
                record_workset_indices,
            })
        })
        .collect()
}

fn shop_and_item_dialogue_record_id(index: usize) -> String {
    format!("{SHOP_AND_ITEM_DIALOGUE_TABLE_ID}:{index:03}")
}

fn workset_slot_demand(workset: &GlyphWorkset) -> usize {
    workset.target_glyphs.len() + workset.preserved_active_codes.len()
}

fn workset_contains(container: &GlyphWorkset, demand: &GlyphWorkset) -> bool {
    demand.target_glyphs.is_subset(&container.target_glyphs)
        && demand
            .preserved_active_codes
            .is_subset(&container.preserved_active_codes)
        && demand
            .fixed_glyph_codes
            .iter()
            .all(|(glyph, code)| container.fixed_glyph_codes.get(glyph) == Some(code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dialogue_assets::MainDialoguePageWorkset,
        mapper165::battle_codebook_plan::plan_glyph_workset_page_upper_bound,
    };

    fn page(record_id: &str) -> MainDialoguePageWorkset {
        MainDialoguePageWorkset {
            record_id: record_id.to_owned(),
            page_index: 0,
            target_glyphs: BTreeSet::new(),
            dynamic_string_selectors: BTreeSet::new(),
            dynamic_string_selector_counts: BTreeMap::new(),
            dynamic_string_control_count: 0,
            source_reclaimable_active_codes: BTreeSet::new(),
            preserved_target_active_codes: BTreeSet::new(),
        }
    }

    fn workset(glyphs: &str) -> GlyphWorkset {
        GlyphWorkset {
            target_glyphs: glyphs.chars().collect(),
            preserved_active_codes: BTreeSet::new(),
            fixed_glyph_codes: BTreeMap::new(),
        }
    }

    fn bindings(
        display: &MainDialogueDisplayPlan,
        groups: &[CallerHandoffRecordGroup],
    ) -> CallerHandoffLifetimeBindings {
        CallerHandoffLifetimeBindings {
            lifetimes: bind_record_groups_to_worksets(display, groups).unwrap(),
            record_line_policies: RecordLinePolicyBindings::bind_complete_population(
                display,
                &groups
                    .iter()
                    .filter(|group| group.kind == CallerHandoffLifetimeKind::ReplacePreviousRows)
                    .flat_map(|group| group.record_ids.iter().cloned())
                    .collect(),
                &BTreeSet::new(),
            )
            .unwrap(),
            item_use_lifetime_count: groups.len(),
            shop_lifetime_count: 0,
            storage_lifetime_count: 0,
        }
    }

    #[test]
    fn item_result_population_contains_the_vulnerary_handoff() {
        let sequences = item_use_result_dialogue_sequences();
        assert_eq!(sequences.len(), 18);
        assert!(sequences.contains(&vec![0x1A, 0x1D]));
        assert!(sequences.contains(&vec![0x1A, 0x27, 0x28]));
    }

    #[test]
    fn every_record_gets_one_policy_for_all_three_entry_modes() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 3,
            record_ids: vec!["record:a".into(), "record:b".into(), "record:c".into()],
            page_worksets: vec![page("record:a"), page("record:b"), page("record:c")],
        };

        let policies = RecordLinePolicyBindings::bind_complete_population(
            &display,
            &BTreeSet::from(["record:b".to_owned()]),
            &BTreeSet::from(["record:c".to_owned()]),
        )
        .unwrap();

        assert_eq!(policies.by_record_id().len(), display.record_ids.len());
        assert_eq!(
            policies.by_record_id()["record:a"],
            RecordLinePolicy {
                direct_entry: PreviousPhysicalRows::Replace,
                caller_handoff: PreviousPhysicalRows::Retain,
                published_transition: PreviousPhysicalRows::Retain,
            }
        );
        assert_eq!(
            policies.by_record_id()["record:b"].caller_handoff,
            PreviousPhysicalRows::Replace
        );
        assert_eq!(
            policies.by_record_id()["record:c"].direct_entry,
            PreviousPhysicalRows::Retain
        );
        assert_eq!(policies.caller_handoff_replacement_count(), 1);
        assert_eq!(policies.direct_entry_retention_count(), 1);
        assert_eq!(policies.published_transition_replacement_count(), 0);
    }

    #[test]
    fn a_policy_for_an_unknown_record_is_rejected_before_encoding() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 1,
            record_ids: vec!["record:a".into()],
            page_worksets: vec![page("record:a")],
        };

        assert!(
            RecordLinePolicyBindings::bind_complete_population(
                &display,
                &BTreeSet::from(["record:outside".to_owned()]),
                &BTreeSet::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn overlapping_paths_propagate_the_predecessor_only_into_each_successor() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 3,
            record_ids: vec!["record:a".into(), "record:b".into(), "record:c".into()],
            page_worksets: vec![page("record:a"), page("record:b"), page("record:c")],
        };
        let groups = [
            CallerHandoffRecordGroup {
                role: "path a-b",
                kind: CallerHandoffLifetimeKind::OrderedPath,
                record_ids: vec!["record:a".into(), "record:b".into()],
            },
            CallerHandoffRecordGroup {
                role: "path a-c",
                kind: CallerHandoffLifetimeKind::OrderedPath,
                record_ids: vec!["record:a".into(), "record:c".into()],
            },
        ];
        let bindings = bindings(&display, &groups);
        let plan = plan_caller_handoff_residency(
            &[workset("가"), workset("나"), workset("다")],
            &bindings,
        )
        .unwrap();

        assert_eq!(
            plan.augmented_worksets[0].target_glyphs,
            "가".chars().collect()
        );
        assert_eq!(
            plan.augmented_worksets[1].target_glyphs,
            "가나".chars().collect()
        );
        assert_eq!(
            plan.augmented_worksets[2].target_glyphs,
            "가다".chars().collect()
        );
        assert_eq!(plan.overlapping_workset_count, 1);
        assert_eq!(plan.maximum_lifetimes_per_workset, 2);
    }

    #[test]
    fn every_successor_page_keeps_predecessor_glyphs_at_the_same_codes() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 3,
            record_ids: vec!["record:a".into(), "record:b".into(), "record:c".into()],
            page_worksets: vec![page("record:a"), page("record:b"), page("record:c")],
        };
        let groups = [
            CallerHandoffRecordGroup {
                role: "path a-b",
                kind: CallerHandoffLifetimeKind::OrderedPath,
                record_ids: vec!["record:a".into(), "record:b".into()],
            },
            CallerHandoffRecordGroup {
                role: "path a-c",
                kind: CallerHandoffLifetimeKind::OrderedPath,
                record_ids: vec!["record:a".into(), "record:c".into()],
            },
        ];
        let bindings = bindings(&display, &groups);
        let mut plan = plan_caller_handoff_residency(
            &[workset("가"), workset("나"), workset("다")],
            &bindings,
        )
        .unwrap();
        let codebook = plan_glyph_workset_page_upper_bound(&plan.augmented_worksets).unwrap();

        plan.bind_selected_codebook(&codebook).unwrap();

        assert!(plan.retained_rows_share_codebook());
        for successor in [1, 2] {
            let predecessor_page = codebook.workset_page_indices[0];
            let successor_page = codebook.workset_page_indices[successor];
            assert_eq!(
                codebook.page_assignments[predecessor_page][&'가'],
                codebook.page_assignments[successor_page][&'가']
            );
        }
    }

    #[test]
    fn three_step_path_compares_each_retained_glyph_only_after_it_appears() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 3,
            record_ids: vec!["record:a".into(), "record:b".into(), "record:c".into()],
            page_worksets: vec![page("record:a"), page("record:b"), page("record:c")],
        };
        let groups = [CallerHandoffRecordGroup {
            role: "path a-b-c",
            kind: CallerHandoffLifetimeKind::OrderedPath,
            record_ids: vec!["record:a".into(), "record:b".into(), "record:c".into()],
        }];
        let bindings = bindings(&display, &groups);
        let mut plan = plan_caller_handoff_residency(
            &[workset("가"), workset("나"), workset("다")],
            &bindings,
        )
        .unwrap();
        let codebook = plan_glyph_workset_page_upper_bound(&plan.augmented_worksets).unwrap();

        plan.bind_selected_codebook(&codebook).unwrap();

        assert!(plan.retained_rows_share_codebook());
        assert_eq!(
            plan.augmented_worksets[0].target_glyphs,
            workset("가").target_glyphs
        );
        assert_eq!(
            plan.augmented_worksets[1].target_glyphs,
            workset("가나").target_glyphs
        );
        assert_eq!(
            plan.augmented_worksets[2].target_glyphs,
            workset("가나다").target_glyphs
        );
    }
}
