use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::sha1_hex;

pub(super) struct ClassProfileRuntimeEvidence {
    pub(super) manifest_sha1: String,
    pub(super) sample_count: usize,
    pub(super) unique_image_count: usize,
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
    profile_index: u8,
    mapper_register_2: Option<u8>,
    mapper_register_4: Option<u8>,
    original_english_visible: Option<bool>,
    image: String,
    image_sha1: String,
}

pub(super) fn verify_class_profile_runtime_evidence(
    manifest_path: &Path,
    output_sha1: &str,
    second_page_mapper_register: u8,
) -> Result<ClassProfileRuntimeEvidence> {
    let manifest_bytes = fs::read(manifest_path).with_context(|| {
        format!(
            "read installed class-profile evidence {}",
            manifest_path.display()
        )
    })?;
    let manifest: RuntimeManifest = serde_json::from_slice(&manifest_bytes).with_context(|| {
        format!(
            "parse installed class-profile evidence {}",
            manifest_path.display()
        )
    })?;
    ensure!(
        manifest.format_version == 1
            && manifest.screen_role == "automatic_class_profile"
            && manifest.output_sha1.eq_ignore_ascii_case(output_sha1)
            && !manifest.input_used,
        "installed class-profile runtime evidence is not bound to this no-input output"
    );
    ensure!(
        manifest.samples.len() == 7,
        "installed class-profile runtime evidence must contain seven route samples"
    );
    ensure!(
        manifest
            .samples
            .windows(2)
            .all(|pair| pair[0].frame_count < pair[1].frame_count),
        "installed class-profile runtime frames are not strictly increasing"
    );
    let intervals = manifest
        .samples
        .windows(2)
        .map(|pair| pair[1].frame_count - pair[0].frame_count)
        .collect::<BTreeSet<_>>();
    ensure!(
        intervals.len() >= 4,
        "installed class-profile runtime samples are not irregular"
    );

    let samples = manifest
        .samples
        .iter()
        .map(|sample| (sample.label.as_str(), sample))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        samples.len() == manifest.samples.len(),
        "installed class-profile runtime evidence repeats a label"
    );
    for (label, profile_index) in [
        ("first_profile_stable", 0),
        ("late_first_font_page", 8),
        ("second_font_page_boundary", 11),
        ("late_second_font_page", 20),
        ("last_profile", 21),
    ] {
        let sample = samples
            .get(label)
            .with_context(|| format!("installed class-profile evidence lost {label}"))?;
        ensure!(
            sample.phase == "profile" && sample.profile_index == profile_index,
            "installed class-profile evidence changed {label}"
        );
    }
    let boundary = samples["second_font_page_boundary"];
    ensure!(
        boundary.mapper_register_2 == Some(second_page_mapper_register)
            && boundary.mapper_register_4 == Some(second_page_mapper_register),
        "installed class-profile page boundary did not select both CHR latches"
    );
    let automatic_exit = samples
        .get("automatic_blackout")
        .context("installed class-profile evidence lost the automatic blackout")?;
    ensure!(
        automatic_exit.phase == "automatic_exit" && automatic_exit.profile_index == 0,
        "installed class-profile automatic-exit state changed"
    );
    let after_exit = samples
        .get("preserved_english_opening_after_exit")
        .context("installed class-profile evidence lost the post-exit opening")?;
    ensure!(
        after_exit.phase == "after_exit"
            && after_exit.profile_index == 0
            && after_exit.original_english_visible == Some(true),
        "installed class-profile evidence no longer preserves the English opening"
    );

    let parent = manifest_path
        .parent()
        .context("installed class-profile evidence has no parent directory")?;
    let mut image_hashes = BTreeSet::new();
    for sample in &manifest.samples {
        let relative = Path::new(&sample.image);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| !matches!(component, Component::ParentDir)),
            "installed class-profile image path escapes the manifest directory"
        );
        let image_path = parent.join(relative);
        let image = fs::read(&image_path).with_context(|| {
            format!(
                "read installed class-profile image {}",
                image_path.display()
            )
        })?;
        ensure!(
            sha1_hex(&image) == sample.image_sha1,
            "installed class-profile image SHA-1 changed for {}",
            sample.label
        );
        image_hashes.insert(sample.image_sha1.as_str());
    }
    ensure!(
        image_hashes.len() == manifest.samples.len(),
        "installed class-profile route repeats an image"
    );

    Ok(ClassProfileRuntimeEvidence {
        manifest_sha1: sha1_hex(&manifest_bytes),
        sample_count: manifest.samples.len(),
        unique_image_count: image_hashes.len(),
    })
}
