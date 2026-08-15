use super::*;
use crate::full_translation_install::runtime_code::RuntimeRoutine;

fn check_inputs<'a>(
    source: &'a [u8],
    installed: &'a [u8],
    required_mutations: &'a [MutationIdentity],
    actual_mutations: &'a [MutationIdentity],
    tracked_write_count: usize,
) -> TechnicalInstallationCheckInputs<'a> {
    TechnicalInstallationCheckInputs {
        source,
        installed,
        required_mutations,
        actual_mutations,
        tracked_write_count,
        all_required_dialogue_runtime_hook_roles_assembled: true,
        runtime_state_initializer_installed: true,
    }
}

fn synthetic_expanded_prg_rom() -> Rom {
    let mut bytes = vec![0xFF; HEADER_SIZE + 512 * 1024];
    bytes[..HEADER_SIZE].fill(0);
    bytes[..4].copy_from_slice(b"NES\x1A");
    bytes[4] = 32;
    bytes[5] = 0;
    Rom::parse(bytes).unwrap()
}

#[test]
fn four_page_chr_growth_is_a_source_to_final_mutation() {
    let mut bytes = vec![0xFF; HEADER_SIZE + 16 * 1024 + 8 * 1024];
    bytes[..HEADER_SIZE].fill(0);
    bytes[..4].copy_from_slice(b"NES\x1A");
    bytes[4] = 1;
    bytes[5] = 1;
    let candidate = Rom::parse(bytes).unwrap();

    let growth = plan_candidate_image_growth_for_highest_page(&candidate, 5).unwrap();
    let expanded = growth.apply(candidate.data()).unwrap();
    let append = growth.append_identity().unwrap();

    assert_eq!(growth.appended_chr_page_count, 4);
    assert_eq!(growth.appended_chr_byte_count, 16 * 1024);
    assert_eq!(growth.final_chr_bank_count, 3);
    assert_eq!(append.offset, candidate.data().len());
    assert_eq!(expanded.len(), candidate.data().len() + 16 * 1024);
    assert!(
        expanded[candidate.data().len()..]
            .iter()
            .all(|byte| *byte == CHR_APPEND_FILL_BYTE)
    );
}

#[test]
fn append_tail_and_overlay_reconstruct_the_exact_final_image() {
    let source = vec![0_u8; 8];
    let growth = ImageGrowthPlan {
        source_byte_count: source.len(),
        final_byte_count: source.len() + FONT_PAGE_SIZE,
        appended_chr_page_count: 1,
        appended_chr_byte_count: FONT_PAGE_SIZE,
        final_chr_bank_count: 1,
    };
    let baseline = growth.apply(&source).unwrap();
    let mut image = IntegratedImage::new(baseline, growth.append_identity());
    image
        .write_expected(
            "first appended glyph byte",
            source.len(),
            &[CHR_APPEND_FILL_BYTE],
            &[0x42],
        )
        .unwrap();
    let actual = image.mutation_identities().to_vec();
    let tracked_write_count = image.writes().len();
    let installed = image.into_data();

    let proof = verify_technical_installation(check_inputs(
        &source,
        &installed,
        &actual,
        &actual,
        tracked_write_count,
    ))
    .unwrap();
    assert!(proof.image_growth_complete);
    assert!(proof.required_mutation_identity_set_complete);
    assert!(proof.every_change_tracked);

    assert!(
        verify_technical_installation(check_inputs(
            &source,
            &installed[..installed.len() - 1],
            &actual,
            &actual,
            tracked_write_count,
        ))
        .is_err()
    );

    let mut wrong_growth = actual.clone();
    let growth_identity = wrong_growth
        .iter_mut()
        .find(|identity| identity.is_growth())
        .unwrap();
    growth_identity.derivation = MutationDerivation::AppendChrTail {
        page_count: 1,
        fill_byte: 0,
    };
    assert!(
        verify_technical_installation(check_inputs(
            &source,
            &installed,
            &wrong_growth,
            &wrong_growth,
            tracked_write_count,
        ))
        .is_err()
    );
}

#[test]
fn equal_counts_cannot_compensate_for_a_missing_and_extra_mutation() {
    let source = vec![0_u8; 8];
    let required = vec![
        MutationIdentity::exact("required A", 1, &[0], &[1]),
        MutationIdentity::exact("required B", 3, &[0], &[2]),
    ];
    let actual = vec![
        MutationIdentity::exact("required A", 1, &[0], &[1]),
        MutationIdentity::exact("unplanned C", 5, &[0], &[3]),
    ];
    let installed = materialize_mutation_plan(&source, &actual).unwrap();

    assert_eq!(required.len(), actual.len());
    assert!(
        verify_technical_installation(check_inputs(
            &source,
            &installed,
            &required,
            &actual,
            actual.len(),
        ))
        .is_err()
    );
}

#[test]
fn runtime_routine_and_hook_derivations_are_part_of_identity() {
    let source = vec![0_u8; 8];
    let required_routine =
        MutationIdentity::runtime_routine("resolver", 1, &[0, 0], &[0x20, 0x80], 0xA010);
    let generic_actual = MutationIdentity::exact("resolver", 1, &[0, 0], &[0x20, 0x80]);
    let installed =
        materialize_mutation_plan(&source, std::slice::from_ref(&generic_actual)).unwrap();
    assert!(
        verify_technical_installation(check_inputs(
            &source,
            &installed,
            std::slice::from_ref(&required_routine),
            std::slice::from_ref(&generic_actual),
            1,
        ))
        .is_err()
    );

    let required_hook = MutationIdentity::runtime_hook(
        "NMI hook",
        4,
        &[0, 0],
        &[0x20, 0x90],
        DialogueRuntimeHookRole::NmiPageComposer,
        RuntimeHookSiteIdentity::Fixed(0xC179),
    );
    let wrong_site_hook = MutationIdentity::runtime_hook(
        "NMI hook",
        4,
        &[0, 0],
        &[0x20, 0x90],
        DialogueRuntimeHookRole::NmiPageComposer,
        RuntimeHookSiteIdentity::Fixed(0xC17A),
    );
    let installed =
        materialize_mutation_plan(&source, std::slice::from_ref(&wrong_site_hook)).unwrap();
    assert!(
        verify_technical_installation(check_inputs(
            &source,
            &installed,
            std::slice::from_ref(&required_hook),
            std::slice::from_ref(&wrong_site_hook),
            1,
        ))
        .is_err()
    );
}

#[test]
fn runtime_state_initializer_preserves_the_concurrent_consumer_page() {
    use crate::{
        full_translation_install::{
            runtime_code::resolve_request::INITIAL_PAGE_REQUEST_RESOLVER_ROLE,
            runtime_state_storage::{
                CANDIDATE_START, CONSUMER_FONT_PAGE, DIALOGUE_RUNTIME_STATE_END,
            },
        },
        rp2a03::{Instruction, assemble_at},
    };

    let cpu_address = 0xA100;
    let mut instructions = vec![Instruction::LdaImmediate(0)];
    instructions
        .extend((CANDIDATE_START..=DIALOGUE_RUNTIME_STATE_END).map(Instruction::StaAbsolute));
    let mut replacement = assemble_at(cpu_address, &instructions).unwrap();
    replacement.push(0x60);
    let source = vec![0xFF; replacement.len()];
    let initializer = MutationIdentity::runtime_routine(
        INITIAL_PAGE_REQUEST_RESOLVER_ROLE,
        0,
        &source,
        &replacement,
        cpu_address,
    );
    let installed = materialize_mutation_plan(&source, std::slice::from_ref(&initializer)).unwrap();
    let proof = verify_runtime_state_initializer_installation(
        std::slice::from_ref(&initializer),
        std::slice::from_ref(&initializer),
        &installed,
    )
    .unwrap();
    assert_eq!(proof.required_identity_count, 1);
    assert!(proof.preserves_consumer_font_page);
    assert!(!initializer.replacement.windows(3).any(|window| {
        window
            == [
                0x8D,
                CONSUMER_FONT_PAGE as u8,
                (CONSUMER_FONT_PAGE >> 8) as u8,
            ]
    }));
    assert!(proof.installed);
    assert!(
        verify_technical_installation(check_inputs(
            &source,
            &installed,
            std::slice::from_ref(&initializer),
            std::slice::from_ref(&initializer),
            1,
        ))
        .is_ok()
    );
    let mut missing_initializer_proof = check_inputs(
        &source,
        &installed,
        std::slice::from_ref(&initializer),
        std::slice::from_ref(&initializer),
        1,
    );
    missing_initializer_proof.runtime_state_initializer_installed = false;
    assert!(verify_technical_installation(missing_initializer_proof).is_err());

    let mut truncated = initializer;
    truncated.replacement.drain(2..5);
    assert!(
        verify_runtime_state_initializer_installation(
            std::slice::from_ref(&truncated),
            std::slice::from_ref(&truncated),
            &installed,
        )
        .is_err()
    );
}

#[test]
fn runtime_material_production_wiring_emits_every_routine_as_its_own_identity() {
    let candidate = synthetic_expanded_prg_rom();
    let mut material = vec![0xFF; 5 * MMC3_PAGE_BYTE_COUNT];
    material[0] = b'F';
    let routine = RuntimeRoutine {
        role: "runtime state initializer and resolver",
        address: 0xA012,
        bytes: vec![0xA9, 0x00, 0x85, 0xF0],
    };
    let routine_offset = 4 * MMC3_PAGE_BYTE_COUNT + 0x12;
    material[routine_offset..routine_offset + routine.bytes.len()].copy_from_slice(&routine.bytes);
    let code_plan = DialogueRuntimeCodePlan {
        code_routines: vec![routine],
        fixed_routines: Vec::new(),
        reclaimed_fixed_routines: Vec::new(),
        hooks: Vec::new(),
        chr_restore_callee_cycles: [(0, 0); 2],
    };

    let required =
        plan_required_runtime_material_mutations(&candidate, &material, &code_plan).unwrap();
    let mut image = IntegratedImage::new(candidate.data().to_vec(), None);
    super::super::install_dialogue_runtime_material(&mut image, &candidate, &material, &code_plan)
        .unwrap();
    let actual = image.mutation_identities().to_vec();
    let tracked_write_count = image.writes().len();
    let installed = image.into_data();

    assert!(required.iter().any(|identity| matches!(
        identity.derivation,
        MutationDerivation::RuntimeRoutine {
            cpu_address: 0xA012
        }
    )));
    let proof = verify_technical_installation(check_inputs(
        candidate.data(),
        &installed,
        &required,
        &actual,
        tracked_write_count,
    ))
    .unwrap();
    assert!(proof.required_runtime_routine_identities_installed);

    let mut missing_routine_material = material;
    missing_routine_material[routine_offset..routine_offset + 4].fill(0xFF);
    assert!(
        verify_runtime_material_code_projection(&missing_routine_material, &code_plan).is_err()
    );
}

#[test]
fn final_byte_drift_or_unregistered_change_fails_the_source_to_final_audit() {
    let source = vec![0_u8; 8];
    let mutation = MutationIdentity::exact("runtime material", 2, &[0, 0], &[0x44, 0x55]);
    let identities = vec![mutation];
    let installed = materialize_mutation_plan(&source, &identities).unwrap();

    let mut replacement_drift = installed.clone();
    replacement_drift[2] ^= 0xFF;
    assert!(
        verify_technical_installation(check_inputs(
            &source,
            &replacement_drift,
            &identities,
            &identities,
            1,
        ))
        .is_err()
    );

    let mut unregistered = installed;
    unregistered[7] = 0x99;
    assert!(
        verify_technical_installation(check_inputs(
            &source,
            &unregistered,
            &identities,
            &identities,
            1,
        ))
        .is_err()
    );
}
