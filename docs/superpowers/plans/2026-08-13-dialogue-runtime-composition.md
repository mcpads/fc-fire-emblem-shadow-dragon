# 대사 런타임 합성 계층 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 대사 한 페이지가 CHR RAM에서 한글로 화면에 나온다.

**Architecture:** 전송 계층이 이미 «바이트를 vblank 안에서 옮기는 법»을 안다. 이 계획은 «무엇을 옮길지»와 «옮긴 것을 화면이 보게 하는 법»을 채운다. 생산자가 `$77F4`/`$77F1`에서 레코드 색인을 찾고, 그 레코드의 가시 페이지에서 그룹 선택자를 찾고, 그룹의 조밀 조회표로 타일 목록을 세워 요청을 발행한다. 소비자는 그 목록을 걷고, 다 걸으면 selector가 CHR RAM을 고른다.

**Tech Stack:** Rust 2024 (`fc-fire-emblem-patch`), `retro_rp2a03` 타입 어셈블러, Mesen2 + emucap MCP.

## Global Constraints

- **전송 계층의 불변 조건을 물려받는다.** `$C179` 소비자, 조용한 프레임 게이트, 사이클 예산의 빌드 시점 강제. 이 계획이 예산을 다시 유도하더라도 그 세 가지 구조는 바꾸지 않는다.
- **안전 성질:** 글자 타일이 CHR RAM에 올라가기 전에는 그 글자를 절대 출력하지 않는다. 모든 실패는 «원본 동작으로 되돌아감»이거나 «기다림»이어야 한다.
- 실행 코드는 페이지 `2E` 꼬리, CPU `$B131..$C000`의 3,791바이트다. 전송 루틴이 이미 174바이트를 쓴다.
- 고정 뱅크 동굴 `$F400..$F4B0` 176바이트 중 트램폴린·게이트·초기화가 이미 125바이트를 쓴다. **남은 것은 51바이트다.** 새 고정 뱅크 코드는 이 안에 들어가거나 다른 동굴을 찾아야 한다.
- 대사 본문은 절대 git에 커밋하지 않는다.
- 검증은 `cargo test --workspace`와 `cargo clippy --workspace --all-targets`. clippy 기준선은 11개다.

## 이 계획의 범위

| 갈래 | 이 계획 |
|---|---|
| 런타임 식별표 조회 (`$77F4`/`$77F1` → 레코드 색인) | 포함 |
| 페이지 선택자 조회 (레코드 + 가시 페이지 → 그룹) | 포함 |
| 그룹 조밀 조회표 순회 → 타일 목록 전송 | 포함 |
| CHR RAM selector (`$FF40`) | 포함 |
| 콜드 생산자 하나 (`0A:$809B`) 실제 결선 | 포함 |
| `continuous` 차이 타일 경로 | **계획 C** |
| 나머지 생산자 넷 (`$85F8`·`$865F`·`$871C`·`$85C9`) | **계획 C** |
| 출력 시점 동적 remap 투영 | **계획 C** |

끝나면 나오는 것: **첫 대사 한 페이지가 한글로 보이는 ROM.** `E4`/`E6` 전이는 아직 콜드로 다시 올리므로 느리지만 깨지지 않는다.

## 확인해 둔 자료 형식

계획을 쓰기 전에 읽은 것이다. 추측이 아니다.

**런타임 식별표** (`FDID`, 재료 구역 4):

| 자리 | 내용 |
|---|---|
| 0..4 | 매직 `FDID` |
| 4 | 스키마 |
| 5 | 표 개수 |
| 6..8 | 전체 길이 |
| 8..10 | selector 디렉터리 오프셋 (=16) |
| 10..12 | 표 서술자 오프셋 |
| 12..14 | 엔트리 오프셋 |
| 16..272 | selector 디렉터리 256바이트. `$77F4` 값으로 색인하면 표 번호, `FF`면 없음 |
| 272.. | 표 서술자 4바이트씩: selector, 엔트리 수, 엔트리 오프셋(u16) |
| 그 뒤 | 엔트리 2바이트씩 = 레코드 색인 u16 LE, `FFFF`면 없음 |

**페이지 스캔** (재료 구역 2):

| 자리 | 내용 |
|---|---|
| 앞 | 그룹마다 조밀 조회표 320바이트: 하위 256바이트가 코드→atlas 색인, 상위 64바이트가 코드→분류(`FF`면 없음) |
| 가운데 | 페이지 작업집합마다 1바이트 그룹 선택자 (928개) |
| 뒤 | 레코드마다 2바이트 오프셋 + 끝 표시 하나. `directory[record]`부터 `directory[record+1]`까지가 그 레코드의 가시 페이지 선택자들 |

**CHR selector 사슬**: 소유자 `$FF1D`, 대체 지점 `$FF40`이 지금 `JMP $F990`(폐기된 표본 selector)이다. `$07DF` 비트 7이 오버라이드 표시, 하위 5비트가 페이지다.

---

### Task 1: 전송을 타일 목록 순회로 바꾸고 예산을 다시 유도한다

전송 루틴은 지금 **연속** 바이트를 옮긴다. 실제 합성은 흩어진 타일을 옮긴다. 그룹의 조밀 조회표에서 코드 `c`의 atlas 색인을 읽어 `atlas[index]`를 `CHR[c]`로 옮기는 것이라, 원본 주소와 목적 주소가 타일마다 따로 나온다.

몸통이 길어지므로 **프레임당 타일 수가 8보다 작아질 수 있다.** 그 값을 여기서 다시 유도한다. 짐작하지 않는다.

#### 계획을 쓴 뒤 확인한 세 가지

**하나. 존재 판정을 런타임이 하면 안 된다.** 조밀 조회표의 상위 64바이트는 코드 4개당 1바이트에 2비트씩 클래스를 담고 `3`이 «없음»이다. 런타임이 코드 256개를 훑어 가변 시프트로 클래스를 뽑는 것은 타일당 비용이 아니라 프레임당 256회 비용이라 예산에 들어가지 않는다. 그러므로 빌드가 **그룹마다 존재하는 코드 목록**을 미리 세워 재료에 넣는다. 목록은 코드 한 바이트씩이고 atlas 색인은 기존 조회표가 그대로 준다. 35그룹 × (1 + 최대 206) ≈ 7.2 KB다.

**둘. atlas와 목록이 한 창에 같이 못 들어간다.** 실측 크기는 atlas 5,704바이트(713타일, 8 KiB 페이지 하나)이고 스캔 재료는 조회표만 35 × 320 = 11,200바이트다. 여기에 목록 7.2 KB가 붙으면 둘은 서로 다른 MMC3 페이지에 놓인다. 그런데 `$A000` 창은 실행 코드가 쓰고 `$8000` 창은 하나뿐이다.

그래서 **타일마다 `$8000`을 두 번 바꾼다.** 목록·조회표 페이지에서 코드와 atlas 색인을 읽고, atlas 페이지로 바꿔 8바이트를 옮기고, 다시 돌아온다. MMC3 뱅크 쓰기는 왕복 24사이클이라 타일당 비용으로 감당된다. 재료를 재배치해 한 페이지에 몰아넣는 대안은 atlas 5,704 + 조회표 11,200이 이미 8 KiB를 넘어 성립하지 않는다.

**셋. 콜드 요청의 실제 크기는 206타일이다.** `maximum_static_page_group_overlay_tile_count`가 206이고, 가시 페이지 하나가 바꾸는 양(`maximum_visible_page_overlay_tile_count`)은 37, 전이 델타 최대는 66이다. 탐침이 쓰던 210은 페이지 슬롯 수요였지 타일 수가 아니었다. 콜드는 206타일, 계획 C의 `continuous`는 66타일이 상한이다.

이 셋을 반영하면 타일당 비용은 뱅크 왕복 24 + 목록·조회·주소 계산 약 30 + 복사 88 + 상위 평면 34 + 루프 10 ≈ 186사이클이다. 예산 1,243을 나누면 **6타일 근처**가 되고, 206타일 콜드는 35프레임 남짓이다. 이 값은 Step 3이 실제 명령에서 다시 센다.

**Files:**
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/transport.rs`
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_cursor_storage.rs`

**Interfaces:**
- Consumes: `worst_case_cycles`, `budgeted_transport_cycles`, `trampoline::worst_case_reserve_cycles`
- Produces: `TILES_PER_FRAME`(재유도된 값), 커서 의미 변경 — `CURSOR_SOURCE_LOW/HIGH`가 **타일 목록** 포인터가 된다

- [ ] **Step 1: 커서의 뜻을 바꾼다**

`runtime_cursor_storage.rs`의 주석과 이름을 고친다. 네 바이트의 자리는 그대로다.

```rust
/// 남은 타일 목록의 읽기 포인터 하위 바이트다. 목록은 코드 한 바이트씩이다.
pub(super) const CURSOR_LIST_LOW: u16 = CANDIDATE_START + SHARED_CONTRACT_BYTE_COUNT;
/// 그 상위 바이트다.
pub(super) const CURSOR_LIST_HIGH: u16 = CURSOR_LIST_LOW + 1;
/// 그룹 조밀 조회표가 놓인 페이지 안 오프셋의 상위 바이트다. 하위는 코드로 만든다.
pub(super) const CURSOR_GROUP_LOOKUP_HIGH: u16 = CURSOR_LIST_LOW + 2;
/// 아직 올리지 못한 타일 수다.
pub(super) const CURSOR_REMAINING_TILES: u16 = CURSOR_LIST_LOW + 3;
```

- [ ] **Step 2: 타일 몸통을 목록 순회로 다시 쓴다**

`tile_body`를 아래로 바꾼다. `$00`/`$01`이 목록 포인터, `$02`/`$03`은 쓰지 않는다(NMI가 보존하지 않는다). 조회표 접근은 `LDA (zp),Y` 대신 절대 색인을 쓴다. 조회표가 `$8000` 창의 한 페이지 안에 있고 코드가 그 페이지 오프셋의 하위 바이트가 되도록 배치하기 때문이다.

```rust
fn tile_body(loop_start: u16) -> Vec<Instruction> {
    let mut instructions = vec![
        // 다음 타일 코드를 목록에서 읽는다.
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(SCRATCH_POINTER_LOW),
        Instruction::Tax,
        // 목적지 PPU 주소는 코드에서 나온다. `$1000 + code × 16`.
        Instruction::LdaAbsolute(PPU_STATUS),
        Instruction::Txa,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::Clc,
        Instruction::AdcImmediate((CHR_RAM_BASE >> 8) as u8),
        Instruction::StaAbsolute(PPU_ADDRESS),
        Instruction::Txa,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::StaAbsolute(PPU_ADDRESS),
        // 원본 주소는 조회표가 준다. 조회표 하위 바이트가 곧 코드다.
        Instruction::Txa,
        Instruction::StaZeroPage(SCRATCH_LOOKUP_LOW),
        Instruction::LdaAbsolute(CURSOR_GROUP_LOOKUP_HIGH),
        Instruction::StaZeroPage(SCRATCH_LOOKUP_HIGH),
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(SCRATCH_LOOKUP_LOW),
        // atlas 색인 × 8이 atlas 안의 오프셋이다.
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::StaZeroPage(SCRATCH_ATLAS_LOW),
    ];
    for _ in 0..ATLAS_TILE_BYTE_COUNT {
        instructions.extend([
            Instruction::LdaIndirectY(SCRATCH_ATLAS_LOW),
            Instruction::StaAbsolute(PPU_DATA),
            Instruction::Iny,
        ]);
    }
    instructions.push(Instruction::LdaImmediate(0));
    for _ in 0..ATLAS_TILE_BYTE_COUNT {
        instructions.push(Instruction::StaAbsolute(PPU_DATA));
    }
    instructions.extend([
        Instruction::IncZeroPage(SCRATCH_POINTER_LOW),
        Instruction::Dex,
        Instruction::BneAbsolute(loop_start),
    ]);
    instructions
}
```

`SCRATCH_LOOKUP_*`와 `SCRATCH_ATLAS_*`는 NMI가 보존하는 제로 페이지가 아니다. `$00`/`$01`만 안전하므로, 이 과제는 **먼저 안전한 제로 페이지 두 쌍을 더 찾아야 한다.** `runtime_state_storage`의 추적 기계를 제로 페이지에 돌려 후보를 고른다. 후보가 없으면 조회표 접근을 절대 주소 자기수정 없이 `LDA abs,X`로 바꾸고 조회표를 고정 주소에 둔다.

- [ ] **Step 3: 예산을 다시 유도한다**

Task 5(계획 A)의 예산 시험이 그대로 돈다. `TILES_PER_FRAME`을 8에서 내려가며 `one_frame_of_transport_fits_the_measured_vblank_remainder`가 통과하는 최대값을 찾는다.

Run: `cargo test --workspace one_frame_of_transport_fits -- --nocapture`
Expected: 통과할 때까지 값을 내린다. `the_budget_is_the_largest_batch_that_still_fits`가 그 값이 상한임을 확인한다.

- [ ] **Step 4: 커밋**

```bash
git add apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/transport.rs \
        apps/fc_fire_emblem_patch/src/full_translation_install/runtime_cursor_storage.rs
git commit -m "Walk a tile list instead of a contiguous range"
```

---

### Task 2: 런타임 식별표 조회를 방출한다

`$77F4`(디렉터리 선택자)와 `$77F1`(엔트리 색인)에서 레코드 색인을 얻는다. 범위 밖이면 실패로 두고 요청을 발행하지 않는다.

**Files:**
- Create: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/identity_lookup.rs`

**Interfaces:**
- Produces: `pub(super) fn build_identity_lookup(origin: u16, material_base: u16) -> Result<RuntimeRoutine>` — A에 레코드 색인 하위, X에 상위를 남기고 캐리로 성공 여부를 알린다

- [ ] **Step 1: 실패하는 테스트를 쓴다**

```rust
/// 범위 밖 선택자는 요청을 발행하지 않고 원본 경로로 되돌아가야 한다.
/// «잘못 그림»이 아니라 «원본대로»가 실패 모습이다.
#[test]
fn an_unmapped_selector_reports_failure_without_touching_the_request() {
    let routine = build_identity_lookup(0xB200, 0x8000).unwrap();
    let request_store = [0x8D, 0xF4, 0x07];

    assert!(
        !routine.bytes.windows(3).any(|window| window == request_store),
        "the lookup must not publish a request; the producer does that"
    );
}
```

- [ ] **Step 2: 조회를 구현한다**

```rust
// selector 디렉터리는 재료 시작 + 16이다. `$77F4`로 색인한다.
Instruction::LdaAbsolute(0x77F4),
Instruction::Tax,
Instruction::LdaAbsoluteX(material_base + 16),
Instruction::CmpImmediate(0xFF),
// FF면 없는 선택자다. 캐리를 지우고 돌아간다.
```

표 서술자는 4바이트씩이므로 표 번호 × 4를 서술자 오프셋에 더한다. 엔트리 수와 비교해 `$77F1`이 범위 안인지 보고, 엔트리 오프셋 + 색인 × 2에서 레코드 색인 u16을 읽는다. `FFFF`면 실패다.

- [ ] **Step 3: 테스트를 돌려 통과를 확인하고 커밋**

Run: `cargo test --workspace identity_lookup`

```bash
git commit -m "Emit the runtime identity lookup"
```

---

### Task 3: 페이지 선택자 조회와 타일 목록 세우기를 방출한다

레코드 색인과 가시 페이지 색인에서 그룹 선택자를 얻고, 그 그룹의 조밀 조회표에서 «이 페이지가 쓰는 코드 목록»을 세운다.

**Files:**
- Create: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/page_selector.rs`

**Interfaces:**
- Consumes: Task 2의 레코드 색인
- Produces: `pub(super) fn build_page_selector(origin: u16, scan_base: u16, group_stride: u16) -> Result<RuntimeRoutine>`

- [ ] **Step 1: 실패하는 테스트를 쓴다**

```rust
/// 레코드 디렉터리는 시작과 끝 오프셋을 함께 준다. 끝을 읽지 않으면 다음
/// 레코드의 선택자를 이 레코드 것으로 잘못 쓴다.
#[test]
fn the_selector_range_uses_both_directory_entries() {
    let routine = build_page_selector(0xB280, 0x8000, 320).unwrap();
    let directory_reads = routine
        .bytes
        .windows(3)
        .filter(|window| window[0] == 0xBD || window[0] == 0xB9)
        .count();

    assert!(directory_reads >= 2, "the routine reads only one directory entry");
}
```

- [ ] **Step 2: 조회를 구현하고 통과를 확인한다**

레코드 색인 × 2로 디렉터리에서 시작 오프셋을, +2에서 끝 오프셋을 읽는다. 가시 페이지 색인을 더해 선택자 배열에서 그룹 선택자 한 바이트를 읽는다. 페이지 색인이 끝을 넘으면 실패다.

그룹 선택자 × `group_stride`(320)가 조밀 조회표의 오프셋이다. 그 표의 상위 64바이트에서 «분류가 `FF`가 아닌 코드»가 이 그룹이 쓰는 코드다. 목록은 그 코드들을 훑어 만든다.

Run: `cargo test --workspace page_selector`

- [ ] **Step 3: 커밋**

```bash
git commit -m "Emit the visible-page group selector lookup"
```

---

### Task 4: CHR RAM selector를 `$FF40`에 건다

전송이 끝나도 화면이 CHR RAM을 보지 않으면 아무것도 바뀌지 않는다. 탐침에서 `$2007` 쓰기가 버려진 것이 그 증거다.

**Files:**
- Create: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/chr_selector.rs`

**Interfaces:**
- Produces: `pub(super) fn build_chr_selector(origin: u16, fallback: u16) -> Result<RuntimeRoutine>`, `pub(super) const SELECTOR_CHAIN_REPLACEMENT: u16 = 0xFF40;`

- [ ] **Step 1: 실패하는 테스트를 쓴다**

```rust
/// 준비되지 않았을 때 CHR RAM을 고르면 빈 타일이 화면에 나온다.
/// 그것이 안전 성질 위반이므로 `ready`가 아니면 기존 사슬로 넘겨야 한다.
#[test]
fn an_unready_state_falls_through_to_the_existing_chain() {
    let routine = build_chr_selector(0xF490, 0xF990).unwrap();

    assert_eq!(
        &routine.bytes[routine.bytes.len() - 3..],
        [0x4C, 0x90, 0xF9],
        "the selector must end by handing the existing chain control"
    );
}
```

- [ ] **Step 2: selector를 구현한다**

`$07F4`가 `ready(3)`면 CHR 뱅크 레지스터에 0을 써서 CHR RAM을 고르고 돌아간다. 아니면 `JMP $F990`으로 기존 사슬에 넘긴다. 매퍼 165의 CHR RAM은 레지스터 값 0이며 이 사실은 `mapper165::encode_chr_page_register`가 이미 담고 있다.

**동굴 자리를 먼저 확인한다.** `$F400..$F4B0`에 남은 것은 51바이트다. 들어가지 않으면 `$FC56..$FC60`의 10바이트 확장이나 다른 `FF` 구간을 찾아 `runtime_control_flow.rs`에 결속한다.

- [ ] **Step 3: 원본 `$FF40`을 바이트로 결속한다**

지금 값은 `4C 90 F9`다. 바뀌었으면 설치를 거부한다.

```rust
const SELECTOR_CHAIN_CODE: [u8; 3] = [0x4C, 0x90, 0xF9];
```

- [ ] **Step 4: 테스트와 커밋**

Run: `cargo test --workspace chr_selector`

```bash
git commit -m "Select CHR RAM only once the page is ready"
```

---

### Task 5: 콜드 생산자를 실제 조회에 결선한다

`0A:$809B`의 초기화가 지금은 상수를 쓴다. Task 2·3의 조회를 불러 진짜 목록과 개수를 세운다.

**Files:**
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/dispatcher_gate.rs`
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code.rs`

- [ ] **Step 1: 실패하는 테스트를 쓴다**

```rust
/// 조회가 실패하면 요청을 발행하지 않아야 한다. 발행하면 소비자가 쓰레기 목록을
/// 걷고 CHR RAM이 깨진다.
#[test]
fn a_failed_lookup_leaves_the_request_inactive() {
    let routine = build_cold_initializer(0xF480, 0xB200, 0xB280).unwrap();
    let request_store = [0x8D, REQUEST_STATE as u8, (REQUEST_STATE >> 8) as u8];
    let publish_at = routine.bytes.windows(3).position(|w| w == request_store).unwrap();
    let first_branch = routine.bytes.iter().position(|b| *b == 0x90 || *b == 0xB0).unwrap();

    assert!(first_branch < publish_at, "the initializer publishes before it can fail");
}
```

- [ ] **Step 2: 구현하고 통과를 확인한 뒤 커밋**

```bash
git commit -m "Drive the cold request from the real identity lookup"
```

---

### Task 6: 탐침을 다시 만들고 첫 한글 페이지를 확인한다

**Files:**
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install/transport_probe.rs`

- [ ] **Step 1: 탐침이 재료를 함께 싣게 한다**

지금 탐침은 실행 코드만 싣는다. 합성은 atlas·스캔·식별표가 있어야 하므로 재료 용기도 함께 넣는다. 전체 설치가 만드는 것과 같은 재료를 쓴다.

- [ ] **Step 2: 재빌드**

```bash
cargo run -q --release -p fc-fire-emblem-patch -- build-dialogue-transport-probe \
  "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes" \
  out/fire-emblem-fe1-korean-release.nes
```

- [ ] **Step 3: 첫 대사를 본다**

emucap으로 띄우고 `evidence/private/chapter7-maximum-page-reload/next-story.mss`를 올린 뒤 대사를 진행시킨다.

Expected: 대사 창이 잠깐 멈춘 뒤 **한글**로 나온다. 안 나오면 `$07F4`를 읽어 `ready(3)`에 도달했는지, `$07F8`이 줄어드는지로 «전송이 굶었는가»와 «selector가 안 걸렸는가»를 가른다.

- [ ] **Step 4: vblank 불변 조건을 다시 잰다**

Task 1이 몸통을 바꿨으므로 계획 A의 실측을 다시 한다. `$C3A5`에서 scanline이 241..260 안이어야 한다.

- [ ] **Step 5: 의사결정 로그에 결과를 남기고 커밋**

---

## 실행 중 상태

계획을 쓴 뒤 실제로 만들면서 아래가 끝났다.

- [x] 그룹마다 «쓰는 코드와 그 atlas 주소» 덩이를 재료에 넣는다. 항목 3바이트, 총 21,411바이트
- [x] 재료 용기를 다섯 페이지로 늘리고 실행 코드를 마지막 페이지 전체(`$A000`, 8,192바이트)에 고정한다
- [x] 전송을 목록 순회로 다시 쓰고 예산을 다시 유도한다 → **프레임당 4타일**
- [x] CHR selector를 `$FF40`에 걸고, `ready`가 아니면 기존 사슬로 넘긴다
- [x] 설치가 재료까지 실은 확인용 탐침 이미지를 낼 수 있다

추가로 끝난 것.

- [x] 해석기를 방출하고 콜드 생산자에 결선한다. 고정 뱅크 `$F558`의 424바이트를 두 번째 동굴로 쓴다
- [x] CHR RAM을 올리는 동안 걸고 끝에서 되돌린다 (되돌리는 값은 아래 참고)
- [x] 실제 1장 대사에서 런타임이 발화하는 것을 확인했다 — `$07F4 = 01`, 그룹 페이지 `$2E`, 159타일

**남은 것과 그 이유.**

전송이 206항목을 정확히 걷고 `ready`까지 가는 것은 실행으로 확인했다. 그런데 CHR RAM은 0인 채였다. `$2007`에 쓰는 동안 CHR RAM이 두 CHR 창에 걸려 있지 않아 쓰기가 CHR ROM으로 가기 때문이다.

거는 것은 레지스터 네 번 쓰기로 끝난다. 어려운 쪽은 **되돌리기**다. 되돌리지 않으면 반쯤 합성된 CHR RAM이 화면에 나와 안전 성질이 깨지는데, 되돌릴 값을 아는 것은 원본 도우미 `$FA80`·`$FAA0`뿐이고 그 비용이 아직 측정되지 않았다.

이걸 넣었더니 예산 시험이 그냥 통과했고, 그게 진짜 결함이었다. `worst_case_cycles`가 `JSR`를 6사이클로 세고 불려 가는 코드를 아예 세지 않고 있었다. 호출이 들어간 모든 예산이 과소평가돼 있었다는 뜻이다.

사이클 모델은 이제 «비용을 모르는 호출»을 거부한다. 도우미 비용은 경로 전수로 세어 131을 얻었고 그 값이 예산에 들어가 프레임당 타일이 넷에서 셋이 됐다.

**지금 막혀 있는 것은 CHR 뱅크 되돌리기다.** 실행해 보니 1장 맵에서 대사가 시작되는 순간 화면이 깨진다. 원인은 되돌릴 값을 `$5E`·`$5F`에서 읽는 것인데, 맵이 그려지는 중에도 `$5D`·`$5E`·`$5F`가 전부 0이었다. 그 셋은 현재 뱅크의 그림자가 아니라 «바꿔 달라는 요청»이고, 원본 `$C1EC`가 `$5D != 0`일 때만 쓰는 이유가 그것이다.

다음 과제는 «지금 걸려 있는 CHR 뱅크를 아는 방법»을 찾는 것이다. 사슬 `$FF1D`의 호출자들이 페이지를 누산기에 실어 오므로, 그 값을 담아 두는 변수가 있는지 아니면 화면 종류에서 유도되는지부터 조사한다. 그것이 닫히기 전에는 CHR RAM을 빌려 쓸 수 없다.

같은 조사에서 이미 하나를 건졌다. 사슬이 누산기로 페이지를 나른다는 사실은 CHR selector가 그것을 덮어 화면 전체를 깨뜨리는 것으로 드러났고, 지금은 양쪽 경로에서 밀고 되돌린다.

## 자체 검토

**1. 스펙 대응 — 그리고 이 계획이 틀린 곳.** 설계의 «합성 절차»에서 `cold`의 두 단계 중 **원본 글꼴 복원**을 이 계획은 «첫 페이지를 보는 데는 필요 없다»며 계획 C로 미뤘다. 그 판단이 틀렸다.

실행해 보니 맵 타일과 대사 글꼴이 같은 4 KiB 페이지 안에 함께 있다. 래치 두 벌(레지스터 2 = FD, 4 = FE)은 호출부 열세 곳 중 열한 곳이 **같은 값**을 주고, 다른 값을 주는 두 곳은 `$5D != 0`일 때만 도는데 1장 진입부터 대사까지 그 조건이 한 번도 서지 않았다.

그래서 한글만 올린 CHR RAM을 고르면 맵 타일이 통째로 사라진다. 복원은 «두 번째 페이지의 잔재» 문제가 아니라 **첫 페이지가 성립하기 위한 조건**이었다. 계획 C가 아니라 여기서 해야 한다.

**2. 알려진 위험.** Task 1이 이 계획의 문지방이다. 몸통이 길어져 프레임당 타일이 크게 줄면 한 페이지가 수십 프레임이 되어 체감이 나빠진다. 그때는 조회표 접근을 줄이는 배치(코드→atlas 색인을 미리 풀어 목록에 담기)를 검토한다. 다만 **예산을 늘려 vblank를 넘기는 선택은 하지 않는다.**

**3. 제로 페이지.** «안전한 자리를 찾는다»는 문제가 아니었다. 전투 합성이 이미 `$00`..`$07`을 진입에서 밀고 이탈에서 되돌리는 방식(`BORROWED_SCRATCH`)을 쓴다. 밀고 되돌리면 안전은 증명이 아니라 구조로 성립한다. 전송도 같은 방식으로 필요한 만큼만 빌린다. 프레임당 한 쌍이면 약 26사이클이다.

**4. 목록 형식을 코드 한 바이트로 두는 이유.** `(코드, atlas 주소)` 3바이트씩이면 런타임이 조회표를 안 봐도 되지만 35그룹 × 206 × 3 = 21.6 KB라 24 KiB 용기에 조회표와 함께 들어가지 못한다. 타일 데이터를 목록에 직접 넣는 안은 57 KB로 PRG 상한을 넘는다. 코드 한 바이트 + 기존 조회표가 유일하게 성립하는 조합이다.
