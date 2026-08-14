use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;

use super::*;

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[test]
fn exact_artifact_and_irregular_digest_bound_samples_are_admitted() {
    let fixture = RuntimeFixture::new();
    let artifact_sha1 = "11".repeat(20);
    fixture.write_manifest(json!({
        "artifact_sha1": artifact_sha1,
        "runs": [{
            "run_id": "cold-front-end",
            "started_from_cold_boot": true,
            "savestate_used": false,
            "observations": [{
                "screen_role": "save_slot_selection",
                "kind": "representative",
                "hangul_readable": true,
                "japanese_target_text_absent": true,
                "protected_original_text": "preserved",
                "visual_glitch_absent_across_samples": true,
                "samples": fixture.samples([0, 47, 130])
            }]
        }]
    }));

    let evidence =
        load_final_artifact_runtime_evidence(Some(&fixture.manifest_path), &artifact_sha1).unwrap();

    assert!(evidence.verification_started());
    assert_eq!(
        evidence.bound_screen_roles(),
        BTreeSet::from(["save_slot_selection".to_owned()])
    );
}

#[test]
fn evidence_for_another_artifact_is_rejected() {
    let fixture = RuntimeFixture::new();
    fixture.write_manifest(json!({
        "artifact_sha1": "22".repeat(20),
        "runs": [{
            "run_id": "wrong-image",
            "started_from_cold_boot": true,
            "savestate_used": false,
            "observations": [{
                "screen_role": "save_slot_selection",
                "kind": "representative",
                "hangul_readable": true,
                "japanese_target_text_absent": true,
                "protected_original_text": "preserved",
                "visual_glitch_absent_across_samples": true,
                "samples": fixture.samples([0, 47, 130])
            }]
        }]
    }));

    let error =
        load_final_artifact_runtime_evidence(Some(&fixture.manifest_path), &"11".repeat(20))
            .err()
            .unwrap();
    assert!(format!("{error:#}").contains("names ROM"));
}

#[test]
fn regular_frame_sampling_is_rejected() {
    let fixture = RuntimeFixture::new();
    let artifact_sha1 = "11".repeat(20);
    fixture.write_manifest(json!({
        "artifact_sha1": artifact_sha1,
        "runs": [{
            "run_id": "regular-sampling",
            "started_from_cold_boot": true,
            "savestate_used": false,
            "observations": [{
                "screen_role": "save_slot_selection",
                "kind": "representative",
                "hangul_readable": true,
                "japanese_target_text_absent": true,
                "protected_original_text": "preserved",
                "visual_glitch_absent_across_samples": true,
                "samples": fixture.samples([0, 60, 120])
            }]
        }]
    }));

    let error = load_final_artifact_runtime_evidence(Some(&fixture.manifest_path), &artifact_sha1)
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("irregular offsets"));
}

#[test]
fn a_single_page_cannot_bind_the_multi_page_main_dialogue_role() {
    let fixture = RuntimeFixture::new();
    let artifact_sha1 = "11".repeat(20);
    fixture.write_manifest(json!({
        "artifact_sha1": artifact_sha1,
        "runs": [{
            "run_id": "single-dialogue-page",
            "started_from_cold_boot": true,
            "savestate_used": false,
            "observations": [{
                "screen_role": MULTI_PAGE_MAIN_DIALOGUE_ROLE,
                "kind": "representative",
                "hangul_readable": true,
                "japanese_target_text_absent": true,
                "protected_original_text": "preserved",
                "visual_glitch_absent_across_samples": true,
                "samples": fixture.samples([0, 47, 130])
            }]
        }]
    }));

    let error = load_final_artifact_runtime_evidence(Some(&fixture.manifest_path), &artifact_sha1)
        .err()
        .unwrap();

    assert!(format!("{error:#}").contains("has no progression result"));
}

#[test]
fn distinct_pages_following_record_and_exit_bind_main_dialogue_progression() {
    let fixture = RuntimeFixture::new();
    let artifact_sha1 = "11".repeat(20);
    fixture.write_manifest(json!({
        "artifact_sha1": artifact_sha1,
        "runs": [{
            "run_id": "complete-dialogue-progression",
            "started_from_cold_boot": true,
            "savestate_used": false,
            "observations": [{
                "screen_role": MULTI_PAGE_MAIN_DIALOGUE_ROLE,
                "kind": "representative",
                "hangul_readable": true,
                "japanese_target_text_absent": true,
                "protected_original_text": "preserved",
                "visual_glitch_absent_across_samples": true,
                "main_dialogue_progression": {
                    "distinct_visible_page_sample_offsets": [0, 840],
                    "following_record_sample_offset": 1650,
                    "role_exit_sample_offset": 3274
                },
                "samples": fixture.samples_at(&[0, 840, 1650, 3274])
            }]
        }]
    }));

    let evidence =
        load_final_artifact_runtime_evidence(Some(&fixture.manifest_path), &artifact_sha1).unwrap();

    assert!(
        evidence
            .bound_screen_roles()
            .contains(MULTI_PAGE_MAIN_DIALOGUE_ROLE)
    );
}

struct RuntimeFixture {
    directory: PathBuf,
    manifest_path: PathBuf,
    image_sha256: String,
}

impl RuntimeFixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "fc-fire-emblem-final-runtime-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let image_bytes = b"synthetic runtime frame";
        fs::write(directory.join("frame.png"), image_bytes).unwrap();
        Self {
            manifest_path: directory.join("manifest.json"),
            image_sha256: format!("{:x}", Sha256::digest(image_bytes)),
            directory,
        }
    }

    fn samples(&self, offsets: [u64; 3]) -> Vec<serde_json::Value> {
        self.samples_at(&offsets)
    }

    fn samples_at(&self, offsets: &[u64]) -> Vec<serde_json::Value> {
        offsets
            .iter()
            .copied()
            .map(|frame_offset| {
                json!({
                    "frame_offset": frame_offset,
                    "image": "frame.png",
                    "image_sha256": self.image_sha256,
                })
            })
            .collect()
    }

    fn write_manifest(&self, value: serde_json::Value) {
        fs::write(
            &self.manifest_path,
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
}

impl Drop for RuntimeFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
