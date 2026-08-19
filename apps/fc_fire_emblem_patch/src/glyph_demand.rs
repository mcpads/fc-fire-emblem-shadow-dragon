//! 번역 산출물의 글리프 수요를 모집단별로 세고, 함께 상주해야 하는 모집단이
//! 활성 슬롯 예산에 들어가는지와 어디를 줄이면 가장 싼지를 보고한다.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::sha1_hex;

pub struct GlyphDemandSummary {
    pub report_sha1: String,
    pub population_count: usize,
    pub coresident_set_count: usize,
    pub over_budget_set_names: Vec<String>,
}

pub fn analyze_glyph_demand(
    main_dialogue_workspace_path: &Path,
    fixed_text_workspace_path: &Path,
    population_specs: &[String],
    coresident_specs: &[String],
    slot_budget: usize,
    report_path: &Path,
) -> Result<GlyphDemandSummary> {
    let main_dialogue_workspace = fs::read(main_dialogue_workspace_path).with_context(|| {
        format!(
            "read main dialogue workspace {}",
            main_dialogue_workspace_path.display()
        )
    })?;
    let fixed_text_workspace = fs::read(fixed_text_workspace_path).with_context(|| {
        format!(
            "read fixed text workspace {}",
            fixed_text_workspace_path.display()
        )
    })?;
    let populations = population_specs
        .iter()
        .map(|spec| parse_named_list(spec))
        .collect::<Result<Vec<_>>>()?;
    let coresident = coresident_specs
        .iter()
        .map(|spec| parse_named_list(spec))
        .collect::<Result<Vec<_>>>()?;

    let report = build_report(
        &main_dialogue_workspace,
        &fixed_text_workspace,
        &populations,
        &coresident,
        slot_budget,
    )?;

    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize glyph-demand report")?;
    report_bytes.push(b'\n');
    let report_sha1 = sha1_hex(&report_bytes);
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(GlyphDemandSummary {
        report_sha1,
        population_count: report.populations.len(),
        coresident_set_count: report.coresident_sets.len(),
        over_budget_set_names: report
            .coresident_sets
            .iter()
            .filter(|set| !set.fits)
            .map(|set| set.name.clone())
            .collect(),
    })
}

/// 통계를 셀 한 덩어리의 번역문이다. 대사 레코드 하나, 고정 문자열 하나가 된다.
#[derive(Debug)]
pub(crate) struct TextUnit {
    pub(crate) id: String,
    pub(crate) text: String,
}

/// 이름 붙은 글리프 모집단이다. 대사 표 하나, 고정 문자열 표 하나, 또는 원본
/// 상태기가 고르는 레코드 묶음처럼 함께 판단할 단위로 만든다.
#[derive(Debug)]
pub(crate) struct GlyphPopulation {
    pub(crate) name: String,
    pub(crate) kind: &'static str,
    pub(crate) units: Vec<TextUnit>,
}

#[derive(Debug, Serialize)]
pub(crate) struct GlyphDemand {
    pub(crate) glyph: char,
    pub(crate) occurrence_count: usize,
    pub(crate) unit_count: usize,
    pub(crate) unit_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PopulationSummary {
    pub(crate) name: String,
    pub(crate) kind: &'static str,
    pub(crate) unit_count: usize,
    pub(crate) occurrence_count: usize,
    pub(crate) glyph_count: usize,
    pub(crate) glyphs: Vec<GlyphDemand>,
}

/// 주 대사 작업공간에서 통계에 필요한 부분만 읽는다. 나머지 필드는 원본 결속과
/// 재삽입이 소유하므로 여기서 다시 검사하지 않는다.
#[derive(Deserialize)]
struct MainDialogueWorkspaceView {
    records: Vec<MainDialogueRecordView>,
}

#[derive(Deserialize)]
struct MainDialogueRecordView {
    id: String,
    table_id: String,
    lines: Vec<MainDialogueLineView>,
}

#[derive(Deserialize)]
struct MainDialogueLineView {
    korean: String,
}

#[derive(Deserialize)]
struct FixedTextWorkspaceView {
    entries: Vec<FixedTextEntryView>,
}

#[derive(Deserialize)]
struct FixedTextEntryView {
    id: String,
    table_id: String,
    korean_markup: String,
}

pub(crate) fn main_dialogue_populations(workspace_bytes: &[u8]) -> Result<Vec<GlyphPopulation>> {
    let workspace: MainDialogueWorkspaceView =
        serde_json::from_slice(workspace_bytes).context("parse main dialogue workspace")?;
    Ok(group_units(
        "main_dialogue_table",
        workspace.records.into_iter().map(|record| {
            let text = record
                .lines
                .into_iter()
                .map(|line| line.korean)
                .collect::<String>();
            (
                record.table_id,
                TextUnit {
                    id: record.id,
                    text,
                },
            )
        }),
    ))
}

pub(crate) fn fixed_text_populations(workspace_bytes: &[u8]) -> Result<Vec<GlyphPopulation>> {
    let workspace: FixedTextWorkspaceView =
        serde_json::from_slice(workspace_bytes).context("parse fixed text workspace")?;
    Ok(group_units(
        "fixed_text_table",
        workspace.entries.into_iter().map(|entry| {
            (
                entry.table_id,
                TextUnit {
                    id: entry.id,
                    text: entry.korean_markup,
                },
            )
        }),
    ))
}

/// `NAME=MEMBER[,MEMBER...]` 명령줄 인자 하나다. 임시 모집단과 공존 집합이
/// 같은 형태를 쓴다.
#[derive(Debug)]
pub(crate) struct NamedList {
    pub(crate) name: String,
    pub(crate) members: Vec<String>,
}

pub(crate) fn parse_named_list(spec: &str) -> Result<NamedList> {
    let (name, members) = spec
        .split_once('=')
        .with_context(|| format!("{spec:?} is not a NAME=MEMBER[,MEMBER...] specification"))?;
    ensure!(
        !name.is_empty(),
        "{spec:?} is not a NAME=MEMBER[,MEMBER...] specification"
    );
    let members = members
        .split(',')
        .map(str::trim)
        .filter(|member| !member.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    ensure!(
        !members.is_empty(),
        "{spec:?} is not a NAME=MEMBER[,MEMBER...] specification"
    );
    Ok(NamedList {
        name: name.to_owned(),
        members,
    })
}

#[derive(Debug, Serialize)]
pub(crate) struct GlyphDemandReport {
    pub(crate) schema: u8,
    pub(crate) source_sha1: String,
    pub(crate) slot_budget: usize,
    pub(crate) populations: Vec<PopulationSummary>,
    pub(crate) coresident_sets: Vec<CoresidentSummary>,
}

pub(crate) fn build_report(
    main_dialogue_workspace: &[u8],
    fixed_text_workspace: &[u8],
    population_specs: &[NamedList],
    coresident_specs: &[NamedList],
    slot_budget: usize,
) -> Result<GlyphDemandReport> {
    let source_sha1 = bind_workspace_source(main_dialogue_workspace, fixed_text_workspace)?;

    let mut populations = main_dialogue_populations(main_dialogue_workspace)?;
    populations.extend(fixed_text_populations(fixed_text_workspace)?);
    for spec in population_specs {
        populations.push(select_population(&spec.name, &populations, &spec.members)?);
    }

    let summaries = populations
        .iter()
        .map(summarize_population)
        .collect::<Vec<_>>();
    let summary_by_name = summaries
        .iter()
        .map(|summary| (summary.name.as_str(), summary))
        .collect::<BTreeMap<_, _>>();

    let coresident_sets = coresident_specs
        .iter()
        .map(|spec| {
            let members = spec
                .members
                .iter()
                .map(|name| {
                    summary_by_name
                        .get(name.as_str())
                        .copied()
                        .with_context(|| format!("no glyph population is named {name}"))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(evaluate_coresident_set(&spec.name, &members, slot_budget))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(GlyphDemandReport {
        schema: 1,
        source_sha1,
        slot_budget,
        populations: summaries,
        coresident_sets,
    })
}

fn bind_workspace_source(
    main_dialogue_workspace: &[u8],
    fixed_text_workspace: &[u8],
) -> Result<String> {
    #[derive(Deserialize)]
    struct SourceBinding {
        source_sha1: String,
    }

    let dialogue: SourceBinding = serde_json::from_slice(main_dialogue_workspace)
        .context("read main dialogue workspace source binding")?;
    let fixed: SourceBinding = serde_json::from_slice(fixed_text_workspace)
        .context("read fixed text workspace source binding")?;
    ensure!(
        dialogue.source_sha1 == fixed.source_sha1,
        "translation workspaces must bind the same source: {} and {}",
        dialogue.source_sha1,
        fixed.source_sha1
    );
    Ok(dialogue.source_sha1)
}

/// 원본 상태기가 고르는 레코드 묶음처럼 표 경계와 무관한 모집단을 만든다.
pub(crate) fn select_population(
    name: &str,
    populations: &[GlyphPopulation],
    unit_ids: &[String],
) -> Result<GlyphPopulation> {
    let available = populations
        .iter()
        .flat_map(|population| &population.units)
        .map(|unit| (unit.id.as_str(), unit.text.as_str()))
        .collect::<BTreeMap<_, _>>();

    let units = unit_ids
        .iter()
        .map(|unit_id| {
            let text = available
                .get(unit_id.as_str())
                .with_context(|| format!("no translation unit is named {unit_id}"))?;
            Ok(TextUnit {
                id: unit_id.clone(),
                text: (*text).to_owned(),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(GlyphPopulation {
        name: name.to_owned(),
        kind: "selected_units",
        units,
    })
}

fn group_units(
    kind: &'static str,
    units: impl Iterator<Item = (String, TextUnit)>,
) -> Vec<GlyphPopulation> {
    let mut grouped = BTreeMap::<String, Vec<TextUnit>>::new();
    for (group, unit) in units {
        grouped.entry(group).or_default().push(unit);
    }
    grouped
        .into_iter()
        .map(|(name, units)| GlyphPopulation { name, kind, units })
        .collect()
}

/// 이 글리프를 번역문에서 없애면 공존 집합이 슬롯 하나만큼 줄어든다. 등장이
/// 적을수록 손볼 문장이 적으므로 싼 후보다.
#[derive(Debug, Serialize)]
pub(crate) struct ReductionCandidate {
    pub(crate) glyph: char,
    pub(crate) occurrence_count: usize,
    pub(crate) unit_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CoresidentSummary {
    pub(crate) name: String,
    pub(crate) population_names: Vec<String>,
    pub(crate) glyph_count: usize,
    pub(crate) slot_budget: usize,
    pub(crate) fits: bool,
    pub(crate) excess_glyph_count: usize,
    pub(crate) reduction_candidates: Vec<ReductionCandidate>,
}

pub(crate) fn evaluate_coresident_set(
    name: &str,
    populations: &[&PopulationSummary],
    slot_budget: usize,
) -> CoresidentSummary {
    let mut occurrences = BTreeMap::<char, usize>::new();
    let mut units = BTreeMap::<char, BTreeSet<&str>>::new();
    for demand in populations
        .iter()
        .flat_map(|population| population.glyphs.iter())
    {
        *occurrences.entry(demand.glyph).or_default() += demand.occurrence_count;
        units
            .entry(demand.glyph)
            .or_default()
            .extend(demand.unit_ids.iter().map(String::as_str));
    }
    let glyphs = occurrences.keys().copied().collect::<BTreeSet<_>>();

    let mut reduction_candidates = occurrences
        .iter()
        .map(|(glyph, occurrence_count)| ReductionCandidate {
            glyph: *glyph,
            occurrence_count: *occurrence_count,
            unit_ids: units[glyph]
                .iter()
                .map(|unit_id| (*unit_id).to_owned())
                .collect(),
        })
        .collect::<Vec<_>>();
    reduction_candidates.sort_by_key(|candidate| {
        (
            candidate.occurrence_count,
            candidate.unit_ids.len(),
            candidate.glyph,
        )
    });

    CoresidentSummary {
        name: name.to_owned(),
        population_names: populations
            .iter()
            .map(|population| population.name.clone())
            .collect(),
        glyph_count: glyphs.len(),
        slot_budget,
        fits: glyphs.len() <= slot_budget,
        excess_glyph_count: glyphs.len().saturating_sub(slot_budget),
        reduction_candidates,
    }
}

/// 번역 마크업 `{...}` 안쪽은 제어·리터럴·동적 문자열 선택자라 글꼴 슬롯을
/// 요구하지 않는다. 원본이 그대로 두는 ASCII 영문·숫자·레이아웃 문자도 보호
/// 코드를 쓰므로 활성 한글 슬롯 수요에서 뺀다.
fn target_glyphs(text: &str) -> impl Iterator<Item = char> + '_ {
    let mut inside_markup = false;
    text.chars().filter(move |glyph| match glyph {
        '{' => {
            inside_markup = true;
            false
        }
        '}' => {
            inside_markup = false;
            false
        }
        _ => !inside_markup && !glyph.is_ascii(),
    })
}

pub(crate) fn summarize_population(population: &GlyphPopulation) -> PopulationSummary {
    let mut occurrences = BTreeMap::<char, usize>::new();
    let mut units = BTreeMap::<char, BTreeSet<&str>>::new();
    let mut occurrence_count = 0;

    for unit in &population.units {
        for glyph in target_glyphs(&unit.text) {
            *occurrences.entry(glyph).or_default() += 1;
            units.entry(glyph).or_default().insert(unit.id.as_str());
            occurrence_count += 1;
        }
    }

    let glyphs = occurrences
        .into_iter()
        .map(|(glyph, occurrence_count)| GlyphDemand {
            glyph,
            occurrence_count,
            unit_count: units[&glyph].len(),
            unit_ids: units[&glyph]
                .iter()
                .map(|unit_id| (*unit_id).to_owned())
                .collect(),
        })
        .collect::<Vec<_>>();

    PopulationSummary {
        name: population.name.clone(),
        kind: population.kind,
        unit_count: population.units.len(),
        occurrence_count,
        glyph_count: glyphs.len(),
        glyphs,
    }
}

#[cfg(test)]
mod tests;
