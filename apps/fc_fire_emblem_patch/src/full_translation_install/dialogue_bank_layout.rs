//! 확장 PRG의 뱅크 배치에 쓰는 고정값이다.
//!
//! 전에는 전이 미러 뱅크 계획 안에 함께 있었다. 그 계획은 이중 진입 모델과 함께
//! 폐기했지만 이 값들은 미러와 무관하게 다른 도메인의 배치가 계속 쓴다.
//! 의사결정 59번을 따른다.

/// PRG 뱅크 하나의 크기다.
pub(super) const PRG_BANK_SIZE: usize = 16 * 1024;

/// 전투 합성 재료가 이미 차지한 확장 PRG 뱅크다.
pub(super) const BATTLE_MATERIAL_BANK: u8 = 0x10;

/// 원본 고정 뱅크다. 확장 배치는 이 뱅크를 침범하지 않는다.
pub(super) const ACTIVE_FIXED_BANK: u8 = 0x1F;
