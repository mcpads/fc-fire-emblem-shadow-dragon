use super::{
    SAMPLE_OFFSETS, evidence_scope,
    run_analysis::{expected_run_specs, validate_frame_sequence},
};

#[test]
fn variant_plan_covers_natural_direct_and_visible_routing_entries() {
    let specs = expected_run_specs();
    assert_eq!(specs.len(), 3);
    assert_eq!(specs[0].entries.len(), 7);
    assert_eq!(specs[1].entries.first(), Some(&(0x35, 0x40)));
    assert_eq!(specs[1].entries.last(), Some(&(0x01, 0x40)));
    assert_eq!(specs[2].entries.first(), Some(&(0x35, 0x41)));
    assert_eq!(specs[2].entries.last(), Some(&(0x02, 0x41)));
    assert_eq!(specs[2].allowed_extra_directories, &["no-op-entry-01"]);
}

#[test]
fn natural_plan_preserves_observed_direct_and_routing_actions() {
    let natural = &expected_run_specs()[0].entries;
    assert_eq!(
        natural,
        &[
            (0x07, 0x40),
            (0x06, 0x40),
            (0x05, 0x41),
            (0x04, 0x41),
            (0x03, 0x40),
            (0x02, 0x40),
            (0x01, 0x40),
        ]
    );
}

#[test]
fn producer_frames_must_match_irregular_capture_offsets() {
    assert_eq!(SAMPLE_OFFSETS, [7, 19, 43, 82, 171]);
    validate_frame_sequence(&[1007, 1019, 1043, 1082, 1171]).unwrap();
    let error = validate_frame_sequence(&[1007, 1019, 1043, 1083, 1171]).unwrap_err();
    assert!(error.to_string().contains("irregular sample offsets"));
}

#[test]
fn public_scope_excludes_dialogue_and_evidence_paths() {
    let scope = evidence_scope();
    assert_eq!(scope.translation_direction, "Japanese to Korean only");
    assert!(scope.preserve_existing_english_and_digits);
    assert!(!scope.dialogue_content_emitted);
    assert!(!scope.evidence_paths_emitted);
}
