use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::sha1_hex;

pub(super) struct WeaponShopSharedTextRuntimeEvidence {
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
    original_english_digits_and_g_preserved: bool,
    item_list_selector_address: String,
    selected_item_selector_address: String,
    original_table_read_after_selected_hook: bool,
    samples: Vec<RuntimeSample>,
}

#[derive(Debug, Deserialize)]
struct RuntimeSample {
    label: String,
    frame_count: u64,
    phase: String,
    outer_state: u8,
    item_list_visible: bool,
    selected_item_visible: bool,
    choice_labels_visible: bool,
    korean_dialogue_visible: bool,
    original_digits_and_g_visible: bool,
    image: String,
    image_sha1: String,
}

pub(super) fn verify_weapon_shop_shared_text_runtime_evidence(
    manifest_path: &Path,
    output_sha1: &str,
    mapper_register: u8,
) -> Result<WeaponShopSharedTextRuntimeEvidence> {
    let manifest_bytes = fs::read(manifest_path).with_context(|| {
        format!(
            "read installed weapon-shop shared-text evidence {}",
            manifest_path.display()
        )
    })?;
    let manifest: RuntimeManifest = serde_json::from_slice(&manifest_bytes).with_context(|| {
        format!(
            "parse installed weapon-shop shared-text evidence {}",
            manifest_path.display()
        )
    })?;
    ensure!(
        manifest.format_version == 1
            && manifest.screen_role == "weapon_shop_shared_text"
            && manifest.output_sha1.eq_ignore_ascii_case(output_sha1)
            && manifest.mapper_register == mapper_register,
        "installed weapon-shop shared-text runtime evidence is not bound to this output"
    );
    ensure!(
        manifest.route == "confirmation_decline_continue_return_to_items_exit_to_map"
            && manifest.input_used
            && !manifest.funds_or_inventory_mutated
            && manifest.facility_action_completed
            && manifest.original_english_digits_and_g_preserved,
        "installed weapon-shop shared-text route changed"
    );
    ensure!(
        manifest.item_list_selector_address == "0xF3D0"
            && manifest.selected_item_selector_address == "0xF4DE"
            && !manifest.original_table_read_after_selected_hook,
        "installed weapon-shop shared-text selector proof changed"
    );
    ensure!(
        manifest.samples.len() == 17,
        "installed weapon-shop shared-text evidence must contain seventeen route samples"
    );
    ensure!(
        manifest
            .samples
            .windows(2)
            .all(|pair| pair[0].frame_count < pair[1].frame_count),
        "installed weapon-shop shared-text frames are not strictly increasing"
    );
    let intervals = manifest
        .samples
        .windows(2)
        .map(|pair| pair[1].frame_count - pair[0].frame_count)
        .collect::<BTreeSet<_>>();
    ensure!(
        intervals.len() >= 6,
        "installed weapon-shop shared-text samples are not irregular"
    );

    let samples = manifest
        .samples
        .iter()
        .map(|sample| (sample.label.as_str(), sample))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        samples.len() == manifest.samples.len(),
        "installed weapon-shop shared-text evidence repeats a label"
    );
    for sample in manifest
        .samples
        .iter()
        .filter(|sample| sample.phase == "confirmation")
    {
        ensure!(
            sample.outer_state == 4
                && sample.item_list_visible
                && sample.selected_item_visible
                && sample.choice_labels_visible
                && sample.korean_dialogue_visible
                && sample.original_digits_and_g_visible,
            "installed weapon-shop confirmation changed at {}",
            sample.label
        );
    }
    for sample in manifest
        .samples
        .iter()
        .filter(|sample| sample.phase == "continue_prompt")
    {
        ensure!(
            sample.outer_state == 12
                && !sample.item_list_visible
                && sample.selected_item_visible
                && !sample.choice_labels_visible
                && sample.korean_dialogue_visible
                && !sample.original_digits_and_g_visible,
            "installed weapon-shop continue prompt changed at {}",
            sample.label
        );
    }
    for sample in manifest
        .samples
        .iter()
        .filter(|sample| sample.phase == "item_list_return")
    {
        ensure!(
            sample.outer_state == 4
                && sample.item_list_visible
                && !sample.selected_item_visible
                && !sample.choice_labels_visible
                && sample.korean_dialogue_visible
                && sample.original_digits_and_g_visible,
            "installed weapon-shop item-list return changed at {}",
            sample.label
        );
    }
    for sample in manifest
        .samples
        .iter()
        .filter(|sample| sample.phase == "exit_message")
    {
        ensure!(
            sample.outer_state == 8
                && !sample.item_list_visible
                && sample.selected_item_visible
                && !sample.choice_labels_visible
                && sample.korean_dialogue_visible
                && sample.original_digits_and_g_visible,
            "installed weapon-shop exit message changed at {}",
            sample.label
        );
    }
    let map_restored = samples
        .get("map_restored_plus_43")
        .context("installed weapon-shop shared-text evidence lost map restoration")?;
    ensure!(
        map_restored.phase == "map_restored"
            && map_restored.outer_state == 0
            && !map_restored.item_list_visible
            && !map_restored.selected_item_visible
            && !map_restored.choice_labels_visible
            && !map_restored.korean_dialogue_visible
            && !map_restored.original_digits_and_g_visible,
        "installed weapon-shop shared-text route no longer restores the map"
    );

    let expected_phase_counts = BTreeMap::from([
        ("confirmation", 4usize),
        ("continue_prompt", 4),
        ("item_list_return", 4),
        ("exit_message", 4),
        ("map_restored", 1),
    ]);
    let mut phase_hashes = BTreeMap::<&str, BTreeSet<&str>>::new();
    let mut phase_counts = BTreeMap::<&str, usize>::new();
    for sample in &manifest.samples {
        phase_hashes
            .entry(sample.phase.as_str())
            .or_default()
            .insert(sample.image_sha1.as_str());
        *phase_counts.entry(sample.phase.as_str()).or_default() += 1;
    }
    ensure!(
        phase_counts == expected_phase_counts,
        "installed weapon-shop shared-text phase coverage changed"
    );
    ensure!(
        phase_hashes.values().all(|hashes| hashes.len() == 1),
        "installed weapon-shop shared-text phase is not temporally stable"
    );

    let parent = manifest_path
        .parent()
        .context("installed weapon-shop shared-text evidence has no parent directory")?;
    let mut image_hashes = BTreeSet::new();
    for sample in &manifest.samples {
        let relative = Path::new(&sample.image);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| !matches!(component, Component::ParentDir)),
            "installed weapon-shop shared-text image path escapes the manifest directory"
        );
        let image_path = parent.join(relative);
        let image = fs::read(&image_path).with_context(|| {
            format!(
                "read installed weapon-shop shared-text image {}",
                image_path.display()
            )
        })?;
        ensure!(
            sha1_hex(&image) == sample.image_sha1,
            "installed weapon-shop shared-text image SHA-1 changed for {}",
            sample.label
        );
        image_hashes.insert(sample.image_sha1.as_str());
    }
    ensure!(
        image_hashes.len() == expected_phase_counts.len(),
        "installed weapon-shop shared-text route image phases changed"
    );

    Ok(WeaponShopSharedTextRuntimeEvidence {
        manifest_sha1: sha1_hex(&manifest_bytes),
        sample_count: manifest.samples.len(),
        unique_image_count: image_hashes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_manifest_binds_the_shared_text_decline_and_exit_route() {
        let manifest = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../evidence/private/shop-shared-text-installed/manifest.json"
        ));
        if !manifest.exists() {
            return;
        }
        let evidence = verify_weapon_shop_shared_text_runtime_evidence(
            manifest,
            "4758d15b9beb9b075be6976508b90e17ef1ea54d",
            0xC0,
        )
        .unwrap();

        assert_eq!(evidence.sample_count, 17);
        assert_eq!(evidence.unique_image_count, 5);
    }
}
