use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::sha1_hex;

const MINIMUM_IRREGULAR_SAMPLE_COUNT: usize = 3;
const MULTI_PAGE_MAIN_DIALOGUE_ROLE: &str = "chapter_intro_title_dialogue_composite";
const CHAPTER_CLEAR_EPILOGUE_DIALOGUE_ROLE: &str = "chapter_clear_epilogue_dialogue";
const MAXIMUM_DIALOGUE_COMPLETED_PAGE_COUNT: usize = 15;

#[derive(Serialize)]
pub(super) struct FinalArtifactRuntimeEvidence {
    provided: bool,
    manifest_sha1: String,
    artifact_sha1: String,
    run_count: usize,
    observation_count: usize,
    sample_count: usize,
    representative_role_count: usize,
    worst_case_role_count: usize,
    bound_screen_roles: Vec<String>,
    every_run_started_from_cold_boot: bool,
    savestate_free: bool,
    every_sample_image_digest_bound: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeEvidenceManifest {
    artifact_sha1: String,
    runs: Vec<RuntimeRun>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeRun {
    run_id: String,
    started_from_cold_boot: bool,
    savestate_used: bool,
    observations: Vec<ScreenObservation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScreenObservation {
    screen_role: String,
    kind: ObservationKind,
    hangul_readable: bool,
    japanese_target_text_absent: bool,
    protected_original_text: ProtectedOriginalText,
    visual_glitch_absent_across_samples: bool,
    #[serde(default)]
    main_dialogue_progression: Option<MainDialogueProgression>,
    #[serde(default)]
    maximum_dialogue_progression: Option<MaximumDialogueProgression>,
    samples: Vec<RuntimeSample>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MainDialogueProgression {
    distinct_visible_page_sample_offsets: Vec<u64>,
    following_record_sample_offset: u64,
    role_exit_sample_offset: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaximumDialogueProgression {
    completed_page_sample_offsets: Vec<u64>,
    exit_sample_offset: u64,
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ObservationKind {
    Representative,
    WorstCase,
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ProtectedOriginalText {
    Preserved,
    NotPresent,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeSample {
    frame_offset: u64,
    image: PathBuf,
    image_sha256: String,
}

impl FinalArtifactRuntimeEvidence {
    pub(super) fn absent(artifact_sha1: &str) -> Self {
        Self {
            provided: false,
            manifest_sha1: String::new(),
            artifact_sha1: artifact_sha1.to_owned(),
            run_count: 0,
            observation_count: 0,
            sample_count: 0,
            representative_role_count: 0,
            worst_case_role_count: 0,
            bound_screen_roles: Vec::new(),
            every_run_started_from_cold_boot: false,
            savestate_free: false,
            every_sample_image_digest_bound: false,
        }
    }

    pub(super) fn bound_screen_roles(&self) -> BTreeSet<String> {
        self.bound_screen_roles.iter().cloned().collect()
    }

    pub(super) fn verification_started(&self) -> bool {
        self.provided && !self.bound_screen_roles.is_empty()
    }
}

pub(super) fn load_final_artifact_runtime_evidence(
    manifest_path: Option<&Path>,
    artifact_sha1: &str,
) -> Result<FinalArtifactRuntimeEvidence> {
    let Some(manifest_path) = manifest_path else {
        return Ok(FinalArtifactRuntimeEvidence::absent(artifact_sha1));
    };
    ensure!(
        is_hex_digest(artifact_sha1, 40),
        "integrated artifact SHA-1 is not a 40-digit hexadecimal digest"
    );

    let manifest_bytes = fs::read(manifest_path).with_context(|| {
        format!(
            "read final-artifact runtime evidence {}",
            manifest_path.display()
        )
    })?;
    let manifest: RuntimeEvidenceManifest =
        serde_json::from_slice(&manifest_bytes).with_context(|| {
            format!(
                "parse final-artifact runtime evidence {}",
                manifest_path.display()
            )
        })?;
    ensure!(
        manifest.artifact_sha1.eq_ignore_ascii_case(artifact_sha1),
        "final-artifact runtime evidence names ROM {}, but the integrated artifact is {}",
        manifest.artifact_sha1,
        artifact_sha1
    );
    ensure!(
        !manifest.runs.is_empty(),
        "final-artifact runtime evidence has no runs"
    );

    let manifest_directory = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let mut run_ids = BTreeSet::new();
    let mut bound_screen_roles = BTreeSet::new();
    let mut representative_roles = BTreeSet::new();
    let mut worst_case_roles = BTreeSet::new();
    let mut observation_count = 0usize;
    let mut sample_count = 0usize;

    for run in &manifest.runs {
        ensure!(
            !run.run_id.trim().is_empty(),
            "runtime evidence run ID is empty"
        );
        ensure!(
            run_ids.insert(run.run_id.as_str()),
            "runtime evidence repeats run ID {}",
            run.run_id
        );
        ensure!(
            run.started_from_cold_boot,
            "runtime evidence run {} did not start from a cold boot",
            run.run_id
        );
        ensure!(
            !run.savestate_used,
            "runtime evidence run {} used a savestate",
            run.run_id
        );
        ensure!(
            !run.observations.is_empty(),
            "runtime evidence run {} has no screen observations",
            run.run_id
        );

        for observation in &run.observations {
            verify_observation(observation, manifest_directory, &run.run_id)?;
            observation_count += 1;
            sample_count += observation.samples.len();
            bound_screen_roles.insert(observation.screen_role.clone());
            match observation.kind {
                ObservationKind::Representative => {
                    representative_roles.insert(observation.screen_role.clone());
                }
                ObservationKind::WorstCase => {
                    worst_case_roles.insert(observation.screen_role.clone());
                }
            }
        }
    }

    Ok(FinalArtifactRuntimeEvidence {
        provided: true,
        manifest_sha1: sha1_hex(&manifest_bytes),
        artifact_sha1: artifact_sha1.to_owned(),
        run_count: manifest.runs.len(),
        observation_count,
        sample_count,
        representative_role_count: representative_roles.len(),
        worst_case_role_count: worst_case_roles.len(),
        bound_screen_roles: bound_screen_roles.into_iter().collect(),
        every_run_started_from_cold_boot: true,
        savestate_free: true,
        every_sample_image_digest_bound: true,
    })
}

fn verify_observation(
    observation: &ScreenObservation,
    manifest_directory: &Path,
    run_id: &str,
) -> Result<()> {
    ensure!(
        !observation.screen_role.trim().is_empty(),
        "runtime evidence run {run_id} has an empty screen role"
    );
    ensure!(
        observation.hangul_readable,
        "runtime screen {} was not judged readable Hangul",
        observation.screen_role
    );
    ensure!(
        observation.japanese_target_text_absent,
        "runtime screen {} still contains target Japanese",
        observation.screen_role
    );
    ensure!(
        matches!(
            observation.protected_original_text,
            ProtectedOriginalText::Preserved | ProtectedOriginalText::NotPresent
        ),
        "runtime screen {} did not classify protected original text",
        observation.screen_role
    );
    ensure!(
        observation.visual_glitch_absent_across_samples,
        "runtime screen {} has a visual glitch in its temporal samples",
        observation.screen_role
    );
    if observation.screen_role == MULTI_PAGE_MAIN_DIALOGUE_ROLE {
        let progression = observation
            .main_dialogue_progression
            .as_ref()
            .context("multi-page main-dialogue runtime evidence has no progression result")?;
        let sample_offsets = observation
            .samples
            .iter()
            .map(|sample| sample.frame_offset)
            .collect::<BTreeSet<_>>();
        ensure!(
            progression.distinct_visible_page_sample_offsets.len() >= 2,
            "multi-page main-dialogue runtime evidence observed fewer than two distinct pages"
        );
        ensure!(
            progression
                .distinct_visible_page_sample_offsets
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            "multi-page main-dialogue page samples are not strictly increasing"
        );
        ensure!(
            progression
                .distinct_visible_page_sample_offsets
                .iter()
                .all(|offset| sample_offsets.contains(offset)),
            "multi-page main-dialogue page evidence names an unbound sample"
        );
        let last_visible_page = *progression
            .distinct_visible_page_sample_offsets
            .last()
            .context("multi-page main-dialogue evidence has no page sample")?;
        ensure!(
            last_visible_page < progression.following_record_sample_offset
                && sample_offsets.contains(&progression.following_record_sample_offset),
            "multi-page main-dialogue evidence did not bind a later following-record sample"
        );
        ensure!(
            progression.following_record_sample_offset < progression.role_exit_sample_offset
                && sample_offsets.contains(&progression.role_exit_sample_offset),
            "multi-page main-dialogue evidence did not bind a later role-exit sample"
        );
    } else {
        ensure!(
            observation.main_dialogue_progression.is_none(),
            "runtime screen {} declares main-dialogue progression for another role",
            observation.screen_role
        );
    }
    if observation.screen_role == CHAPTER_CLEAR_EPILOGUE_DIALOGUE_ROLE
        && observation.kind == ObservationKind::WorstCase
    {
        let progression = observation
            .maximum_dialogue_progression
            .as_ref()
            .context("maximum-dialogue runtime evidence has no progression result")?;
        let sample_offsets = observation
            .samples
            .iter()
            .map(|sample| sample.frame_offset)
            .collect::<BTreeSet<_>>();
        ensure!(
            progression.completed_page_sample_offsets.len()
                == MAXIMUM_DIALOGUE_COMPLETED_PAGE_COUNT,
            "maximum-dialogue runtime evidence observed {} completed pages instead of {}",
            progression.completed_page_sample_offsets.len(),
            MAXIMUM_DIALOGUE_COMPLETED_PAGE_COUNT
        );
        ensure!(
            progression
                .completed_page_sample_offsets
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            "maximum-dialogue completed-page samples are not strictly increasing"
        );
        ensure!(
            progression
                .completed_page_sample_offsets
                .iter()
                .all(|offset| sample_offsets.contains(offset)),
            "maximum-dialogue progression names an unbound completed-page sample"
        );
        let completed_page_image_digests = progression
            .completed_page_sample_offsets
            .iter()
            .filter_map(|offset| {
                observation
                    .samples
                    .iter()
                    .find(|sample| sample.frame_offset == *offset)
                    .map(|sample| sample.image_sha256.as_str())
            })
            .collect::<BTreeSet<_>>();
        ensure!(
            completed_page_image_digests.len() == MAXIMUM_DIALOGUE_COMPLETED_PAGE_COUNT,
            "maximum-dialogue runtime evidence has {} distinct completed-page images instead of {}",
            completed_page_image_digests.len(),
            MAXIMUM_DIALOGUE_COMPLETED_PAGE_COUNT
        );
        let last_completed_page = *progression
            .completed_page_sample_offsets
            .last()
            .context("maximum-dialogue evidence has no completed-page sample")?;
        ensure!(
            last_completed_page < progression.exit_sample_offset
                && sample_offsets.contains(&progression.exit_sample_offset),
            "maximum-dialogue evidence did not bind an exit after page 15"
        );
    } else {
        ensure!(
            observation.maximum_dialogue_progression.is_none(),
            "runtime screen {} declares maximum-dialogue progression for another role",
            observation.screen_role
        );
    }
    ensure!(
        observation.samples.len() >= MINIMUM_IRREGULAR_SAMPLE_COUNT,
        "runtime screen {} has fewer than {} temporal samples",
        observation.screen_role,
        MINIMUM_IRREGULAR_SAMPLE_COUNT
    );

    let offsets = observation
        .samples
        .iter()
        .map(|sample| sample.frame_offset)
        .collect::<Vec<_>>();
    ensure!(
        offsets.windows(2).all(|pair| pair[0] < pair[1]),
        "runtime screen {} sample offsets are not strictly increasing",
        observation.screen_role
    );
    let gaps = offsets
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect::<BTreeSet<_>>();
    ensure!(
        gaps.len() >= 2,
        "runtime screen {} uses regular temporal sampling; use irregular offsets to cover blinking UI",
        observation.screen_role
    );

    for sample in &observation.samples {
        ensure!(
            !sample.image.is_absolute(),
            "runtime sample image must be relative to its private manifest"
        );
        ensure!(
            is_hex_digest(&sample.image_sha256, 64),
            "runtime sample image SHA-256 is malformed for {}",
            sample.image.display()
        );
        let image_path = manifest_directory.join(&sample.image);
        let image_bytes = fs::read(&image_path)
            .with_context(|| format!("read runtime sample image {}", image_path.display()))?;
        let actual_sha256 = format!("{:x}", Sha256::digest(&image_bytes));
        ensure!(
            actual_sha256.eq_ignore_ascii_case(&sample.image_sha256),
            "runtime sample image digest mismatch for {}",
            image_path.display()
        );
    }
    Ok(())
}

fn is_hex_digest(value: &str, digit_count: usize) -> bool {
    value.len() == digit_count && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests;
