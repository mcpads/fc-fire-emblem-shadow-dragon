use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::sha1_hex;

pub(super) struct ShopDialogueRuntimeEvidence {
    pub(super) manifest_sha1: String,
    pub(super) sample_count: usize,
    pub(super) unique_image_count: usize,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    format_version: u8,
    screen_role: String,
    output_sha1: String,
    mapper_register: u8,
    route: String,
    input_used: bool,
    funds_or_inventory_mutated: bool,
    facility_action_completed: bool,
    shared_item_names_translated: bool,
    shared_yes_no_labels_translated: bool,
    samples: Vec<RuntimeSample>,
}

#[derive(Debug, Deserialize)]
struct RuntimeSample {
    label: String,
    frame_count: u64,
    phase: String,
    outer_state: u8,
    korean_dialogue_visible: bool,
    original_digits_and_g_visible: bool,
    image: String,
    image_sha1: String,
}

pub(super) fn verify_shop_dialogue_runtime_evidence(
    manifest_path: &Path,
    output_sha1: &str,
    mapper_register: u8,
) -> Result<ShopDialogueRuntimeEvidence> {
    let manifest_bytes = fs::read(manifest_path).with_context(|| {
        format!(
            "read installed weapon-shop dialogue evidence {}",
            manifest_path.display()
        )
    })?;
    let manifest: RuntimeManifest = serde_json::from_slice(&manifest_bytes).with_context(|| {
        format!(
            "parse installed weapon-shop dialogue evidence {}",
            manifest_path.display()
        )
    })?;
    ensure!(
        manifest.format_version == 1
            && manifest.screen_role == "weapon_shop_dialogue_lifetime"
            && manifest.output_sha1.eq_ignore_ascii_case(output_sha1)
            && manifest.mapper_register == mapper_register,
        "installed weapon-shop dialogue runtime evidence is not bound to this output"
    );
    ensure!(
        manifest.route == "purchase_confirmation_decline_continue_no_exit"
            && manifest.input_used
            && !manifest.funds_or_inventory_mutated
            && manifest.facility_action_completed,
        "installed weapon-shop dialogue runtime route changed"
    );
    ensure!(
        !manifest.shared_item_names_translated && !manifest.shared_yes_no_labels_translated,
        "weapon-shop runtime evidence no longer describes the dialogue-only stage"
    );
    ensure!(
        manifest.samples.len() == 7,
        "installed weapon-shop dialogue evidence must contain seven route samples"
    );
    ensure!(
        manifest
            .samples
            .windows(2)
            .all(|pair| pair[0].frame_count < pair[1].frame_count),
        "installed weapon-shop dialogue frames are not strictly increasing"
    );
    let intervals = manifest
        .samples
        .windows(2)
        .map(|pair| pair[1].frame_count - pair[0].frame_count)
        .collect::<BTreeSet<_>>();
    ensure!(
        intervals.len() >= 4,
        "installed weapon-shop dialogue samples are not irregular"
    );

    let samples = manifest
        .samples
        .iter()
        .map(|sample| (sample.label.as_str(), sample))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        samples.len() == manifest.samples.len(),
        "installed weapon-shop dialogue evidence repeats a label"
    );
    for label in [
        "continue_prompt_plus_7",
        "continue_prompt_plus_19",
        "continue_prompt_plus_43",
    ] {
        let sample = samples
            .get(label)
            .with_context(|| format!("installed weapon-shop dialogue evidence lost {label}"))?;
        ensure!(
            sample.phase == "continue_prompt"
                && sample.outer_state == 12
                && sample.korean_dialogue_visible
                && sample.original_digits_and_g_visible,
            "installed weapon-shop continue prompt changed at {label}"
        );
    }
    for label in [
        "exit_text_drawing",
        "exit_message_plus_19",
        "exit_message_plus_43",
    ] {
        let sample = samples
            .get(label)
            .with_context(|| format!("installed weapon-shop dialogue evidence lost {label}"))?;
        ensure!(
            sample.phase == "exit_message"
                && sample.outer_state == 8
                && sample.korean_dialogue_visible
                && sample.original_digits_and_g_visible,
            "installed weapon-shop exit message changed at {label}"
        );
    }
    let map_restored = samples
        .get("map_restored_plus_43")
        .context("installed weapon-shop dialogue evidence lost map restoration")?;
    ensure!(
        map_restored.phase == "map_restored"
            && map_restored.outer_state == 0
            && !map_restored.korean_dialogue_visible
            && !map_restored.original_digits_and_g_visible,
        "installed weapon-shop dialogue route no longer restores the map"
    );

    let continue_hashes = [
        samples["continue_prompt_plus_7"].image_sha1.as_str(),
        samples["continue_prompt_plus_19"].image_sha1.as_str(),
        samples["continue_prompt_plus_43"].image_sha1.as_str(),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    ensure!(
        continue_hashes.len() == 1,
        "installed weapon-shop continue prompt is not temporally stable"
    );
    ensure!(
        samples["exit_message_plus_19"].image_sha1 == samples["exit_message_plus_43"].image_sha1,
        "installed weapon-shop exit message is not stable after text drawing"
    );

    let parent = manifest_path
        .parent()
        .context("installed weapon-shop dialogue evidence has no parent directory")?;
    let mut image_hashes = BTreeSet::new();
    for sample in &manifest.samples {
        let relative = Path::new(&sample.image);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| !matches!(component, Component::ParentDir)),
            "installed weapon-shop dialogue image path escapes the manifest directory"
        );
        let image_path = parent.join(relative);
        let image = fs::read(&image_path).with_context(|| {
            format!(
                "read installed weapon-shop dialogue image {}",
                image_path.display()
            )
        })?;
        ensure!(
            sha1_hex(&image) == sample.image_sha1,
            "installed weapon-shop dialogue image SHA-1 changed for {}",
            sample.label
        );
        image_hashes.insert(sample.image_sha1.as_str());
    }
    ensure!(
        image_hashes.len() == 4,
        "installed weapon-shop dialogue route image phases changed"
    );

    Ok(ShopDialogueRuntimeEvidence {
        manifest_sha1: sha1_hex(&manifest_bytes),
        sample_count: manifest.samples.len(),
        unique_image_count: image_hashes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_manifest_binds_the_decline_exit_and_map_restore_route() {
        let manifest = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../evidence/private/shop-dialogue-installed/manifest.json"
        ));
        if !manifest.exists() {
            return;
        }
        let evidence = verify_shop_dialogue_runtime_evidence(
            manifest,
            "41ba8b0a3924289ffa5ded73a90ad8b36028afef",
            0xC0,
        )
        .unwrap();

        assert_eq!(evidence.sample_count, 7);
        assert_eq!(evidence.unique_image_count, 4);
    }
}
