use super::*;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum RuntimeHookSiteIdentity {
    Fixed(u16),
    Switchable { bank: u8, address: u16 },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum MutationDerivation {
    ExactReplacement,
    AppendChrTail {
        page_count: usize,
        fill_byte: u8,
    },
    RuntimeRoutine {
        cpu_address: u16,
    },
    RuntimeHook {
        hook_role: DialogueRuntimeHookRole,
        site: RuntimeHookSiteIdentity,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct MutationIdentity {
    pub(super) role: String,
    pub(super) offset: usize,
    pub(super) expected: Vec<u8>,
    pub(super) replacement: Vec<u8>,
    pub(super) derivation: MutationDerivation,
}

impl MutationIdentity {
    pub(super) fn exact(
        role: impl Into<String>,
        offset: usize,
        expected: &[u8],
        replacement: &[u8],
    ) -> Self {
        Self {
            role: role.into(),
            offset,
            expected: expected.to_vec(),
            replacement: replacement.to_vec(),
            derivation: MutationDerivation::ExactReplacement,
        }
    }

    pub(super) fn runtime_routine(
        role: impl Into<String>,
        offset: usize,
        expected: &[u8],
        replacement: &[u8],
        cpu_address: u16,
    ) -> Self {
        Self {
            role: role.into(),
            offset,
            expected: expected.to_vec(),
            replacement: replacement.to_vec(),
            derivation: MutationDerivation::RuntimeRoutine { cpu_address },
        }
    }

    pub(super) fn runtime_hook(
        role: impl Into<String>,
        offset: usize,
        expected: &[u8],
        replacement: &[u8],
        hook_role: DialogueRuntimeHookRole,
        site: RuntimeHookSiteIdentity,
    ) -> Self {
        Self {
            role: role.into(),
            offset,
            expected: expected.to_vec(),
            replacement: replacement.to_vec(),
            derivation: MutationDerivation::RuntimeHook { hook_role, site },
        }
    }

    pub(super) fn is_growth(&self) -> bool {
        matches!(self.derivation, MutationDerivation::AppendChrTail { .. })
    }
}

#[derive(Clone, Debug)]
pub(super) struct ImageGrowthPlan {
    pub(super) source_byte_count: usize,
    pub(super) final_byte_count: usize,
    pub(super) appended_chr_page_count: usize,
    pub(super) appended_chr_byte_count: usize,
    pub(super) final_chr_bank_count: u8,
}

impl ImageGrowthPlan {
    pub(super) fn append_identity(&self) -> Option<MutationIdentity> {
        (self.appended_chr_byte_count != 0).then(|| MutationIdentity {
            role: CHR_APPEND_ROLE.to_owned(),
            offset: self.source_byte_count,
            expected: Vec::new(),
            replacement: vec![CHR_APPEND_FILL_BYTE; self.appended_chr_byte_count],
            derivation: MutationDerivation::AppendChrTail {
                page_count: self.appended_chr_page_count,
                fill_byte: CHR_APPEND_FILL_BYTE,
            },
        })
    }

    pub(super) fn apply(&self, source: &[u8]) -> Result<Vec<u8>> {
        ensure!(
            source.len() == self.source_byte_count
                && self.final_byte_count == self.source_byte_count + self.appended_chr_byte_count,
            "integrated CHR growth no longer begins at the exact candidate end"
        );
        let mut expanded = source.to_vec();
        expanded.resize(self.final_byte_count, CHR_APPEND_FILL_BYTE);
        Ok(expanded)
    }
}

pub(super) struct IntegratedImage {
    tracked: TrackedImage,
    mutation_identities: Vec<MutationIdentity>,
}

impl IntegratedImage {
    pub(super) fn new(data: Vec<u8>, growth: Option<MutationIdentity>) -> Self {
        Self {
            tracked: TrackedImage::new(data),
            mutation_identities: growth.into_iter().collect(),
        }
    }

    pub(super) fn write_expected(
        &mut self,
        role: impl Into<String>,
        offset: usize,
        expected: &[u8],
        replacement: &[u8],
    ) -> Result<()> {
        self.write_identity(MutationIdentity::exact(role, offset, expected, replacement))
    }

    pub(super) fn write_runtime_routine(
        &mut self,
        role: impl Into<String>,
        offset: usize,
        expected: &[u8],
        replacement: &[u8],
        cpu_address: u16,
    ) -> Result<()> {
        self.write_identity(MutationIdentity::runtime_routine(
            role,
            offset,
            expected,
            replacement,
            cpu_address,
        ))
    }

    pub(super) fn write_runtime_hook(
        &mut self,
        role: impl Into<String>,
        offset: usize,
        expected: &[u8],
        replacement: &[u8],
        hook_role: DialogueRuntimeHookRole,
        site: RuntimeHookSiteIdentity,
    ) -> Result<()> {
        self.write_identity(MutationIdentity::runtime_hook(
            role,
            offset,
            expected,
            replacement,
            hook_role,
            site,
        ))
    }

    fn write_identity(&mut self, identity: MutationIdentity) -> Result<()> {
        ensure!(
            !identity.is_growth(),
            "image growth must be registered before Expected Writes"
        );
        self.tracked.write_expected(
            identity.role.clone(),
            identity.offset,
            &identity.expected,
            &identity.replacement,
        )?;
        self.mutation_identities.push(identity);
        Ok(())
    }

    pub(super) fn writes(&self) -> &[crate::tracked::WriteReport] {
        self.tracked.writes()
    }

    pub(super) fn mutation_identities(&self) -> &[MutationIdentity] {
        &self.mutation_identities
    }

    pub(super) fn verify_all_changes_tracked(&self, source: &[u8]) -> Result<()> {
        self.tracked.verify_all_changes_tracked(source)
    }

    pub(super) fn into_data(self) -> Vec<u8> {
        self.tracked.into_data()
    }
}

mod required_mutations;
mod verification;

pub(super) use required_mutations::{
    mutation_expected_slice, plan_candidate_image_growth, plan_required_mutation_identities,
    runtime_hook_file_offset, runtime_hook_site_identity, runtime_material_routine_file_offset,
    verify_runtime_material_code_projection,
};
#[cfg(test)]
use required_mutations::{
    plan_candidate_image_growth_for_highest_page, plan_required_runtime_material_mutations,
};
pub(super) use verification::{
    TechnicalInstallationCheckInputs, materialize_mutation_plan, unique_mutation_identity_set,
    verify_runtime_state_initializer_installation, verify_technical_installation,
};

#[cfg(test)]
mod tests;
