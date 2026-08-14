use super::*;
use crate::{
    full_translation_install::{
        runtime_code::resolve_request::INITIAL_PAGE_REQUEST_RESOLVER_ROLE,
        runtime_state_storage::{CANDIDATE_END, CANDIDATE_START},
    },
    rp2a03::{Instruction, assemble_at},
};

pub(in crate::full_translation_install::integrated_write_set) struct TechnicalInstallationCheckInputs<
    'a,
> {
    pub(in crate::full_translation_install::integrated_write_set) source: &'a [u8],
    pub(in crate::full_translation_install::integrated_write_set) installed: &'a [u8],
    pub(in crate::full_translation_install::integrated_write_set) required_mutations:
        &'a [MutationIdentity],
    pub(in crate::full_translation_install::integrated_write_set) actual_mutations:
        &'a [MutationIdentity],
    pub(in crate::full_translation_install::integrated_write_set) tracked_write_count: usize,
    pub(in crate::full_translation_install::integrated_write_set) all_required_dialogue_runtime_hook_roles_assembled:
        bool,
    pub(in crate::full_translation_install::integrated_write_set) runtime_state_initializer_installed:
        bool,
}

#[derive(Debug)]
pub(in crate::full_translation_install::integrated_write_set) struct RuntimeStateInitializerProof {
    pub(in crate::full_translation_install::integrated_write_set) required_identity_count: usize,
    pub(in crate::full_translation_install::integrated_write_set) actual_identity_count: usize,
    pub(in crate::full_translation_install::integrated_write_set) clears_full_reserved_range: bool,
    pub(in crate::full_translation_install::integrated_write_set) installed: bool,
}

#[derive(Debug)]
pub(in crate::full_translation_install::integrated_write_set) struct TechnicalInstallationProof {
    pub(in crate::full_translation_install::integrated_write_set) required_mutation_identity_sha1:
        String,
    pub(in crate::full_translation_install::integrated_write_set) actual_mutation_identity_sha1:
        String,
    pub(in crate::full_translation_install::integrated_write_set) image_growth_complete: bool,
    pub(in crate::full_translation_install::integrated_write_set) required_mutation_identity_set_complete:
        bool,
    pub(in crate::full_translation_install::integrated_write_set) required_runtime_routine_identities_installed:
        bool,
    pub(in crate::full_translation_install::integrated_write_set) required_runtime_hook_identities_installed:
        bool,
    pub(in crate::full_translation_install::integrated_write_set) final_replacement_bytes_match_manifest:
        bool,
    pub(in crate::full_translation_install::integrated_write_set) every_change_tracked: bool,
    pub(in crate::full_translation_install::integrated_write_set) technical_installation_complete:
        bool,
}

pub(in crate::full_translation_install::integrated_write_set) fn verify_technical_installation(
    inputs: TechnicalInstallationCheckInputs<'_>,
) -> Result<TechnicalInstallationProof> {
    let required_set = unique_mutation_identity_set(inputs.required_mutations);
    let actual_set = unique_mutation_identity_set(inputs.actual_mutations);
    let tracked_mutation_count = inputs
        .actual_mutations
        .iter()
        .filter(|identity| !identity.is_growth())
        .count();
    let required_mutation_identity_set_complete = required_set.is_some()
        && actual_set.is_some()
        && required_set == actual_set
        && inputs.tracked_write_count == tracked_mutation_count;
    let image_growth_complete = image_growth_matches_plan(
        inputs.source,
        inputs.installed,
        inputs.required_mutations,
        inputs.actual_mutations,
    );
    let required_runtime_routine_identities_installed =
        mutation_kind_matches_exactly(
            inputs.required_mutations,
            inputs.actual_mutations,
            |derivation| matches!(derivation, MutationDerivation::RuntimeRoutine { .. }),
        ) && final_bytes_match_kind(inputs.installed, inputs.actual_mutations, |derivation| {
            matches!(derivation, MutationDerivation::RuntimeRoutine { .. })
        });
    let required_runtime_hook_identities_installed = inputs
        .all_required_dialogue_runtime_hook_roles_assembled
        && mutation_kind_matches_exactly(
            inputs.required_mutations,
            inputs.actual_mutations,
            |derivation| matches!(derivation, MutationDerivation::RuntimeHook { .. }),
        )
        && final_bytes_match_kind(inputs.installed, inputs.actual_mutations, |derivation| {
            matches!(derivation, MutationDerivation::RuntimeHook { .. })
        });
    let final_replacement_bytes_match_manifest =
        final_bytes_match_kind(inputs.installed, inputs.actual_mutations, |derivation| {
            !matches!(derivation, MutationDerivation::AppendChrTail { .. })
        });
    let every_change_tracked =
        final_image_matches_mutation_plan(inputs.source, inputs.installed, inputs.actual_mutations);
    let technical_installation_complete = image_growth_complete
        && required_mutation_identity_set_complete
        && required_runtime_routine_identities_installed
        && required_runtime_hook_identities_installed
        && inputs.runtime_state_initializer_installed
        && final_replacement_bytes_match_manifest
        && every_change_tracked;
    ensure!(
        technical_installation_complete,
        "technical installation incomplete: mutation identities {}/{}, tracked {}/{}, growth {}, routines {}, hooks {}, runtime-state initializer {}, final replacements {}, source-to-final diff {}",
        inputs.actual_mutations.len(),
        inputs.required_mutations.len(),
        inputs.tracked_write_count,
        tracked_mutation_count,
        image_growth_complete,
        required_runtime_routine_identities_installed,
        required_runtime_hook_identities_installed,
        inputs.runtime_state_initializer_installed,
        final_replacement_bytes_match_manifest,
        every_change_tracked,
    );

    Ok(TechnicalInstallationProof {
        required_mutation_identity_sha1: mutation_identity_sha1(inputs.required_mutations)?,
        actual_mutation_identity_sha1: mutation_identity_sha1(inputs.actual_mutations)?,
        image_growth_complete,
        required_mutation_identity_set_complete,
        required_runtime_routine_identities_installed,
        required_runtime_hook_identities_installed,
        final_replacement_bytes_match_manifest,
        every_change_tracked,
        technical_installation_complete,
    })
}

pub(in crate::full_translation_install::integrated_write_set) fn verify_runtime_state_initializer_installation(
    required: &[MutationIdentity],
    actual: &[MutationIdentity],
    installed: &[u8],
) -> Result<RuntimeStateInitializerProof> {
    let required_initializers = runtime_state_initializer_identities(required);
    let actual_initializers = runtime_state_initializer_identities(actual);
    let required_identity_count = required_initializers.len();
    let actual_identity_count = actual_initializers.len();
    let clears_full_reserved_range = required_initializers
        .first()
        .is_some_and(|identity| initializer_has_typed_full_range_prefix(identity))
        && actual_initializers
            .first()
            .is_some_and(|identity| initializer_has_typed_full_range_prefix(identity));
    let installed_bytes_match = actual_initializers.first().is_some_and(|identity| {
        identity
            .offset
            .checked_add(identity.replacement.len())
            .and_then(|end| installed.get(identity.offset..end))
            == Some(identity.replacement.as_slice())
    });
    let installed = required_identity_count == 1
        && actual_identity_count == 1
        && required_initializers == actual_initializers
        && clears_full_reserved_range
        && installed_bytes_match;
    ensure!(
        installed,
        "runtime-state cold initializer is not one exact installed typed routine clearing 0x{CANDIDATE_START:04X}..0x{CANDIDATE_END:04X}"
    );
    Ok(RuntimeStateInitializerProof {
        required_identity_count,
        actual_identity_count,
        clears_full_reserved_range,
        installed,
    })
}

fn runtime_state_initializer_identities(identities: &[MutationIdentity]) -> Vec<&MutationIdentity> {
    identities
        .iter()
        .filter(|identity| {
            identity.role == INITIAL_PAGE_REQUEST_RESOLVER_ROLE
                && matches!(
                    identity.derivation,
                    MutationDerivation::RuntimeRoutine { .. }
                )
        })
        .collect()
}

fn initializer_has_typed_full_range_prefix(identity: &MutationIdentity) -> bool {
    let MutationDerivation::RuntimeRoutine { cpu_address } = identity.derivation else {
        return false;
    };
    let mut instructions = vec![Instruction::LdaImmediate(0)];
    instructions.extend((CANDIDATE_START..=CANDIDATE_END).map(Instruction::StaAbsolute));
    assemble_at(cpu_address, &instructions)
        .is_ok_and(|prefix| identity.replacement.starts_with(&prefix))
}

pub(in crate::full_translation_install::integrated_write_set) fn unique_mutation_identity_set(
    identities: &[MutationIdentity],
) -> Option<BTreeSet<MutationIdentity>> {
    let set = identities.iter().cloned().collect::<BTreeSet<_>>();
    (set.len() == identities.len()).then_some(set)
}

fn mutation_kind_matches_exactly(
    required: &[MutationIdentity],
    actual: &[MutationIdentity],
    predicate: impl Fn(&MutationDerivation) -> bool,
) -> bool {
    required
        .iter()
        .filter(|identity| predicate(&identity.derivation))
        .cloned()
        .collect::<BTreeSet<_>>()
        == actual
            .iter()
            .filter(|identity| predicate(&identity.derivation))
            .cloned()
            .collect::<BTreeSet<_>>()
}

fn final_bytes_match_kind(
    installed: &[u8],
    identities: &[MutationIdentity],
    predicate: impl Fn(&MutationDerivation) -> bool,
) -> bool {
    identities
        .iter()
        .filter(|identity| predicate(&identity.derivation))
        .all(|identity| {
            identity
                .offset
                .checked_add(identity.replacement.len())
                .and_then(|end| installed.get(identity.offset..end))
                == Some(identity.replacement.as_slice())
        })
}

fn image_growth_matches_plan(
    source: &[u8],
    installed: &[u8],
    required: &[MutationIdentity],
    actual: &[MutationIdentity],
) -> bool {
    let required_growth = required
        .iter()
        .filter(|identity| identity.is_growth())
        .collect::<Vec<_>>();
    let actual_growth = actual
        .iter()
        .filter(|identity| identity.is_growth())
        .collect::<Vec<_>>();
    if required_growth != actual_growth || required_growth.len() > 1 {
        return false;
    }
    let Some(growth) = required_growth.first() else {
        return installed.len() == source.len();
    };
    let MutationDerivation::AppendChrTail {
        page_count,
        fill_byte,
    } = growth.derivation
    else {
        return false;
    };
    growth.role == CHR_APPEND_ROLE
        && growth.offset == source.len()
        && growth.expected.is_empty()
        && fill_byte == CHR_APPEND_FILL_BYTE
        && growth.replacement.len() == page_count * FONT_PAGE_SIZE
        && growth.replacement.iter().all(|byte| *byte == fill_byte)
        && installed.len() == source.len() + growth.replacement.len()
}

fn final_image_matches_mutation_plan(
    source: &[u8],
    installed: &[u8],
    identities: &[MutationIdentity],
) -> bool {
    materialize_mutation_plan(source, identities).as_deref() == Some(installed)
}

pub(in crate::full_translation_install::integrated_write_set) fn materialize_mutation_plan(
    source: &[u8],
    identities: &[MutationIdentity],
) -> Option<Vec<u8>> {
    let growth = identities
        .iter()
        .filter(|identity| identity.is_growth())
        .collect::<Vec<_>>();
    if growth.len() > 1 {
        return None;
    }
    let mut baseline = source.to_vec();
    if let Some(growth) = growth.first() {
        if growth.offset != source.len() || !growth.expected.is_empty() {
            return None;
        }
        baseline.extend_from_slice(&growth.replacement);
    }
    let mut reconstructed = baseline.clone();
    let mut covered = vec![false; baseline.len()];
    for identity in identities.iter().filter(|identity| !identity.is_growth()) {
        if identity.expected.len() != identity.replacement.len() {
            return None;
        }
        let Some(end) = identity.offset.checked_add(identity.expected.len()) else {
            return None;
        };
        if baseline.get(identity.offset..end) != Some(identity.expected.as_slice()) {
            return None;
        }
        let Some(range) = covered.get_mut(identity.offset..end) else {
            return None;
        };
        if range.iter().any(|covered| *covered) {
            return None;
        }
        range.fill(true);
        reconstructed[identity.offset..end].copy_from_slice(&identity.replacement);
    }
    Some(reconstructed)
}

fn mutation_identity_sha1(identities: &[MutationIdentity]) -> Result<String> {
    let mut ordered = identities.iter().collect::<Vec<_>>();
    ordered.sort();
    let mut identity = Vec::new();
    for write in ordered {
        identity.extend_from_slice(
            &u64::try_from(write.role.len())
                .context("mutation identity role length exceeds u64")?
                .to_le_bytes(),
        );
        identity.extend_from_slice(write.role.as_bytes());
        identity.extend_from_slice(
            &u64::try_from(write.offset)
                .context("mutation identity offset exceeds u64")?
                .to_le_bytes(),
        );
        identity.extend_from_slice(
            &u64::try_from(write.expected.len())
                .context("mutation identity expected length exceeds u64")?
                .to_le_bytes(),
        );
        identity.extend_from_slice(
            &u64::try_from(write.replacement.len())
                .context("mutation identity replacement length exceeds u64")?
                .to_le_bytes(),
        );
        identity.extend_from_slice(&write.expected);
        identity.extend_from_slice(&write.replacement);
        match &write.derivation {
            MutationDerivation::ExactReplacement => identity.push(0),
            MutationDerivation::AppendChrTail {
                page_count,
                fill_byte,
            } => {
                identity.push(1);
                identity.extend_from_slice(
                    &u64::try_from(*page_count)
                        .context("append page count exceeds u64")?
                        .to_le_bytes(),
                );
                identity.push(*fill_byte);
            }
            MutationDerivation::RuntimeRoutine { cpu_address } => {
                identity.push(2);
                identity.extend_from_slice(&cpu_address.to_le_bytes());
            }
            MutationDerivation::RuntimeHook { hook_role, site } => {
                identity.push(3);
                let role = serde_json::to_vec(hook_role)
                    .context("serialize runtime hook role identity")?;
                identity.extend_from_slice(
                    &u64::try_from(role.len())
                        .context("runtime hook role identity length exceeds u64")?
                        .to_le_bytes(),
                );
                identity.extend_from_slice(&role);
                match site {
                    RuntimeHookSiteIdentity::Fixed(address) => {
                        identity.push(0);
                        identity.extend_from_slice(&address.to_le_bytes());
                    }
                    RuntimeHookSiteIdentity::Switchable { bank, address } => {
                        identity.push(1);
                        identity.push(*bank);
                        identity.extend_from_slice(&address.to_le_bytes());
                    }
                }
            }
        }
    }
    Ok(sha1_hex(&identity))
}
