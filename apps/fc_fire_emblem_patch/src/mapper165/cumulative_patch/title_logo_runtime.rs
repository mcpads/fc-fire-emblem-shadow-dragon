use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::sha1_hex;

pub(super) struct TitleLogoRuntimeEvidence {
    pub(super) manifest_sha1: String,
    pub(super) sample_count: usize,
    pub(super) unique_image_count: usize,
}

/// 증거 경로를 준 기본 빌드는 정확한 산출물 결속을 요구한다. 경로를 생략한 개발
/// 빌드만 명시적으로 미결 상태를 돌려주며, 오래된 증거를 현재 산출물에 승계하지 않는다.
pub(super) fn load_title_logo_runtime_evidence(
    manifest_path: Option<&Path>,
    output_sha1: &str,
) -> Result<Option<TitleLogoRuntimeEvidence>> {
    manifest_path
        .map(|path| verify_title_logo_runtime_evidence(path, output_sha1))
        .transpose()
}

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    format_version: u8,
    screen_role: String,
    output_sha1: String,
    input_used: bool,
    samples: Vec<RuntimeSample>,
}

#[derive(Debug, Deserialize)]
struct RuntimeSample {
    label: String,
    frame_count: u64,
    phase: String,
    korean_logo_visible: bool,
    sword_visible: bool,
    tm_visible: Option<bool>,
    copyright_visible: bool,
    preserved_english_visible: Option<bool>,
    runtime_top_strip_cells_blank: Option<bool>,
    runtime_reasserted_logo_cells_match_static_logo: Option<bool>,
    image: String,
    image_sha1: String,
}

pub(super) fn verify_title_logo_runtime_evidence(
    manifest_path: &Path,
    output_sha1: &str,
) -> Result<TitleLogoRuntimeEvidence> {
    let manifest_bytes = fs::read(manifest_path).with_context(|| {
        format!(
            "read installed title-logo evidence {}",
            manifest_path.display()
        )
    })?;
    let manifest: RuntimeManifest = serde_json::from_slice(&manifest_bytes).with_context(|| {
        format!(
            "parse installed title-logo evidence {}",
            manifest_path.display()
        )
    })?;
    ensure!(
        manifest.format_version == 1
            && manifest.screen_role == "title_logo"
            && manifest.output_sha1.eq_ignore_ascii_case(output_sha1)
            && !manifest.input_used,
        "installed title-logo runtime evidence is not bound to this no-input output"
    );
    ensure!(
        manifest.samples.len() == 4,
        "installed title-logo runtime evidence must contain four route samples"
    );
    ensure!(
        manifest
            .samples
            .windows(2)
            .all(|pair| pair[0].frame_count < pair[1].frame_count),
        "installed title-logo runtime frames are not strictly increasing"
    );
    let intervals = manifest
        .samples
        .windows(2)
        .map(|pair| pair[1].frame_count - pair[0].frame_count)
        .collect::<BTreeSet<_>>();
    ensure!(
        intervals.len() == 3,
        "installed title-logo runtime samples are not irregular"
    );

    let samples = manifest
        .samples
        .iter()
        .map(|sample| (sample.label.as_str(), sample))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        samples.len() == manifest.samples.len(),
        "installed title-logo runtime evidence repeats a label"
    );
    let initial = samples
        .get("initial_palette_phase")
        .context("installed title-logo evidence lost its initial palette phase")?;
    ensure!(
        initial.phase == "initial_logo"
            && initial.korean_logo_visible
            && initial.sword_visible
            && !initial.copyright_visible,
        "installed title-logo initial palette phase changed"
    );
    let completed = samples
        .get("completed_palette_phase")
        .context("installed title-logo evidence lost its completed palette phase")?;
    ensure!(
        completed.phase == "completed_logo"
            && completed.korean_logo_visible
            && completed.sword_visible
            && completed.tm_visible == Some(true)
            && completed.runtime_top_strip_cells_blank == Some(true)
            && completed.runtime_reasserted_logo_cells_match_static_logo == Some(true)
            && !completed.copyright_visible,
        "installed title-logo completed palette phase changed"
    );
    let copyright = samples
        .get("completed_logo_with_copyright")
        .context("installed title-logo evidence lost its copyright phase")?;
    ensure!(
        copyright.phase == "completed_logo"
            && copyright.korean_logo_visible
            && copyright.sword_visible
            && copyright.tm_visible == Some(true)
            && copyright.runtime_top_strip_cells_blank == Some(true)
            && copyright.runtime_reasserted_logo_cells_match_static_logo == Some(true)
            && copyright.copyright_visible,
        "installed title-logo copyright phase changed"
    );
    let automatic_exit = samples
        .get("automatic_profile_after_title")
        .context("installed title-logo evidence lost its automatic exit")?;
    ensure!(
        automatic_exit.phase == "automatic_exit"
            && !automatic_exit.korean_logo_visible
            && !automatic_exit.sword_visible
            && !automatic_exit.copyright_visible
            && automatic_exit.preserved_english_visible == Some(true),
        "installed title-logo automatic exit changed"
    );

    let parent = manifest_path
        .parent()
        .context("installed title-logo evidence has no parent directory")?;
    let mut image_hashes = BTreeSet::new();
    for sample in &manifest.samples {
        let relative = Path::new(&sample.image);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| !matches!(component, Component::ParentDir)),
            "installed title-logo image path escapes its manifest directory"
        );
        let image_path = parent.join(relative);
        let image = fs::read(&image_path)
            .with_context(|| format!("read installed title-logo image {}", image_path.display()))?;
        ensure!(
            sha1_hex(&image) == sample.image_sha1,
            "installed title-logo image SHA-1 changed for {}",
            sample.label
        );
        image_hashes.insert(sample.image_sha1.as_str());
    }
    ensure!(
        image_hashes.len() == manifest.samples.len(),
        "installed title-logo route repeats an image"
    );

    Ok(TitleLogoRuntimeEvidence {
        manifest_sha1: sha1_hex(&manifest_bytes),
        sample_count: manifest.samples.len(),
        unique_image_count: image_hashes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_manifest_keeps_runtime_binding_unresolved() {
        assert!(
            load_title_logo_runtime_evidence(None, &"00".repeat(20))
                .unwrap()
                .is_none()
        );
    }
}
