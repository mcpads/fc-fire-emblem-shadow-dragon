use std::{collections::BTreeSet, path::Path, path::PathBuf};

use anyhow::{Result, ensure};
use serde::Deserialize;

use super::{
    MAXIMUM_DIALOGUE_COMPLETED_PAGE_COUNT, ObservationKind, ProtectedOriginalText,
    verify_runtime_image,
};

const MULTI_PAGE_MAIN_DIALOGUE_ROLE: &str = "chapter_intro_title_dialogue_composite";
const CHAPTER_CLEAR_EPILOGUE_DIALOGUE_ROLE: &str = "chapter_clear_epilogue_dialogue";
const CHAPTER_SAVE_OFFER_ROLE: &str = "chapter_save_offer";
const CHAPTER_SAVE_COMPLETE_CONTINUE_PROMPT_ROLE: &str = "chapter_save_complete_continue_prompt";
const UNIT_ROSTER_ROLE: &str = "unit_roster";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeRoute {
    route_id: String,
    kind: RuntimeRouteKind,
    checkpoints: Vec<RouteCheckpoint>,
}

impl RuntimeRoute {
    pub(super) fn id(&self) -> &str {
        &self.route_id
    }

    pub(super) fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RuntimeRouteKind {
    ChapterClearToNextMap,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteCheckpoint {
    kind: RouteCheckpointKind,
    frame_offset: u64,
    text_result: RouteCheckpointText,
    japanese_target_text_absent: bool,
    protected_original_text: ProtectedOriginalText,
    visible_frame_glitch_absent: bool,
    image: PathBuf,
    image_sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RouteCheckpointKind {
    ChapterClearPage,
    NextStory,
    ChapterSaveOffer,
    ChapterSaveCompleteContinuePrompt,
    ChapterIntroPage,
    NextChapterMap,
    UnitRoster,
    MapReturn,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RouteCheckpointText {
    TranslatedHangul,
    PreservedOriginalOnly,
    NoTargetText,
}

pub(super) fn verify_route(
    route: &RuntimeRoute,
    manifest_directory: &Path,
    run_id: &str,
) -> Result<Vec<(&'static str, ObservationKind)>> {
    match route.kind {
        RuntimeRouteKind::ChapterClearToNextMap => {
            verify_chapter_clear_to_next_map_route(route, manifest_directory, run_id)
        }
    }
}

fn verify_chapter_clear_to_next_map_route(
    route: &RuntimeRoute,
    manifest_directory: &Path,
    run_id: &str,
) -> Result<Vec<(&'static str, ObservationKind)>> {
    let mut expected_checkpoint_kinds =
        vec![RouteCheckpointKind::ChapterClearPage; MAXIMUM_DIALOGUE_COMPLETED_PAGE_COUNT];
    expected_checkpoint_kinds.extend([
        RouteCheckpointKind::NextStory,
        RouteCheckpointKind::ChapterSaveOffer,
        RouteCheckpointKind::ChapterSaveCompleteContinuePrompt,
        RouteCheckpointKind::ChapterIntroPage,
        RouteCheckpointKind::ChapterIntroPage,
        RouteCheckpointKind::ChapterIntroPage,
        RouteCheckpointKind::ChapterIntroPage,
        RouteCheckpointKind::NextChapterMap,
        RouteCheckpointKind::UnitRoster,
        RouteCheckpointKind::MapReturn,
    ]);
    let actual_checkpoint_kinds = route
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.kind)
        .collect::<Vec<_>>();
    ensure!(
        actual_checkpoint_kinds == expected_checkpoint_kinds,
        "runtime evidence run {run_id} route {} does not follow the chapter-clear to next-map checkpoint sequence",
        route.route_id
    );

    let offsets = route
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.frame_offset)
        .collect::<Vec<_>>();
    ensure!(
        offsets.windows(2).all(|pair| pair[0] < pair[1]),
        "runtime evidence run {run_id} route {} checkpoint offsets are not strictly increasing",
        route.route_id
    );

    let completed_page_digests = route.checkpoints[..MAXIMUM_DIALOGUE_COMPLETED_PAGE_COUNT]
        .iter()
        .map(|checkpoint| checkpoint.image_sha256.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    ensure!(
        completed_page_digests.len() == MAXIMUM_DIALOGUE_COMPLETED_PAGE_COUNT,
        "runtime evidence run {run_id} route {} has {} distinct chapter-clear pages instead of {}",
        route.route_id,
        completed_page_digests.len(),
        MAXIMUM_DIALOGUE_COMPLETED_PAGE_COUNT
    );
    let chapter_intro_digests = route
        .checkpoints
        .iter()
        .filter(|checkpoint| checkpoint.kind == RouteCheckpointKind::ChapterIntroPage)
        .map(|checkpoint| checkpoint.image_sha256.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    ensure!(
        chapter_intro_digests.len() == 4,
        "runtime evidence run {run_id} route {} has {} distinct chapter-intro pages instead of 4",
        route.route_id,
        chapter_intro_digests.len()
    );

    for checkpoint in &route.checkpoints {
        verify_route_checkpoint(checkpoint, manifest_directory, run_id, &route.route_id)?;
    }

    Ok(vec![
        (
            CHAPTER_CLEAR_EPILOGUE_DIALOGUE_ROLE,
            ObservationKind::WorstCase,
        ),
        (CHAPTER_SAVE_OFFER_ROLE, ObservationKind::Representative),
        (
            CHAPTER_SAVE_COMPLETE_CONTINUE_PROMPT_ROLE,
            ObservationKind::Representative,
        ),
        (
            MULTI_PAGE_MAIN_DIALOGUE_ROLE,
            ObservationKind::Representative,
        ),
        (UNIT_ROSTER_ROLE, ObservationKind::Representative),
    ])
}

fn verify_route_checkpoint(
    checkpoint: &RouteCheckpoint,
    manifest_directory: &Path,
    run_id: &str,
    route_id: &str,
) -> Result<()> {
    let expected_text_result = match checkpoint.kind {
        RouteCheckpointKind::ChapterClearPage
        | RouteCheckpointKind::ChapterSaveOffer
        | RouteCheckpointKind::ChapterSaveCompleteContinuePrompt
        | RouteCheckpointKind::ChapterIntroPage
        | RouteCheckpointKind::UnitRoster => RouteCheckpointText::TranslatedHangul,
        RouteCheckpointKind::NextStory => RouteCheckpointText::PreservedOriginalOnly,
        RouteCheckpointKind::NextChapterMap | RouteCheckpointKind::MapReturn => {
            RouteCheckpointText::NoTargetText
        }
    };
    ensure!(
        checkpoint.text_result == expected_text_result,
        "runtime evidence run {run_id} route {route_id} checkpoint {:?} has the wrong text result",
        checkpoint.kind
    );
    ensure!(
        checkpoint.japanese_target_text_absent,
        "runtime evidence run {run_id} route {route_id} checkpoint still contains target Japanese"
    );
    ensure!(
        matches!(
            checkpoint.protected_original_text,
            ProtectedOriginalText::Preserved | ProtectedOriginalText::NotPresent
        ),
        "runtime evidence run {run_id} route {route_id} checkpoint did not classify protected original text"
    );
    ensure!(
        checkpoint.visible_frame_glitch_absent,
        "runtime evidence run {run_id} route {route_id} checkpoint has a visible-frame glitch"
    );
    verify_runtime_image(
        &checkpoint.image,
        &checkpoint.image_sha256,
        manifest_directory,
    )
}
