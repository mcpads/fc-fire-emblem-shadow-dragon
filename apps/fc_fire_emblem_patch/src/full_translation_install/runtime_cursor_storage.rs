//! 소비자만 쓰는 전송 커서와, 요청·완성 정체성의 자리다.
//!
//! 휘발 상태 `$07F0..=$07F4`는 생산자와 소비자가 공유하는 계약이라 커서를 넣지
//! 않는다. 커서는 NMI 안에서만 살아 있고 NMI 밖에서 읽는 곳이 없다.
//!
//! 자리는 공유 계약 **바로 뒤**에 붙인다. 새 증명을 만들지 않고 `runtime_state_storage`가
//! 이미 세운 증명의 범위를 넓혀 함께 덮는 편이 낫기 때문이다. 그 증명은 18개
//! 처리기의 도달 명령, 원본 NMI, 오디오, 그리고 PRG 전체 직접 피연산자 조사를
//! 거친다. 커서만 따로 원시 바이트 훑기로 정당화하면 약한 증명이 하나 생긴다.
//!
//! 아래로 두지 않은 이유도 있다. 원본 PPU 블록 큐는 `$0781`에서 **위로** 자라고
//! 하드 상한이 `$07DF`다. 공유 계약 아래에 네 바이트를 이어 잡으면 `$07EF`에
//! 걸리는데 그 자리는 원본이 쓴다.
//!
//! 앞의 두 바이트는 요청 단계에 따라 소유자가 바뀐다. `cold_requested` 동안에는
//! NMI 소비자의 항목 커서이고, 전송이 끝나 `ready`를 게시하기 직전에는 그 페이지를
//! 만든 원문 selector/index로 바뀐다. 생산자는 `ready`에서만 게시 정체성을 읽고,
//! 소비자는 `ready`에서 커서를 읽지 않으므로 같은 자리를 안전하게 공유한다.
//!
//! 커서가 목적지 PPU 주소도 원본 주소도 담지 않는 것은 둘 다 유도되기 때문이다.
//! 덮기 단계의 목적지는 항목이 담은 코드에서 `$1000 + code × 16`으로 나오고 원본
//! 주소는 항목이 그대로 담고 있다. 복원 단계는 양쪽 다 «몇 덩어리 남았나»에서 나온다.

use super::runtime_state_storage::CANDIDATE_START;

/// 공유 계약이 쓰는 바이트 수다. 커서는 그 뒤에서 시작한다.
const SHARED_CONTRACT_BYTE_COUNT: u16 = 5;

/// 다음에 읽을 그룹 덩이 항목의 CPU 주소 하위 바이트다. 주소는 `$8000` 창 안이다.
pub(super) const CURSOR_ENTRY_LOW: u16 = CANDIDATE_START + SHARED_CONTRACT_BYTE_COUNT;
/// 그 상위 바이트다.
pub(super) const CURSOR_ENTRY_HIGH: u16 = CURSOR_ENTRY_LOW + 1;
/// `ready`일 때 이 페이지를 만든 원문 디렉터리 선택자다. 요청 중에는 같은 자리를
/// `CURSOR_ENTRY_LOW`가 소유한다.
pub(super) const PUBLISHED_SOURCE_DIRECTORY_SELECTOR: u16 = CURSOR_ENTRY_LOW;
/// `ready`일 때 이 페이지를 만든 원문 엔트리 색인이다. 요청 중에는 같은 자리를
/// `CURSOR_ENTRY_HIGH`가 소유한다.
pub(super) const PUBLISHED_SOURCE_ENTRY_INDEX: u16 = CURSOR_ENTRY_HIGH;
/// 그룹 덩이가 들어 있는 MMC3 페이지다. 소비자가 타일마다 이 페이지를 건다.
pub(super) const CURSOR_GROUP_PAGE: u16 = CURSOR_ENTRY_LOW + 2;
/// 이번 단계에서 아직 처리하지 못한 몫이다. 복원 단계에서는 덩어리 수, 덮기 단계에서는
/// 타일 수다.
pub(super) const CURSOR_REMAINING_TILES: u16 = CURSOR_ENTRY_LOW + 3;
/// 지금 어느 단계인지다. 0이면 원본 페이지 복원, 1이면 한글 타일 덮기다.
pub(super) const CURSOR_PHASE: u16 = CURSOR_ENTRY_LOW + 4;
/// 복원 단계 동안 보관해 두는 덮기 타일 수다. 두 단계가 같은 «남은 몫» 바이트를
/// 쓰기 때문에 따로 담아 둔다.
pub(super) const CURSOR_OVERLAY_TILES: u16 = CURSOR_ENTRY_LOW + 5;
/// 생산자 호출 시점에 살아 있던 원문 디렉터리 선택자다. 연속 대사에서는 현재
/// 레코드가 아니라 다음에 승격할 선행 조회값이다. 전송 중 원본이 또 바꿀 수 있으므로
/// 완료 시점에 살아 있는 값을 다시 읽지 않는다.
pub(super) const REQUEST_SOURCE_DIRECTORY_SELECTOR: u16 = CURSOR_OVERLAY_TILES + 1;
/// 생산자 호출 시점에 살아 있던 원문 엔트리 색인이다.
pub(super) const REQUEST_SOURCE_ENTRY_INDEX: u16 = REQUEST_SOURCE_DIRECTORY_SELECTOR + 1;

#[cfg(test)]
mod tests {
    use super::*;

    /// 커서와 요청 정체성은 공유 계약 뒤에 붙고 예약 범위 안에서 끝나야 한다. 밖으로
    /// 나가면 그 바이트는 아무 증명도 받지 못한 채 쓰이게 된다.
    #[test]
    fn the_cursor_lives_inside_the_proven_reservation() {
        assert_eq!(
            CURSOR_ENTRY_LOW,
            CANDIDATE_START + SHARED_CONTRACT_BYTE_COUNT
        );
        assert!(CURSOR_OVERLAY_TILES < super::super::runtime_state_storage::CANDIDATE_END);
        assert_eq!(
            REQUEST_SOURCE_ENTRY_INDEX,
            super::super::runtime_state_storage::CANDIDATE_END
        );
    }

    /// 네 바이트에 무엇이 들어가는지가 설계 결정이다. 주소는 항목이 담으므로
    /// 커서는 «어디까지 읽었나»와 «어느 페이지인가»만 담는다.
    #[test]
    fn the_cursor_carries_a_read_position_rather_than_addresses() {
        let slots = [
            CURSOR_ENTRY_LOW,
            CURSOR_ENTRY_HIGH,
            CURSOR_GROUP_PAGE,
            CURSOR_REMAINING_TILES,
            CURSOR_PHASE,
            CURSOR_OVERLAY_TILES,
            REQUEST_SOURCE_DIRECTORY_SELECTOR,
            REQUEST_SOURCE_ENTRY_INDEX,
        ];

        assert_eq!(slots.len(), 8);
        for pair in slots.windows(2) {
            assert_eq!(pair[1], pair[0] + 1, "cursor slots must be contiguous");
        }
    }

    /// 요청 중의 커서와 준비 뒤의 원문 정체성은 상태로 구분되는 phase union이다.
    /// 별도 주소를 쓰면 검증된 예약 범위를 불필요하게 넓히게 된다.
    #[test]
    fn published_source_identity_reuses_cursor_slots_only_after_readiness() {
        assert_eq!(PUBLISHED_SOURCE_DIRECTORY_SELECTOR, CURSOR_ENTRY_LOW);
        assert_eq!(PUBLISHED_SOURCE_ENTRY_INDEX, CURSOR_ENTRY_HIGH);
    }

    #[test]
    fn request_source_identity_ends_the_proven_reservation() {
        assert_eq!(
            REQUEST_SOURCE_ENTRY_INDEX,
            super::super::runtime_state_storage::CANDIDATE_END
        );
    }
}
