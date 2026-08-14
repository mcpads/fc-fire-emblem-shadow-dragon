use serde::Serialize;

/// 한 번 검증된 원천 생산자가 어느 화면 소비자에 어떤 결속을 제공하는지 나타낸다.
/// ID는 보고서에 원문 바이트나 번역문을 복제하지 않고도 원천 검증을 재현할 수 있는
/// 안정적인 역할·주소 식별자다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ScreenConsumerSourceBinding {
    pub(crate) screen_role: &'static str,
    pub(crate) population_ids: Vec<String>,
    pub(crate) source_binding_ids: Vec<String>,
}

/// 한 번역 도메인의 원천 population과 화면 소비자 결속을 함께 반환하는 owning-module
/// 검사 결과다. census는 이 결과를 해석하지 않고 기대 화면 메타데이터와 대조한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranslationConsumerSourceEvidence {
    pub(crate) population_ids: Vec<String>,
    pub(crate) screen_bindings: Vec<ScreenConsumerSourceBinding>,
}

pub(crate) fn source_binding_id(prg_bank: usize, cpu_address: u16, role: &str) -> String {
    format!("{prg_bank:02X}:{cpu_address:04X}:{role}")
}

pub(crate) fn qualified_source_binding_id(
    prg_bank: usize,
    cpu_address: u16,
    role: &str,
    qualifier: &str,
) -> String {
    format!(
        "{}[{qualifier}]",
        source_binding_id(prg_bank, cpu_address, role)
    )
}
