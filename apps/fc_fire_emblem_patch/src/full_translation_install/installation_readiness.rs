use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum InstallationReadinessStage {
    TranslationInput,
    RuntimeStateStorage,
    DeclaredConsumerProjection,
    TechnicalInstallation,
    TranslationBaseline,
    ArtifactEmission,
    RuntimeEvidence,
    RuntimeEvidenceInProgress,
    WholeGameRegression,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct InstallationReadinessInputs {
    pub(super) translation_input_complete: bool,
    pub(super) runtime_state_storage_complete: bool,
    pub(super) all_declared_consumers_statically_accounted: bool,
    pub(super) carried_domain_reinspection_complete: bool,
    pub(super) technical_installation_complete: bool,
    pub(super) translation_baseline_accepted: bool,
    pub(super) output_will_be_emitted: bool,
    pub(super) dynamic_verification_started: bool,
    pub(super) declared_consumer_runtime_observation_complete: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct InstallationReadiness {
    stage: InstallationReadinessStage,
}

impl InstallationReadiness {
    pub(super) fn evaluate(inputs: InstallationReadinessInputs) -> Result<Self> {
        ensure!(
            !inputs.declared_consumer_runtime_observation_complete
                || inputs.dynamic_verification_started,
            "declared consumer runtime observation cannot be complete before verification starts"
        );

        let stage = if !inputs.translation_input_complete {
            InstallationReadinessStage::TranslationInput
        } else if !inputs.runtime_state_storage_complete {
            InstallationReadinessStage::RuntimeStateStorage
        } else if !inputs.all_declared_consumers_statically_accounted {
            InstallationReadinessStage::DeclaredConsumerProjection
        } else if !inputs.carried_domain_reinspection_complete
            || !inputs.technical_installation_complete
        {
            InstallationReadinessStage::TechnicalInstallation
        } else if !inputs.translation_baseline_accepted {
            InstallationReadinessStage::TranslationBaseline
        } else if inputs.declared_consumer_runtime_observation_complete {
            InstallationReadinessStage::WholeGameRegression
        } else if inputs.dynamic_verification_started {
            InstallationReadinessStage::RuntimeEvidenceInProgress
        } else if inputs.output_will_be_emitted {
            InstallationReadinessStage::RuntimeEvidence
        } else {
            InstallationReadinessStage::ArtifactEmission
        };

        Ok(Self { stage })
    }

    pub(super) fn next_gate(self) -> &'static str {
        match self.stage {
            InstallationReadinessStage::TranslationInput => {
                "author Korean for every untranslated Japanese line before recalculating glyph lifetimes; do not emit or run a partial ROM"
            }
            InstallationReadinessStage::RuntimeStateStorage => {
                "close the exact volatile runtime-state storage selection against source access, queue, save/load, and battle lifetimes; do not emit or run a partial ROM"
            }
            InstallationReadinessStage::DeclaredConsumerProjection => {
                "finish every remaining declared consumer storage projection against its already-planned font page; do not treat this declared plan as the whole-game census"
            }
            InstallationReadinessStage::TechnicalInstallation => {
                "finish final-artifact installation and carried-domain reinspection before review or artifact-bound runtime evidence"
            }
            InstallationReadinessStage::TranslationBaseline => {
                "accept or revise one complete translation baseline before artifact-bound runtime regression"
            }
            InstallationReadinessStage::ArtifactEmission => {
                "materialize the exact integrated ROM, then bind representative and worst-case declared consumer paths to that artifact before returning to the separate whole-game census"
            }
            InstallationReadinessStage::RuntimeEvidence => {
                "bind representative and worst-case declared consumer paths to the exact emitted artifact before returning to the separate whole-game census"
            }
            InstallationReadinessStage::RuntimeEvidenceInProgress => {
                "continue representative and worst-case declared consumer-path replay on the exact integrated artifact"
            }
            InstallationReadinessStage::WholeGameRegression => {
                "return from the closed declared consumer replay to the separate whole-game consumer census and release regressions for the exact integrated artifact"
            }
        }
    }

    #[cfg(test)]
    fn stage(self) -> InstallationReadinessStage {
        self.stage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_inputs() -> InstallationReadinessInputs {
        InstallationReadinessInputs {
            translation_input_complete: true,
            runtime_state_storage_complete: true,
            all_declared_consumers_statically_accounted: true,
            carried_domain_reinspection_complete: true,
            technical_installation_complete: true,
            translation_baseline_accepted: true,
            output_will_be_emitted: true,
            dynamic_verification_started: false,
            declared_consumer_runtime_observation_complete: false,
        }
    }

    #[test]
    fn unaccepted_translation_baseline_precedes_final_artifact_runtime_evidence() {
        let mut inputs = ready_inputs();
        inputs.translation_baseline_accepted = false;

        let readiness = InstallationReadiness::evaluate(inputs).unwrap();

        assert_eq!(
            readiness.stage(),
            InstallationReadinessStage::TranslationBaseline
        );
        assert!(readiness.next_gate().contains("accept or revise"));
    }

    #[test]
    fn development_runtime_observations_do_not_bypass_baseline_acceptance() {
        let mut inputs = ready_inputs();
        inputs.translation_baseline_accepted = false;
        inputs.dynamic_verification_started = true;
        inputs.declared_consumer_runtime_observation_complete = true;

        let readiness = InstallationReadiness::evaluate(inputs).unwrap();

        assert_eq!(
            readiness.stage(),
            InstallationReadinessStage::TranslationBaseline
        );
        assert!(readiness.next_gate().contains("accept or revise"));
    }

    #[test]
    fn reviewed_emitted_artifact_requests_runtime_evidence() {
        let readiness = InstallationReadiness::evaluate(ready_inputs()).unwrap();

        assert_eq!(
            readiness.stage(),
            InstallationReadinessStage::RuntimeEvidence
        );
    }

    #[test]
    fn reviewed_runtime_replay_reports_progress_then_whole_game_regression() {
        let mut inputs = ready_inputs();
        inputs.dynamic_verification_started = true;
        let in_progress = InstallationReadiness::evaluate(inputs).unwrap();
        assert_eq!(
            in_progress.stage(),
            InstallationReadinessStage::RuntimeEvidenceInProgress
        );

        inputs.declared_consumer_runtime_observation_complete = true;
        let complete = InstallationReadiness::evaluate(inputs).unwrap();
        assert_eq!(
            complete.stage(),
            InstallationReadinessStage::WholeGameRegression
        );
    }

    #[test]
    fn runtime_completion_without_started_evidence_is_rejected() {
        let mut inputs = ready_inputs();
        inputs.declared_consumer_runtime_observation_complete = true;

        let error = InstallationReadiness::evaluate(inputs).unwrap_err();

        assert!(error.to_string().contains("before verification starts"));
    }

    #[test]
    fn incomplete_static_work_precedes_baseline_acceptance_and_runtime() {
        let mut inputs = ready_inputs();
        inputs.translation_baseline_accepted = false;
        inputs.all_declared_consumers_statically_accounted = false;

        let readiness = InstallationReadiness::evaluate(inputs).unwrap();

        assert_eq!(
            readiness.stage(),
            InstallationReadinessStage::DeclaredConsumerProjection
        );
    }
}
