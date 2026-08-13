use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT;

use super::{WorksetDemand, worksets_can_share_page};

const SOLVER_TIMEOUT_SECONDS: u64 = 120;

pub(super) struct CapacitySolverOutput {
    pub(super) page_indices: Vec<usize>,
    pub(super) solver_version: String,
    pub(super) timeout_seconds: u64,
    pub(super) strategy: &'static str,
}

pub(super) fn solve_page_capacity(
    demands: &[WorksetDemand],
    maximum_page_count: usize,
) -> Result<CapacitySolverOutput> {
    if scipy_highs_available() {
        solve_page_capacity_with_highs(demands, maximum_page_count)
    } else {
        solve_page_capacity_with_z3(demands, maximum_page_count)
            .context("SciPy/HiGHS is unavailable and the Z3 fallback also failed")
    }
}

fn scipy_highs_available() -> bool {
    Command::new("python3")
        .args(["-c", "import scipy.optimize"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[derive(Serialize)]
struct HighsInput {
    active_slot_count: usize,
    maximum_page_count: usize,
    timeout_seconds: u64,
    worksets: Vec<HighsWorkset>,
    incompatible_workset_pairs: Vec<[usize; 2]>,
}

#[derive(Serialize)]
struct HighsWorkset {
    glyph_indices: Vec<usize>,
    preserved_codes: Vec<u8>,
}

#[derive(Deserialize)]
struct HighsOutput {
    status: String,
    solver_version: String,
    message: String,
    page_indices: Option<Vec<usize>>,
}

fn solve_page_capacity_with_highs(
    demands: &[WorksetDemand],
    maximum_page_count: usize,
) -> Result<CapacitySolverOutput> {
    let mut glyph_indices = BTreeMap::new();
    for demand in demands {
        for glyph in &demand.signature.target_glyphs {
            let next_index = glyph_indices.len();
            glyph_indices.entry(*glyph).or_insert(next_index);
        }
    }
    let input = HighsInput {
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        maximum_page_count,
        timeout_seconds: SOLVER_TIMEOUT_SECONDS,
        worksets: demands
            .iter()
            .map(|demand| HighsWorkset {
                glyph_indices: demand
                    .signature
                    .target_glyphs
                    .iter()
                    .map(|glyph| glyph_indices[glyph])
                    .collect(),
                preserved_codes: demand.signature.preserved_codes.clone(),
            })
            .collect(),
        incompatible_workset_pairs: incompatible_workset_pairs(demands),
    };
    let input_bytes = serde_json::to_vec(&input).context("serialize HiGHS page model")?;
    let solver_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/solve_dialogue_page_capacity.py");
    let mut child = Command::new("python3")
        .arg(&solver_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("launch {}", solver_path.display()))?;
    child
        .stdin
        .take()
        .context("open HiGHS helper stdin")?
        .write_all(&input_bytes)
        .context("write HiGHS dialogue page model")?;
    let output = child
        .wait_with_output()
        .context("wait for HiGHS dialogue page capacity solver")?;
    let stdout = String::from_utf8(output.stdout).context("decode HiGHS helper stdout")?;
    let stderr = String::from_utf8(output.stderr).context("decode HiGHS helper stderr")?;
    ensure!(
        output.status.success(),
        "HiGHS dialogue page helper failed: {}",
        stderr.trim()
    );
    let solved: HighsOutput =
        serde_json::from_str(&stdout).context("parse HiGHS dialogue page result")?;
    let page_indices = match (solved.status.as_str(), solved.page_indices) {
        ("feasible", Some(page_indices)) => page_indices,
        ("infeasible", _) => bail!(
            "HiGHS proved that the current worksets cannot fit in {maximum_page_count} dialogue font pages"
        ),
        ("timeout", _) => bail!(
            "HiGHS did not find a {maximum_page_count}-page dialogue font plan within {SOLVER_TIMEOUT_SECONDS} seconds: {}",
            solved.message
        ),
        (status, _) => bail!(
            "HiGHS returned an unexpected page result {status:?}: {}",
            solved.message
        ),
    };
    verify_assignment(demands, &page_indices, maximum_page_count)?;
    Ok(CapacitySolverOutput {
        page_indices,
        solver_version: solved.solver_version,
        timeout_seconds: SOLVER_TIMEOUT_SECONDS,
        strategy: "SciPy/HiGHS bounded 0-1 union-capacity assignment",
    })
}

fn solve_page_capacity_with_z3(
    demands: &[WorksetDemand],
    maximum_page_count: usize,
) -> Result<CapacitySolverOutput> {
    ensure!(
        !demands.is_empty(),
        "Z3 page capacity model has no worksets"
    );
    ensure!(
        maximum_page_count > 0,
        "Z3 page capacity model has no pages"
    );
    let version_output = Command::new("z3")
        .arg("-version")
        .output()
        .context("run `z3 -version`; install Z3 to solve the bounded dialogue page plan")?;
    ensure!(
        version_output.status.success(),
        "`z3 -version` failed while preparing the dialogue page plan"
    );
    let solver_version = String::from_utf8(version_output.stdout)
        .context("decode Z3 version")?
        .trim()
        .to_owned();
    let model = build_smt_model(demands, maximum_page_count);
    let mut child = Command::new("z3")
        .arg("-in")
        .arg(format!("-T:{SOLVER_TIMEOUT_SECONDS}"))
        .arg("smt.random_seed=0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("launch Z3 dialogue page capacity solver")?;
    child
        .stdin
        .take()
        .context("open Z3 stdin")?
        .write_all(model.as_bytes())
        .context("write Z3 dialogue page capacity model")?;
    let output = child
        .wait_with_output()
        .context("wait for Z3 dialogue page capacity solver")?;
    let stdout = String::from_utf8(output.stdout).context("decode Z3 stdout")?;
    let stderr = String::from_utf8(output.stderr).context("decode Z3 stderr")?;
    ensure!(
        output.status.success(),
        "Z3 dialogue page capacity solver failed: {}",
        stderr.trim()
    );
    let status = stdout.lines().next().unwrap_or_default().trim();
    match status {
        "sat" => {}
        "unsat" => bail!(
            "Z3 proved that the current worksets cannot fit in {maximum_page_count} dialogue font pages"
        ),
        "unknown" | "timeout" => bail!(
            "Z3 did not decide the {maximum_page_count}-page dialogue font plan within {SOLVER_TIMEOUT_SECONDS} seconds"
        ),
        _ => bail!("Z3 returned an unexpected dialogue page result: {status:?}"),
    }
    let page_indices = parse_page_values(&stdout, demands.len())?;
    verify_assignment(demands, &page_indices, maximum_page_count)?;
    Ok(CapacitySolverOutput {
        page_indices,
        solver_version,
        timeout_seconds: SOLVER_TIMEOUT_SECONDS,
        strategy: "Z3 bounded 0-1 union-capacity assignment",
    })
}

fn build_smt_model(demands: &[WorksetDemand], maximum_page_count: usize) -> String {
    let mut glyph_indices = BTreeMap::new();
    let mut preserved_codes = BTreeSet::new();
    for demand in demands {
        for glyph in &demand.signature.target_glyphs {
            let next_index = glyph_indices.len();
            glyph_indices.entry(*glyph).or_insert(next_index);
        }
        preserved_codes.extend(demand.signature.preserved_codes.iter().copied());
    }
    let mut model = String::from(
        "(set-option :produce-models true)\n(set-option :smt.random_seed 0)\n(set-logic ALL)\n",
    );
    for index in 0..demands.len() {
        model.push_str(&format!("(declare-fun page_{index} () Int)\n"));
        model.push_str(&format!(
            "(assert (and (<= 0 page_{index}) (< page_{index} {maximum_page_count})))\n"
        ));
    }
    model.push_str("(assert (= page_0 0))\n");
    for index in 1..demands.len() {
        model.push_str(&format!("(declare-fun maximum_page_{index} () Int)\n"));
        let previous_maximum = if index == 1 {
            "page_0".to_owned()
        } else {
            format!("maximum_page_{}", index - 1)
        };
        model.push_str(&format!(
            "(assert (= maximum_page_{index} (ite (> page_{index} {previous_maximum}) page_{index} {previous_maximum})))\n"
        ));
        model.push_str(&format!(
            "(assert (<= page_{index} (+ {previous_maximum} 1)))\n"
        ));
    }
    for [left, right] in incompatible_workset_pairs(demands) {
        model.push_str(&format!("(assert (not (= page_{left} page_{right})))\n"));
    }
    for page in 0..maximum_page_count {
        let mut capacity_variables = Vec::new();
        for (glyph, glyph_index) in &glyph_indices {
            let variable = format!("glyph_{glyph_index}_page_{page}");
            capacity_variables.push(variable.clone());
            model.push_str(&format!("(declare-fun {variable} () Bool)\n"));
            let members = demands
                .iter()
                .enumerate()
                .filter(|(_, demand)| demand.signature.target_glyphs.binary_search(glyph).is_ok())
                .map(|(index, _)| format!("(= page_{index} {page})"))
                .collect::<Vec<_>>();
            model.push_str(&format!(
                "(assert (= {variable} (or {})))\n",
                members.join(" ")
            ));
        }
        for code in &preserved_codes {
            let variable = format!("code_{code}_page_{page}");
            capacity_variables.push(variable.clone());
            model.push_str(&format!("(declare-fun {variable} () Bool)\n"));
            let members = demands
                .iter()
                .enumerate()
                .filter(|(_, demand)| demand.signature.preserved_codes.binary_search(code).is_ok())
                .map(|(index, _)| format!("(= page_{index} {page})"))
                .collect::<Vec<_>>();
            model.push_str(&format!(
                "(assert (= {variable} (or {})))\n",
                members.join(" ")
            ));
        }
        model.push_str(&format!(
            "(assert ((_ at-most {}) {}))\n",
            ACTIVE_HANGUL_SLOT_COUNT,
            capacity_variables.join(" ")
        ));
    }
    model.push_str("(check-sat)\n(get-value (");
    for index in 0..demands.len() {
        model.push_str(&format!(" page_{index}"));
    }
    model.push_str("))\n");
    model
}

fn parse_page_values(output: &str, expected_count: usize) -> Result<Vec<usize>> {
    let normalized = output.replace(['(', ')'], " ");
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();
    let mut values = vec![None; expected_count];
    for pair in tokens.windows(2) {
        let Some(index) = pair[0].strip_prefix("page_") else {
            continue;
        };
        let index = index.parse::<usize>().context("decode Z3 page variable")?;
        if index < expected_count {
            values[index] = Some(
                pair[1]
                    .parse::<usize>()
                    .context("decode Z3 page assignment")?,
            );
        }
    }
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| value.with_context(|| format!("Z3 omitted page_{index}")))
        .collect()
}

fn verify_assignment(
    demands: &[WorksetDemand],
    page_indices: &[usize],
    maximum_page_count: usize,
) -> Result<()> {
    ensure!(
        demands.len() == page_indices.len(),
        "Z3 dialogue page assignment count changed"
    );
    for page in 0..maximum_page_count {
        let mut glyphs = BTreeSet::new();
        let mut codes = BTreeSet::new();
        for (demand, assigned_page) in demands.iter().zip(page_indices) {
            if *assigned_page == page {
                glyphs.extend(demand.signature.target_glyphs.iter().copied());
                codes.extend(demand.signature.preserved_codes.iter().copied());
            }
        }
        ensure!(
            glyphs.len() + codes.len() <= ACTIVE_HANGUL_SLOT_COUNT,
            "Z3 dialogue page {page} exceeds the active slot capacity"
        );
        let members = page_indices
            .iter()
            .enumerate()
            .filter_map(|(index, assigned_page)| (*assigned_page == page).then_some(index))
            .collect::<Vec<_>>();
        for (position, left) in members.iter().enumerate() {
            for right in &members[position + 1..] {
                ensure!(
                    worksets_can_share_page(&demands[*left].signature, &demands[*right].signature,),
                    "Z3 dialogue page {page} merges incompatible fixed glyph codes"
                );
            }
        }
    }
    Ok(())
}

fn incompatible_workset_pairs(demands: &[WorksetDemand]) -> Vec<[usize; 2]> {
    let mut pairs = Vec::new();
    for left in 0..demands.len() {
        for right in left + 1..demands.len() {
            if !worksets_can_share_page(&demands[left].signature, &demands[right].signature) {
                pairs.push([left, right]);
            }
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapper165::battle_codebook_plan::workset_pages::WorksetSignature;

    fn demand(glyphs: &str, preserved_codes: &[u8]) -> WorksetDemand {
        WorksetDemand {
            signature: WorksetSignature {
                target_glyphs: glyphs.chars().collect(),
                preserved_codes: preserved_codes.to_vec(),
                fixed_glyph_codes: Vec::new(),
            },
            original_indices: Vec::new(),
        }
    }

    #[test]
    fn smt_model_binds_union_capacity_and_page_symmetry() {
        let model = build_smt_model(&[demand("가나", &[1]), demand("나다", &[2])], 2);

        assert!(model.contains("((_ at-most 210)"));
        assert!(model.contains("(assert (= page_0 0))"));
        assert!(model.contains("(assert (<= page_1 (+ page_0 1)))"));
        assert!(model.contains("(check-sat)"));
    }

    #[test]
    fn smt_model_separates_incompatible_fixed_assignments() {
        let code = crate::font_slots::active_hangul_codes()[0];
        let mut first = demand("가", &[]);
        first.signature.fixed_glyph_codes = vec![('가', code)];
        let mut second = demand("나", &[]);
        second.signature.fixed_glyph_codes = vec![('나', code)];

        let model = build_smt_model(&[first, second], 2);

        assert!(model.contains("(assert (not (= page_0 page_1)))"));
    }

    #[test]
    fn page_value_parser_reads_the_requested_model_values() {
        let values = parse_page_values("sat\n((page_0 0) (page_1 1))\n", 2).unwrap();

        assert_eq!(values, vec![0, 1]);
    }

    #[test]
    fn independent_verifier_rejects_an_over_capacity_model() {
        let glyphs = (0..=ACTIVE_HANGUL_SLOT_COUNT)
            .map(|index| char::from_u32(0xAC00 + index as u32).unwrap())
            .collect::<String>();
        let error = verify_assignment(&[demand(&glyphs, &[])], &[0], 1).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("exceeds the active slot capacity")
        );
    }
}
