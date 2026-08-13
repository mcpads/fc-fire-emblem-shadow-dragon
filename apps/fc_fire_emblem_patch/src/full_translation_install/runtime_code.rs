//! 대사 런타임이 ROM에 넣는 실행 코드다.
//!
//! 갈래를 나눈 기준은 «무엇이 바뀌면 이 파일이 바뀌는가»다. 전송 루프는 프레임
//! 예산이 바뀌면 바뀌고, 트램폴린은 원본 NMI 계약이 바뀌면 바뀐다.

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::{
    runtime_bank_contract::bind_bank_restore_contract, runtime_nmi_contract::bind_quiet_frame_gate,
};
use crate::{
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
};

pub(in crate::full_translation_install) mod chr_page_shadow;
pub(in crate::full_translation_install) mod chr_selector;
pub(in crate::full_translation_install) mod dispatcher_gate;
pub(in crate::full_translation_install) mod lifecycle;
pub(in crate::full_translation_install) mod resolve_request;
pub(super) mod trampoline;
pub(in crate::full_translation_install) mod transport;

/// 대사 런타임이 원본 제어 흐름에 끼어드는 각 자리의 의미다.
///
/// 개수만 비교하면 selector나 관측 훅을 생산자 훅으로 잘못 셀 수 있다. 설치와 완료
/// 판정은 이 역할을 따라가며, 주소는 별도의 `DialogueRuntimeHookSite`가 맡는다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::full_translation_install) enum DialogueRuntimeHookRole {
    InitialDirectEntryRequest,
    E4TransitionEntryRequest,
    E6TransitionEntryRequest,
    E7CallerResumeRequest,
    CompletedPageAdvanceOrLifetimeEnd,
    E7CallerHandoffInvalidation,
    NmiPageComposer,
    DispatcherGate,
    ChrRamSelector,
    ChrFdPageObservation,
    ChrFePageObservation,
}

/// 훅이 가져가는 원본 자리다.
pub(in crate::full_translation_install) enum DialogueRuntimeHookSite {
    Fixed(u16),
    Switchable { bank: u8, address: u16 },
}

/// 역할, 원본 자리, 쓸 바이트를 함께 들고 다니는 설치 단위다.
pub(in crate::full_translation_install) struct DialogueRuntimeHook {
    pub(in crate::full_translation_install) role: DialogueRuntimeHookRole,
    pub(in crate::full_translation_install) write_role: &'static str,
    pub(in crate::full_translation_install) site: DialogueRuntimeHookSite,
    pub(in crate::full_translation_install) bytes: Vec<u8>,
}

/// 먼저 설치했던 표본 전용 실행 코드를 전역 런타임이 되찾아 쓰는 고정 뱅크 조각이다.
///
/// 일반 고정 동굴은 `FF`라는 선행 조건만 필요하지만, 여기는 이미 실행 코드가 있다.
/// 전체 원천 구간의 digest를 별도로 고정해야 그중 일부만 우연히 같아도 덮지 않는다.
pub(in crate::full_translation_install) struct ReclaimedFixedRuntimeRoutine {
    pub(in crate::full_translation_install) routine: RuntimeRoutine,
    pub(in crate::full_translation_install) source_end_exclusive: u16,
    pub(in crate::full_translation_install) expected_source_sha1: &'static str,
}

/// `$C179` 진입 시점에 남아 있는 vblank다. 앞에 NMI 진입 오버헤드와 OAM DMA밖에
/// 없고 둘 다 고정 비용이라 이 값은 표본이 아니라 상수다. 에뮬레이터 실측으로
/// 확인했고 계산값 `2,273 − 566`과 3사이클 차이다. 의사결정 64번을 따른다.
const MEASURED_VBLANK_REMAINDER: u32 = 1_704;
/// 실기 여유다. 남은 vblank를 전부 쓰지 않는다.
const SAFETY_MARGIN_PERCENT: u32 = 20;
/// `$C179`의 `JSR`가 쓰는 몫이다.
const CONSUMER_HOOK_CALL_CYCLES: u32 = 6;
/// MMC3 뱅크 선택 레지스터다. selector가 CHR RAM을 고를 때 먼저 쓴다.
const CHR_BANK_SELECT_REGISTER: u16 = 0x8000;
/// MMC3 뱅크 값 레지스터다.
const CHR_BANK_VALUE_REGISTER: u16 = 0x8001;
/// 전송 루틴이 한 프레임에 쓸 수 있는 사이클이다.
///
/// `trampoline_reserve`는 훅 호출과 트램폴린이 실제로 쓰는 최악 사이클이고 방출한
/// 명령에서 센 값이다. 임의의 여백을 따로 두지 않는다. 안전 여유는 위의 20% 하나뿐이고,
/// 여백을 두 겹으로 쌓으면 어느 쪽이 실제 근거인지 알 수 없게 된다.
fn budgeted_transport_cycles(trampoline_reserve: u32) -> u32 {
    MEASURED_VBLANK_REMAINDER * (100 - SAFETY_MARGIN_PERCENT) / 100 - trampoline_reserve
}

/// 대사 런타임이 ROM에 넣는 실행 코드와 훅 전체다.
pub(in crate::full_translation_install) struct DialogueRuntimeCodePlan {
    /// 실행 코드 페이지에 놓이는 조각들이다.
    pub(in crate::full_translation_install) code_routines: Vec<RuntimeRoutine>,
    /// 고정 뱅크 동굴에 놓이는 조각들이다.
    pub(in crate::full_translation_install) fixed_routines: Vec<RuntimeRoutine>,
    /// 정확한 기존 실행 코드 전체를 확인한 뒤 되찾아 쓰는 고정 뱅크 조각들이다.
    pub(in crate::full_translation_install) reclaimed_fixed_routines:
        Vec<ReclaimedFixedRuntimeRoutine>,
    /// 원본에 실제로 설치할 훅이다. 역할과 주소와 바이트가 한 단위라 따로 세지 않는다.
    pub(in crate::full_translation_install) hooks: Vec<DialogueRuntimeHook>,
}

impl DialogueRuntimeCodePlan {
    pub(in crate::full_translation_install) fn hook_roles(&self) -> Vec<DialogueRuntimeHookRole> {
        self.hooks.iter().map(|hook| hook.role).collect()
    }
}

/// 실행 코드를 전부 조립한다.
///
/// 고정 뱅크 동굴의 배치는 여기서 한 번에 정한다. 조각마다 시작 주소를 따로 두면
/// 하나가 커졌을 때 다음 조각을 덮는다.
pub(in crate::full_translation_install) fn plan_dialogue_runtime_code(
    source: &Rom,
    candidate: &Rom,
    runtime_code_cpu_start: u16,
    atlas_page: u8,
    code_page: u8,
    layout: resolve_request::MaterialLayout,
) -> Result<DialogueRuntimeCodePlan> {
    let bank_restore = bind_bank_restore_contract(candidate)?;
    bind_quiet_frame_gate(source, candidate)?;
    dispatcher_gate::bind_dispatcher_entry(source, candidate)?;
    lifecycle::bind_lifecycle_sites(source, candidate)?;
    chr_selector::bind_selector_chain_site(candidate)?;
    chr_page_shadow::bind_chr_helper_site(candidate)?;

    let transport = transport::build_transport_routine(runtime_code_cpu_start, atlas_page)?;
    let resolver_origin = transport.address
        + u16::try_from(transport.bytes.len()).context("transport routine length overflow")?;
    let resolver = resolve_request::build_resolve_request(resolver_origin, layout)?;
    let next_page_resolver_origin = resolver.address
        + u16::try_from(resolver.bytes.len()).context("initial resolver length overflow")?;
    let next_page_resolver =
        resolve_request::build_resolve_next_page_request(next_page_resolver_origin, layout)?;
    let next_page_resolver_address = next_page_resolver.address;
    let trampoline_routine = trampoline::build_trampoline(bank_restore, transport.address)?;

    let gate = dispatcher_gate::build_dispatcher_gate(chr_page_shadow::OBSERVER_CAVE_ORIGIN)?;
    let observer_origin = gate.address
        + u16::try_from(gate.bytes.len()).context("dispatcher gate length overflow")?;
    let observer = chr_page_shadow::build_chr_page_observer(observer_origin)?;
    let observer_address = observer.address;
    let gate_address = gate.address;
    let mut fixed_support_bytes = gate.bytes;
    fixed_support_bytes.extend_from_slice(&observer.bytes);
    let fixed_support_capacity =
        usize::from(chr_page_shadow::OBSERVER_CAVE_END - chr_page_shadow::OBSERVER_CAVE_ORIGIN);
    ensure!(
        fixed_support_bytes.len() <= fixed_support_capacity,
        "dialogue dispatcher and observer suite exceeds its reclaimed cave"
    );
    fixed_support_bytes.resize(fixed_support_capacity, 0xFF);
    let fixed_support = RuntimeRoutine {
        role: "dialogue dispatcher gate and CHR page observer suite",
        address: chr_page_shadow::OBSERVER_CAVE_ORIGIN,
        bytes: fixed_support_bytes,
    };

    let initializer_origin = trampoline_routine.address
        + u16::try_from(trampoline_routine.bytes.len())
            .context("dialogue trampoline length overflow")?;
    let initializer =
        dispatcher_gate::build_cold_initializer(initializer_origin, resolver.address, code_page)?;

    // 예산은 시험만이 아니라 빌드가 지킨다. vblank를 넘기는 코드는 ROM에 들어가면
    // 안 되므로, 여기서 막지 않으면 그 판정이 시험을 돌리는 사람에게 넘어간다.
    // 의사결정 62번을 따른다.
    let reserve = trampoline::worst_case_reserve_cycles(bank_restore)?;
    let budget = budgeted_transport_cycles(reserve);
    let frame_cycles = transport::worst_case_frame_cycles(runtime_code_cpu_start, atlas_page)?;
    ensure!(
        frame_cycles <= budget,
        "one transport frame costs {frame_cycles} cycles but only {budget} of the measured \
         {MEASURED_VBLANK_REMAINDER}-cycle vblank remainder are budgeted after the \
         {SAFETY_MARGIN_PERCENT}% margin and the {reserve}-cycle trampoline reserve"
    );

    let selector_origin = initializer.address
        + u16::try_from(initializer.bytes.len()).context("cold initializer length overflow")?;
    let selector = chr_selector::build_chr_selector(
        selector_origin,
        CHR_BANK_SELECT_REGISTER,
        CHR_BANK_VALUE_REGISTER,
    )?;

    let initializer_address = initializer.address;
    let selector_address = selector.address;
    let fixed_routines = vec![trampoline_routine, initializer, selector];
    let code_routines = vec![transport, resolver, next_page_resolver];
    ensure_disjoint(
        &fixed_routines.iter().collect::<Vec<_>>(),
        trampoline::TRAMPOLINE_CAVE_END,
    )?;
    let lifecycle = lifecycle::build_lifecycle_suite(next_page_resolver_address, code_page)?;
    let completed_page_entry = lifecycle.completed_page_entry;
    let handoff_invalidation_entry = lifecycle.handoff_invalidation_entry;
    let reclaimed_fixed_routines = vec![
        ReclaimedFixedRuntimeRoutine {
            routine: fixed_support,
            source_end_exclusive: chr_page_shadow::OBSERVER_CAVE_END,
            expected_source_sha1: chr_page_shadow::EXPECTED_SAMPLE_OBSERVER_CAVE_SHA1,
        },
        ReclaimedFixedRuntimeRoutine {
            routine: lifecycle.routine,
            source_end_exclusive: lifecycle::LIFECYCLE_CAVE_END,
            expected_source_sha1: lifecycle::EXPECTED_SAMPLE_LIFECYCLE_SHA1,
        },
    ];

    let mut hooks = vec![
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::ChrFdPageObservation,
            write_role: "dialogue CHR FD page observer hook",
            site: DialogueRuntimeHookSite::Fixed(chr_page_shadow::CHR_HELPER_SITE),
            bytes: chr_page_shadow::helper_hook_bytes(observer_address).to_vec(),
        },
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::ChrRamSelector,
            write_role: "dialogue CHR RAM selector hook",
            site: DialogueRuntimeHookSite::Fixed(chr_selector::SELECTOR_CHAIN_SITE),
            bytes: chr_selector::selector_hook_bytes(selector_address).to_vec(),
        },
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::NmiPageComposer,
            write_role: "dialogue NMI page composer hook",
            site: DialogueRuntimeHookSite::Fixed(super::runtime_nmi_contract::CONSUMER_HOOK),
            bytes: trampoline::hook_bytes().to_vec(),
        },
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::DispatcherGate,
            write_role: "dialogue dispatcher gate hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: 0x0A,
                address: dispatcher_gate::DISPATCHER_ENTRY,
            },
            bytes: dispatcher_gate::dispatcher_hook_bytes(gate_address).to_vec(),
        },
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::InitialDirectEntryRequest,
            write_role: "dialogue initial direct-entry request hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: 0x0A,
                address: dispatcher_gate::COLD_ENTRY,
            },
            bytes: dispatcher_gate::cold_hook_bytes(initializer_address).to_vec(),
        },
    ];
    for (role, write_role, address) in [
        (
            DialogueRuntimeHookRole::E4TransitionEntryRequest,
            "dialogue E4 transition-entry request hook",
            lifecycle::E4_TRANSITION_SITE,
        ),
        (
            DialogueRuntimeHookRole::E6TransitionEntryRequest,
            "dialogue E6 transition-entry request hook",
            lifecycle::E6_TRANSITION_SITE,
        ),
        (
            DialogueRuntimeHookRole::E7CallerResumeRequest,
            "dialogue E7 caller-resume request hook",
            lifecycle::E7_RESUME_SITE,
        ),
    ] {
        hooks.push(DialogueRuntimeHook {
            role,
            write_role,
            site: DialogueRuntimeHookSite::Switchable {
                bank: 0x0A,
                address,
            },
            bytes: dispatcher_gate::cold_hook_bytes(initializer_address).to_vec(),
        });
    }
    hooks.extend([
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::CompletedPageAdvanceOrLifetimeEnd,
            write_role: "dialogue completed-page lifecycle hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: 0x0A,
                address: lifecycle::COMPLETED_PAGE_SITE,
            },
            bytes: lifecycle::completed_page_hook_bytes(completed_page_entry)?,
        },
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::E7CallerHandoffInvalidation,
            write_role: "dialogue E7 caller-handoff invalidation hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: 0x0A,
                address: lifecycle::E7_HANDOFF_SITE,
            },
            bytes: lifecycle::handoff_invalidation_hook_bytes(handoff_invalidation_entry).to_vec(),
        },
    ]);

    Ok(DialogueRuntimeCodePlan {
        code_routines,
        fixed_routines,
        reclaimed_fixed_routines,
        hooks,
    })
}

/// ROM의 한 자리에 놓이는 실행 코드 조각이다.
#[derive(Debug)]
pub(in crate::full_translation_install) struct RuntimeRoutine {
    pub(in crate::full_translation_install) role: &'static str,
    pub(in crate::full_translation_install) address: u16,
    pub(in crate::full_translation_install) bytes: Vec<u8>,
}

/// 같은 동굴에 놓이는 조각들이 서로 겹치거나 동굴을 넘지 않아야 한다.
/// 겹치면 조용히 잘못된 코드가 실행되고, 넘으면 원본 자료를 덮는다.
pub(super) fn ensure_disjoint(routines: &[&RuntimeRoutine], cave_end: u16) -> Result<()> {
    let mut ordered: Vec<&RuntimeRoutine> = routines.to_vec();
    ordered.sort_by_key(|routine| routine.address);
    for pair in ordered.windows(2) {
        ensure!(
            usize::from(pair[0].address) + pair[0].bytes.len() <= usize::from(pair[1].address),
            "{} ends at {:04X} and overlaps {} at {:04X}",
            pair[0].role,
            usize::from(pair[0].address) + pair[0].bytes.len(),
            pair[1].role,
            pair[1].address
        );
    }
    if let Some(last) = ordered.last() {
        ensure!(
            usize::from(last.address) + last.bytes.len() <= usize::from(cave_end),
            "{} reaches past the reserved cave end {cave_end:04X}",
            last.role
        );
    }
    Ok(())
}

/// 명령 목록을 이어 붙였을 때 다음 명령이 놓일 주소다. 분기 대상을 되메울 때 쓴다.
fn next_address(origin: u16, instructions: &[Instruction]) -> Result<u16> {
    let length = assemble_at(origin, instructions)
        .context("cannot measure a dialogue runtime routine")?
        .len();
    u16::try_from(usize::from(origin) + length)
        .context("dialogue runtime routine crosses the CPU address space")
}

/// 명령 목록이 최악의 경우 쓰는 사이클이다.
///
/// `JSR`는 명령 자체의 6사이클만 세고 불려 가는 코드의 비용은 세지 않는다. 그래서
/// 호출이 섞인 목록을 그냥 더하면 예산이 조용히 과소평가된다. vblank 예산에서
/// 과소평가는 실기 손상이므로, 이 함수는 호출을 만나면 그 자리에서 거부한다.
///
/// 호출이 필요한 코드는 `worst_case_cycles_with_calls`로 불린 곳의 실측 비용을 함께
/// 넘겨야 한다. «얼마인지 모르는 것을 6이라고 세지 않는다»가 규칙이다.
fn worst_case_cycles(instructions: &[Instruction]) -> Result<u32> {
    worst_case_cycles_with_calls(instructions, &[])
}

/// 불려 가는 코드의 최악 사이클을 주소별로 함께 받는다.
fn worst_case_cycles_with_calls(
    instructions: &[Instruction],
    callee_cycles: &[(u16, u32)],
) -> Result<u32> {
    let mut total = 0;
    for instruction in instructions {
        total += u32::from(instruction.worst_case_cycles());
        if let Instruction::JsrAbsolute(target) = instruction {
            let cost = callee_cycles
                .iter()
                .find(|(address, _)| address == target)
                .map(|(_, cost)| *cost)
                .with_context(|| {
                    format!(
                        "a cycle budget counted JSR {target:04X} as six cycles; \
                         the cost of the called code is unknown and must be measured"
                    )
                })?;
            total += cost;
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_routines_are_refused() {
        let first = RuntimeRoutine {
            role: "first",
            address: 0xF400,
            bytes: vec![0; 16],
        };
        let second = RuntimeRoutine {
            role: "second",
            address: 0xF408,
            bytes: vec![0; 4],
        };

        let error = ensure_disjoint(&[&first, &second], 0xF4B0).unwrap_err();

        assert!(error.to_string().contains("overlaps"));
    }

    #[test]
    fn a_routine_past_the_cave_end_is_refused() {
        let only = RuntimeRoutine {
            role: "only",
            address: 0xF4A0,
            bytes: vec![0; 32],
        };

        let error = ensure_disjoint(&[&only], 0xF4B0).unwrap_err();

        assert!(error.to_string().contains("past the reserved cave end"));
    }
}
