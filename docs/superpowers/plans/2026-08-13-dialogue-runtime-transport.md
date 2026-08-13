# 대사 런타임 전송 계층 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 원본 NMI `$C179`에 대사 소비자를 넣어, 조용한 프레임에만 CHR RAM으로 한 프레임 10타일을 올리고 그동안 대사 상태 머신을 붙잡아 두는 전송 계층을 만든다.

**Architecture:** 고정 뱅크 `$F400` 트램폴린이 `JSR $C3A5`를 대체한다. 트램폴린은 원본 대기열 표시 넷을 보고 전부 0일 때만 MMC3 페이지 `2E`를 `$A000`에 걸어 전송 루틴을 부른 뒤 뱅크를 되돌리고 `JMP $C3A5`로 원본에 넘긴다. 전송 루틴은 길이 고정 언롤 복사라 예산을 데이터가 넘길 수 없다. 디스패처 입구 `0A:$8000`은 전송이 끝날 때까지 처리기를 돌리지 않는다.

**Tech Stack:** Rust 2024 (`fc-fire-emblem-patch`), `retro_rp2a03` 타입 어셈블러, Mesen2 + emucap MCP.

## Global Constraints

- 매퍼 165. CHR 상한 256 KiB(4 KiB 페이지 64장), PRG 상한 512 KiB. 현재 PRG는 상한에 정확히 닿아 있으므로 **PRG를 늘리지 않는다.**
- **vblank 밖에서 `$2007`에 쓰지 않는다.** 소비자 진입 `$C179`의 vblank 잔여는 1,704 CPU 사이클(실측), 안전 여유 20%를 뺀 1,363에서 소비자 고정 비용 63을 빼고 바이트당 8사이클로 나눈 뒤 타일 경계로 내려 **프레임당 10타일**이다.
- **안전 성질:** 글자 타일이 CHR RAM에 올라가기 전에는 그 글자를 절대 출력하지 않는다. 모든 실패는 «원본 동작으로 되돌아감»이거나 «기다림»이어야 한다. «잘못 그림»은 0원칙 위반이다.
- 실행 코드는 MMC3 페이지 `2E`의 꼬리, CPU 창 `$A000..$C000`의 끝에 놓인다. 현재 예약은 3,791바이트이고 하한은 `MINIMUM_RUNTIME_CODE_RESERVATION = 1_888`이다.
- 고정 뱅크 트램폴린 동굴은 `$F400..$F4B0` 176바이트, 전 구간 `0xFF`여야 한다.
- 휘발 상태는 `$07F0..=$07F4` 5바이트다. 합성 진행 커서는 여기 넣지 않는다.
- 전투 합성(`$C191 → $FC20`)의 소유권을 건드리지 않는다. 전투는 `$CC == 0x06`(렌더링 끔)일 때만 도는 별개 소유자다.
- 대사 본문은 절대 git에 커밋하지 않는다. `private/`는 `.gitignore`에 있다.
- 검증 명령은 `cargo test --workspace`와 `cargo clippy --workspace --all-targets`다. 둘 다 통과해야 커밋한다.
- 테스트는 요구 동작을 고정한다. 줄 수·현재 항목 수·우연한 출력 순서를 단언하지 않는다(`behavioral-testing` 규약).

## 이 계획의 범위

설계 문서 `docs/superpowers/specs/2026-08-13-dialogue-runtime-design.md`는 다섯 갈래를 담는다. 이 계획은 그중 **전송 계층**만 만든다.

| 갈래 | 이 계획 |
|---|---|
| 휘발 상태와 소비자 지역 커서 | 포함 |
| `$C179` 소비자, 조용한 프레임 게이트, 프레임 예산 | 포함 |
| 디스패처 게이트 `0A:$8000` | 포함 |
| 콜드 초기화 생산자 한 곳(`0A:$809B`) | 포함 — 전송을 끝에서 끝까지 돌리는 데 필요 |
| 합성 절차(atlas 순회, cold/continuous 차이 타일) | **후속 계획 B** |
| 나머지 생산자 네 곳(`$85F8`·`$865F`·`$871C`·완료 페이지) | **후속 계획 C** |
| 출력 시점 글리프 투영(동적 remap) | **후속 계획 B** |

이 계획이 끝나면 나오는 것: 대사에 진입하면 알려진 4 KiB 페이지가 CHR RAM으로 올라갈 때까지 대사가 멈추고, 올라간 뒤 진행하며, 그 어떤 프레임에서도 `$2007` 쓰기가 vblank를 넘지 않는 ROM. 합성 내용이 아직 «원본 글꼴 복원»이라 화면은 원본과 같아 보이지만, **하드웨어 위험은 전부 여기서 닫힌다.**

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/rp2a03.rs` (수정) | 전송 코드가 쓰는 명령어 형식을 타입 ISA로 추가 |
| `src/full_translation_install/runtime_bank_contract.rs` (신규) | `$A000` PRG 뱅크 전환·복원 계약을 원본 바이트에 결속 |
| `src/full_translation_install/runtime_nmi_contract.rs` (신규) | `$C179` 훅 자리와 대기열 표시 넷의 게이트 바이트를 결속 |
| `src/full_translation_install/runtime_cursor_storage.rs` (신규) | 소비자 지역 커서 바이트를 고르고 겹침 없음을 증명 |
| `src/full_translation_install/runtime_code/mod.rs` (신규) | 방출한 루틴 목록과 겹침·용량 검사 |
| `src/full_translation_install/runtime_code/trampoline.rs` (신규) | `$F400` 고정 뱅크 트램폴린 |
| `src/full_translation_install/runtime_code/transport.rs` (신규) | 페이지 `2E` 전송 루틴(언롤 복사, 커서 전진, 완료 표시) |
| `src/full_translation_install/runtime_code/dispatcher_gate.rs` (신규) | `0A:$8000` 보류 게이트와 `0A:$809B` 콜드 초기화 |
| `src/full_translation_install/runtime_control_flow.rs` (수정) | `NMI_HOOK`을 `$C179`로 옮기고 방출 사실을 계획에 반영 |
| `src/full_translation_install/integrated_write_set.rs` (수정) | 방출한 바이트를 누적 이미지에 결속 |

`runtime_code`를 하위 모듈로 쪼개는 이유는 전송 루프와 게이트가 서로 다른 이유로 바뀌기 때문이다. 트램폴린은 원본 NMI 계약이 바뀌면 바뀌고, 전송 루프는 예산이 바뀌면 바뀌고, 디스패처 게이트는 상태 머신 계약이 바뀌면 바뀐다.

---

### Task 1: `$A000` 뱅크 전환·복원 계약을 원본에 결속한다

트램폴린은 페이지 `2E`를 `$A000`에 걸었다가 되돌려야 한다. 되돌릴 값을 어디서 읽는지는 원본이 정해 둔 것이므로 추측하지 않고 바이트로 결속한다. `$C1EC`가 `LDA $5E; JSR $FA80; LDA $5F; JSR $FAA0`을 하는 것은 이미 확인했다. `$FA80`과 `$FAA0`이 각각 어떤 MMC3 레지스터를 쓰는지, 그리고 `$5E`/`$5F`가 그 그림자인지를 이 과제가 닫는다.

**Files:**
- Create: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_bank_contract.rs`
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install.rs` (모듈 선언 추가)

**Interfaces:**
- Consumes: `crate::rom::Rom`, `crate::dialogue_inventory::switchable_cpu_to_file_offset`
- Produces:
  - `pub(super) struct BankRestoreContract { pub(super) select_register_address: u16, pub(super) select_value_address: u16, pub(super) a000_helper: u16, pub(super) a000_shadow: u8, pub(super) prg_a000_register: u8 }`
  - `pub(super) fn bind_bank_restore_contract(source: &Rom) -> Result<BankRestoreContract>`

- [ ] **Step 1: `$FA80`과 `$FAA0`의 실제 바이트를 읽는다**

고정 뱅크는 PRG의 마지막 16 KiB다. 다음을 실행해 두 도우미의 바이트를 확인한다.

```bash
cd "$(git rev-parse --show-toplevel)"
python3 - <<'PY'
data = open("out/fire-emblem-fe1-korean-release.nes","rb").read()
prg = data[16:16+512*1024]
fixed = prg[-16*1024:]            # $C000..$FFFF
def at(addr, n):
    return fixed[addr-0xC000:addr-0xC000+n].hex(" ")
for name, addr in [("FA20",0xFA20),("FA80",0xFA80),("FAA0",0xFAA0)]:
    print(name, at(addr, 16))
PY
```

MMC3식 뱅크 전환은 `LDA #reg; STA $8000; <value>; STA $8001` 꼴이다. 출력에서 `8D 00 80`(STA $8000)과 `8D 01 80`(STA $8001)을 찾고, 그 앞의 `A9 xx`가 레지스터 번호다. PRG `$A000`은 MMC3 레지스터 **7**이다.

- [ ] **Step 2: 실패하는 테스트를 쓴다**

`runtime_bank_contract.rs`를 만들고 아래를 넣는다. `EXPECTED_A000_HELPER_PREFIX`의 값은 Step 1 출력에서 그대로 옮긴다.

```rust
//! `$A000` PRG 뱅크를 걸었다 되돌리는 원본 계약을 바이트로 결속한다.
//!
//! 트램폴린이 페이지 `2E`를 잠깐 `$A000`에 걸기 때문에 되돌릴 값의 출처가
//! 필요하다. 그 출처는 원본이 정한 것이므로 여기서 고르지 않고 확인만 한다.

use anyhow::{Result, ensure};

use crate::rom::Rom;

/// MMC3식 뱅크 선택 레지스터다.
pub(super) const BANK_SELECT_REGISTER: u16 = 0x8000;
/// 선택한 레지스터에 넣을 값을 쓰는 자리다.
pub(super) const BANK_VALUE_REGISTER: u16 = 0x8001;
/// PRG `$A000` 창을 고르는 MMC3 레지스터 번호다.
pub(super) const PRG_A000_REGISTER: u8 = 7;
/// 원본이 `$A000` 뱅크를 바꿀 때 부르는 도우미다.
pub(super) const A000_BANK_HELPER: u16 = 0xFAA0;
/// `$C1EC`가 그 도우미에 넘기는 제로 페이지 그림자다.
pub(super) const A000_BANK_SHADOW: u8 = 0x5F;

/// `$C1EC`의 뱅크 복원 순서다. `LDA $5D; BEQ; LDA $5E; JSR $FA80; LDA $5F; JSR $FAA0`.
const NMI_BANK_RESTORE: [u8; 13] = [
    0xA5, 0x5D, 0xF0, 0x0A, 0xA5, 0x5E, 0x20, 0x80, 0xFA, 0xA5, 0x5F, 0x20, 0xA0, 
];

#[derive(Debug, Clone, Copy)]
pub(super) struct BankRestoreContract {
    pub(super) select_register_address: u16,
    pub(super) select_value_address: u16,
    pub(super) a000_helper: u16,
    pub(super) a000_shadow: u8,
    pub(super) prg_a000_register: u8,
}

pub(super) fn bind_bank_restore_contract(source: &Rom) -> Result<BankRestoreContract> {
    ensure!(
        fixed_bytes(source, 0xC1EC, NMI_BANK_RESTORE.len())? == NMI_BANK_RESTORE,
        "NMI bank restore sequence at $C1EC changed"
    );
    ensure!(
        helper_writes_register(source, A000_BANK_HELPER, PRG_A000_REGISTER)?,
        "the $A000 bank helper no longer selects MMC3 register {PRG_A000_REGISTER}"
    );
    Ok(BankRestoreContract {
        select_register_address: BANK_SELECT_REGISTER,
        select_value_address: BANK_VALUE_REGISTER,
        a000_helper: A000_BANK_HELPER,
        a000_shadow: A000_BANK_SHADOW,
        prg_a000_register: PRG_A000_REGISTER,
    })
}

/// 도우미가 `LDA #reg; STA $8000`으로 시작하는지 본다.
fn helper_writes_register(source: &Rom, helper: u16, register: u8) -> Result<bool> {
    let bytes = fixed_bytes(source, helper, 5)?;
    Ok(bytes[0] == 0xA9 && bytes[1] == register && bytes[2..5] == [0x8D, 0x00, 0x80])
}

fn fixed_bytes(rom: &Rom, address: u16, length: usize) -> Result<&[u8]> {
    let prg = rom.prg();
    let base = prg.len() - 16 * 1024;
    let offset = base + usize::from(address) - 0xC000;
    prg.get(offset..offset + length)
        .ok_or_else(|| anyhow::anyhow!("fixed-bank read at {address:04X} is out of range"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 되돌릴 뱅크 값의 출처가 원본에 있어야 트램폴린이 성립한다.
    #[test]
    fn the_nmi_restores_the_a000_bank_from_a_zero_page_shadow() {
        let rom = crate::test_support::release_rom();

        let contract = bind_bank_restore_contract(&rom).unwrap();

        assert_eq!(contract.a000_shadow, 0x5F);
        assert_eq!(contract.prg_a000_register, 7);
    }
}
```

- [ ] **Step 3: 테스트를 돌려 실패를 확인한다**

Run: `cargo test --workspace runtime_bank_contract`
Expected: `crate::test_support::release_rom` 미해결로 컴파일 실패. 다음 단계에서 만든다.

- [ ] **Step 4: 테스트 지원 도우미를 만든다**

`apps/fc_fire_emblem_patch/src/test_support.rs`를 새로 만들고 `main.rs`에 `#[cfg(test)] mod test_support;`를 추가한다.

```rust
//! 테스트가 쓰는 ROM 적재 도우미다. 산출물이 없으면 명확한 안내로 실패한다.

use std::path::PathBuf;

use crate::rom::Rom;

/// 누적 빌드가 만든 배포 이미지를 읽는다.
pub(crate) fn release_rom() -> Rom {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../out/fire-emblem-fe1-korean-release.nes");
    let data = std::fs::read(&path).unwrap_or_else(|error| {
        panic!("release image {} is missing ({error}); run the cumulative build and BuildReleaseImage first", path.display())
    });
    Rom::parse(data).expect("release image parses")
}
```

- [ ] **Step 5: 테스트를 돌려 통과를 확인하고, `NMI_BANK_RESTORE`의 마지막 바이트를 실제 값으로 채운다**

Run: `cargo test --workspace runtime_bank_contract -- --nocapture`
Expected: `NMI bank restore sequence at $C1EC changed`로 실패하면 위 상수 배열의 13번째 바이트(`0xA0` 다음의 `0xFA`)를 Step 1 출력에 맞춰 고친 뒤 다시 돌린다. 최종 기대: PASS.

`helper_writes_register`가 거짓이면 `$FAA0`이 `$A000`이 아니라 다른 창을 고르는 것이므로, Step 1 출력에서 `A9 07`을 쓰는 도우미를 찾아 `A000_BANK_HELPER`를 그 주소로 바꾼다.

- [ ] **Step 6: 모듈을 선언하고 전체 검사를 돌린다**

`full_translation_install.rs`의 모듈 선언 블록에 `mod runtime_bank_contract;`를 알파벳 순서에 맞게 넣는다.

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS, 경고 없음

- [ ] **Step 7: 커밋**

```bash
git add apps/fc_fire_emblem_patch/src/full_translation_install/runtime_bank_contract.rs \
        apps/fc_fire_emblem_patch/src/test_support.rs \
        apps/fc_fire_emblem_patch/src/main.rs \
        apps/fc_fire_emblem_patch/src/full_translation_install.rs
git commit -m "Bind the A000 bank restore contract to source bytes"
```

---

### Task 2: 전송 코드가 쓰는 명령어 형식을 타입 ISA에 추가한다

`rp2a03.rs`의 `Instruction`은 지금까지 쓴 형식만 담고 있다. 전송 루프와 트램폴린이 쓰는 `DEY`, `LDY zp`, `STX zp`, `BPL`, `BMI`가 없다. 인코딩을 바이트로 고정하는 테스트와 함께 넣는다.

**Files:**
- Modify: `apps/fc_fire_emblem_patch/src/rp2a03.rs`

**Interfaces:**
- Produces: `Instruction::Dey`, `Instruction::LdyZeroPage(u8)`, `Instruction::StxZeroPage(u8)`, `Instruction::BplAbsolute(u16)`, `Instruction::BmiAbsolute(u16)`

- [ ] **Step 1: 실패하는 테스트를 쓴다**

`rp2a03.rs`의 `mod tests` 안에 넣는다.

```rust
/// 전송 계층이 쓰는 형식이 정확한 오피코드로 내려가야 ROM이 성립한다.
#[test]
fn encodes_dialogue_transport_addressing_forms() {
    let bytes = assemble_at(
        0xF400,
        &[
            Instruction::Dey,
            Instruction::LdyZeroPage(0x5F),
            Instruction::StxZeroPage(0x10),
            Instruction::BplAbsolute(0xF400),
            Instruction::BmiAbsolute(0xF400),
        ],
    )
    .unwrap();

    assert_eq!(bytes, [0x88, 0xA4, 0x5F, 0x86, 0x10, 0x10, 0xF9, 0x30, 0xF7]);
}
```

- [ ] **Step 2: 테스트를 돌려 실패를 확인한다**

Run: `cargo test --workspace encodes_dialogue_transport_addressing_forms`
Expected: FAIL — `no variant named Dey found for enum Instruction`

- [ ] **Step 3: 열거자와 세 곳의 대응을 추가한다**

`enum Instruction`에 다음을 추가한다.

```rust
    Dey,
    LdyZeroPage(u8),
    StxZeroPage(u8),
    BplAbsolute(u16),
    BmiAbsolute(u16),
```

`encoded_len`의 1바이트 갈래에 `| Self::Dey`를 넣고, 2바이트 갈래에 `| Self::LdyZeroPage(_) | Self::StxZeroPage(_) | Self::BplAbsolute(_) | Self::BmiAbsolute(_)`를 넣는다.

`lower`에 다음을 추가한다.

```rust
            Self::Dey => implied(Mnemonic::Dey, AddressingMode::Implied),
            Self::LdyZeroPage(address) => zero_page(Mnemonic::Ldy, address),
            Self::StxZeroPage(address) => zero_page(Mnemonic::Stx, address),
            Self::BplAbsolute(target) => relative(Mnemonic::Bpl, "BPL", pc, target)?,
            Self::BmiAbsolute(target) => relative(Mnemonic::Bmi, "BMI", pc, target)?,
```

- [ ] **Step 4: 테스트를 돌려 통과를 확인한다**

Run: `cargo test --workspace encodes_dialogue_transport_addressing_forms`
Expected: PASS

`Mnemonic::Dey` 등이 `retro_rp2a03`에 없다면 그 크레이트가 이름을 다르게 쓰는 것이다. `cargo doc -p retro_rp2a03 --open` 대신 `grep -rn "Dey\|Bpl\|Bmi\|Stx" $(cargo metadata --format-version 1 | python3 -c "import json,sys;print([p['manifest_path'] for p in json.load(sys.stdin)['packages'] if p['name']=='retro-rp2a03'][0])" | xargs dirname)/src | head`로 실제 이름을 확인해 맞춘다.

- [ ] **Step 5: 기존 분기 범위 테스트를 새 분기까지 넓힌다**

`rejects_a_relative_branch_target_outside_signed_byte_range`의 배열에 `Instruction::BplAbsolute(0x8100)`과 `Instruction::BmiAbsolute(0x8100)`을 추가한다.

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS, 경고 없음

- [ ] **Step 6: 커밋**

```bash
git add apps/fc_fire_emblem_patch/src/rp2a03.rs
git commit -m "Add the transport addressing forms to the typed ISA"
```

---

### Task 3: `$C179` 훅 자리와 조용한 프레임 게이트를 원본에 결속한다

훅을 설치하려면 그 자리의 원본 바이트가 정확히 무엇인지, 그리고 게이트가 보는 표시 넷이 정말 각 갈래를 막고 있는지가 ROM에 고정돼 있어야 한다. 원본이 다르면 설치를 거부해야 한다.

**Files:**
- Create: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_nmi_contract.rs`
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install.rs`

**Interfaces:**
- Consumes: `crate::rom::Rom`, `crate::typed_source::decode_rp2a03_sequence`
- Produces:
  - `pub(super) const CONSUMER_HOOK: u16 = 0xC179;`
  - `pub(super) const DISPLACED_CALL: u16 = 0xC3A5;`
  - `pub(super) const QUEUE_FLAGS: [u8; 4] = [0x21, 0x22, 0x89, 0x8A];`
  - `pub(super) fn bind_quiet_frame_gate(source: &Rom, candidate: &Rom) -> Result<QuietFrameGateContract>`
  - `pub(super) struct QuietFrameGateContract { pub(super) gated_branch_count: usize }`

- [ ] **Step 1: 실패하는 테스트를 쓴다**

```rust
//! 조용한 프레임 게이트가 보는 표시 넷과, 훅이 밀어내는 호출을 원본에 결속한다.
//!
//! 게이트의 전제는 «표시가 전부 0이면 원본은 이 프레임 vblank에 PPU 자료를 쓰지
//! 않는다»이다. 그 전제는 아래 세 갈래의 첫 분기가 지키고 있으므로, 분기가 바뀌면
//! 게이트가 무효가 된다. 의사결정 64번을 따른다.

use anyhow::{Result, ensure};

use crate::{rom::Rom, typed_source::decode_rp2a03_sequence};

/// 소비자가 들어갈 자리다. 원본은 여기서 `JSR $C3A5`를 한다.
pub(super) const CONSUMER_HOOK: u16 = 0xC179;
/// 소비자가 밀어내고 자신이 다시 불러 줘야 하는 호출이다.
pub(super) const DISPLACED_CALL: u16 = 0xC3A5;
/// 원본의 vblank PPU 자료 작업을 여는 대기열 표시들이다.
pub(super) const QUEUE_FLAGS: [u8; 4] = [0x21, 0x22, 0x89, 0x8A];

/// `$C179`: `JSR $C3A5`.
const HOOK_SITE: [u8; 3] = [0x20, 0xA5, 0xC3];
/// `$C3A5`: `LDA $21; BEQ $C3BE`.
const BLOCK_INTERPRETER_GATE: [u8; 4] = [0xA5, 0x21, 0xF0, 0x15];
/// `$C296`: `LDY $22; BEQ $C295; BMI $C2CC`.
const PALETTE_QUEUE_GATE: [u8; 6] = [0xA4, 0x22, 0xF0, 0xFB, 0x30, 0x30];
/// `$D4AD`: `LDA $89; BNE $D4B6; LDA $8A; BNE $D4CE; RTS`.
const ROW_UPLOAD_GATE: [u8; 9] = [0xA5, 0x89, 0xD0, 0x05, 0xA5, 0x8A, 0xD0, 0x19, 0x60];
/// `$C733`: `LDA $CD; STA $2000; LDA $CC; STA $2001; RTS`. 소비자가 쓰는 증가 비트의
/// 그림자가 `$CD`라는 근거다.
const CONTROL_RESTORE: [u8; 11] = [
    0xA5, 0xCD, 0x8D, 0x00, 0x20, 0xA5, 0xCC, 0x8D, 0x01, 0x20, 0x60,
];

#[derive(Debug, Clone, Copy)]
pub(super) struct QuietFrameGateContract {
    pub(super) gated_branch_count: usize,
}

pub(super) fn bind_quiet_frame_gate(
    source: &Rom,
    candidate: &Rom,
) -> Result<QuietFrameGateContract> {
    for rom in [source, candidate] {
        ensure!(
            fixed_bytes(rom, CONSUMER_HOOK, HOOK_SITE.len())? == HOOK_SITE,
            "the dialogue consumer hook site at $C179 no longer calls $C3A5"
        );
    }
    let gates: [(&str, u16, &[u8]); 4] = [
        ("block interpreter", 0xC3A5, &BLOCK_INTERPRETER_GATE),
        ("palette queue", 0xC296, &PALETTE_QUEUE_GATE),
        ("row upload", 0xD4AD, &ROW_UPLOAD_GATE),
        ("control restore", 0xC733, &CONTROL_RESTORE),
    ];
    for (role, address, expected) in gates {
        ensure!(
            fixed_bytes(candidate, address, expected.len())? == expected,
            "the {role} gate at {address:04X} changed; the quiet-frame precondition is void"
        );
        decode_rp2a03_sequence(expected, address, role)?;
    }
    Ok(QuietFrameGateContract {
        gated_branch_count: gates.len() - 1,
    })
}

fn fixed_bytes(rom: &Rom, address: u16, length: usize) -> Result<&[u8]> {
    let prg = rom.prg();
    let base = prg.len() - 16 * 1024;
    let offset = base + usize::from(address) - 0xC000;
    prg.get(offset..offset + length)
        .ok_or_else(|| anyhow::anyhow!("fixed-bank read at {address:04X} is out of range"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 게이트가 보는 표시 넷은 각각 원본의 PPU 자료 갈래 하나를 열고 닫는다.
    /// 그 대응이 깨지면 «조용한 프레임»의 뜻이 달라진다.
    #[test]
    fn every_queue_flag_still_guards_a_ppu_data_branch() {
        let rom = crate::test_support::release_rom();

        let contract = bind_quiet_frame_gate(&rom, &rom).unwrap();

        assert_eq!(contract.gated_branch_count, 3);
        assert!(QUEUE_FLAGS.contains(&BLOCK_INTERPRETER_GATE[1]));
        assert!(QUEUE_FLAGS.contains(&PALETTE_QUEUE_GATE[1]));
        assert!(QUEUE_FLAGS.contains(&ROW_UPLOAD_GATE[1]));
        assert!(QUEUE_FLAGS.contains(&ROW_UPLOAD_GATE[5]));
    }

    /// 훅 자리가 다른 호출로 바뀌면 소비자가 밀어낼 대상이 사라지므로 설치를 막는다.
    #[test]
    fn a_changed_hook_site_refuses_installation() {
        let rom = crate::test_support::release_rom();
        let mut bytes = rom.data().to_vec();
        let prg_base = 16;
        let fixed_base = prg_base + rom.prg().len() - 16 * 1024;
        bytes[fixed_base + usize::from(CONSUMER_HOOK) - 0xC000] = 0xEA;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_quiet_frame_gate(&mutated, &mutated).unwrap_err();

        assert!(error.to_string().contains("no longer calls $C3A5"));
    }
}
```

- [ ] **Step 2: 테스트를 돌려 실패를 확인한다**

Run: `cargo test --workspace runtime_nmi_contract`
Expected: FAIL — `unresolved module runtime_nmi_contract`

- [ ] **Step 3: 모듈을 선언한다**

`full_translation_install.rs`의 모듈 선언 블록에 `mod runtime_nmi_contract;`를 넣는다.

- [ ] **Step 4: 테스트를 돌려 통과를 확인한다**

Run: `cargo test --workspace runtime_nmi_contract -- --nocapture`
Expected: PASS

`decode_rp2a03_sequence`가 `$C296`의 `BEQ $C295`(뒤로 가는 분기)를 거부하면, 그 분기는 자기 앞의 `RTS`로 돌아가는 정상 코드이므로 `PALETTE_QUEUE_GATE`를 앞 4바이트(`0xA4, 0x22, 0xF0, 0xFB`)로 줄이고 디코드 대상에서 뺀다. 바이트 결속은 그대로 유지한다.

- [ ] **Step 5: clippy와 전체 테스트**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS, 경고 없음

- [ ] **Step 6: 커밋**

```bash
git add apps/fc_fire_emblem_patch/src/full_translation_install/runtime_nmi_contract.rs \
        apps/fc_fire_emblem_patch/src/full_translation_install.rs
git commit -m "Bind the quiet-frame gate to the source branches it depends on"
```

---

### Task 4: 소비자 지역 커서 저장소를 고르고 겹침 없음을 증명한다

전송은 여러 프레임에 걸치므로 «어디까지 올렸는지»를 기억해야 한다. 설계는 이 커서를 휘발 상태 5바이트에 넣지 않기로 했다. 소비자만 읽고 쓰기 때문이다. 자리는 `$07EB..=$07EF` 5바이트를 쓴다. 이미 선정된 `$07F0..=$07F4` 바로 앞이고 같은 근거(원본 접근 추적)로 닫힌다.

**Files:**
- Create: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_cursor_storage.rs`
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install.rs`

**Interfaces:**
- Consumes: `crate::rom::Rom`
- Produces:
  - `pub(super) const CURSOR_SOURCE_LOW: u16 = 0x07EB;`
  - `pub(super) const CURSOR_SOURCE_HIGH: u16 = 0x07EC;`
  - `pub(super) const CURSOR_DESTINATION_LOW: u16 = 0x07ED;`
  - `pub(super) const CURSOR_DESTINATION_HIGH: u16 = 0x07EE;`
  - `pub(super) const CURSOR_REMAINING_TILES: u16 = 0x07EF;`
  - `pub(super) fn bind_cursor_storage(source: &Rom) -> Result<CursorStorageContract>`
  - `pub(super) struct CursorStorageContract { pub(super) byte_count: usize, pub(super) direct_write_site_count: usize }`

- [ ] **Step 1: 실패하는 테스트를 쓴다**

```rust
//! 소비자만 쓰는 전송 커서의 자리다.
//!
//! 휘발 상태 다섯 바이트는 생산자와 소비자의 계약이라 커서를 넣지 않는다.
//! 커서는 NMI 안에서만 살아 있고 NMI 밖에서 읽는 곳이 없다.

use anyhow::{Result, ensure};

use crate::rom::Rom;

pub(super) const CURSOR_SOURCE_LOW: u16 = 0x07EB;
pub(super) const CURSOR_SOURCE_HIGH: u16 = 0x07EC;
pub(super) const CURSOR_DESTINATION_LOW: u16 = 0x07ED;
pub(super) const CURSOR_DESTINATION_HIGH: u16 = 0x07EE;
pub(super) const CURSOR_REMAINING_TILES: u16 = 0x07EF;

const CURSOR_RANGE: std::ops::RangeInclusive<u16> = CURSOR_SOURCE_LOW..=CURSOR_REMAINING_TILES;

#[derive(Debug, Clone, Copy)]
pub(super) struct CursorStorageContract {
    pub(super) byte_count: usize,
    pub(super) direct_write_site_count: usize,
}

/// 원본 PRG 전체에서 이 범위를 직접 피연산자로 쓰는 명령이 있는지 센다.
/// 절대 주소 형식 `STA/LDA/STX/STY/LDX/LDY abs`의 하위 두 바이트를 훑는다.
pub(super) fn bind_cursor_storage(source: &Rom) -> Result<CursorStorageContract> {
    let mut direct_write_site_count = 0;
    let prg = source.prg();
    for window in prg.windows(3) {
        let operand = u16::from_le_bytes([window[1], window[2]]);
        if !CURSOR_RANGE.contains(&operand) {
            continue;
        }
        // 절대 형식 오피코드만 센다. 우연히 자료가 같은 바이트를 갖는 경우는
        // 아래 테스트가 «0건»을 요구하므로 보수적으로 함께 센다.
        if matches!(
            window[0],
            0x8D | 0x8E | 0x8C | 0xAD | 0xAE | 0xAC | 0x9D | 0x99 | 0xBD | 0xB9 | 0xBC | 0xBE
        ) {
            direct_write_site_count += 1;
        }
    }
    ensure!(
        direct_write_site_count == 0,
        "the dialogue transport cursor range is touched by {direct_write_site_count} source sites"
    );
    Ok(CursorStorageContract {
        byte_count: usize::from(CURSOR_REMAINING_TILES - CURSOR_SOURCE_LOW + 1),
        direct_write_site_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 커서를 원본이 건드리면 여러 프레임에 걸친 전송이 조용히 깨진다.
    #[test]
    fn no_source_instruction_addresses_the_transport_cursor() {
        let rom = crate::test_support::release_rom();

        let contract = bind_cursor_storage(&rom).unwrap();

        assert_eq!(contract.byte_count, 5);
        assert_eq!(contract.direct_write_site_count, 0);
    }

    /// 커서와 휘발 상태는 겹치면 안 된다. 소유자가 다르기 때문이다.
    #[test]
    fn the_cursor_range_stays_below_the_shared_volatile_state() {
        assert!(CURSOR_REMAINING_TILES < 0x07F0);
    }
}
```

- [ ] **Step 2: 테스트를 돌려 실패를 확인한다**

Run: `cargo test --workspace runtime_cursor_storage`
Expected: FAIL — `unresolved module runtime_cursor_storage`

- [ ] **Step 3: 모듈을 선언한다**

`full_translation_install.rs`에 `mod runtime_cursor_storage;`를 넣는다.

- [ ] **Step 4: 테스트를 돌려 통과를 확인한다**

Run: `cargo test --workspace runtime_cursor_storage -- --nocapture`
Expected: PASS

`direct_write_site_count`가 0이 아니면 그 범위를 원본이 쓰는 것이므로, `$07EB` 대신 `$07E6..=$07EA`로 내려 다시 돌린다. `$0780..$07FF`는 원본 블록 큐 버퍼이므로 **`$07E0` 아래로는 내려가지 않는다.** 두 번째 후보도 0이 아니면 이 과제를 멈추고 큐 버퍼의 실제 상한을 먼저 조사한다.

- [ ] **Step 5: clippy와 전체 테스트, 커밋**

```bash
cargo test --workspace && cargo clippy --workspace --all-targets
git add apps/fc_fire_emblem_patch/src/full_translation_install/runtime_cursor_storage.rs \
        apps/fc_fire_emblem_patch/src/full_translation_install.rs
git commit -m "Reserve the transport cursor outside the shared volatile state"
```

---

### Task 5: 페이지 `2E` 전송 루틴을 방출한다

한 프레임에 10타일을 CHR RAM으로 올리고 커서를 전진시킨 뒤, 남은 타일이 0이 되면 휘발 상태를 `ready`로 바꾼다. 복사는 길이 고정 언롤이라 데이터가 반복 횟수를 늘릴 수 없다.

atlas는 타일당 8바이트 1bpp다. CHR에는 16바이트 2bpp로 펼치고 상위 bitplane은 0으로 채운다. 그래서 타일 하나는 «원본 8바이트 전송 + 0 8회»다.

**Files:**
- Create: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/transport.rs`
- Create: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/mod.rs`
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install.rs`

**Interfaces:**
- Consumes: `crate::rp2a03::{Instruction, assemble_at}`, Task 4의 커서 상수
- Produces:
  - `pub(super) const TILES_PER_FRAME: u8 = 10;`
  - `pub(super) const REQUEST_STATE: u16 = 0x07F4;`
  - `pub(super) const STATE_READY: u8 = 3;`
  - `pub(super) struct RuntimeRoutine { pub(super) role: &'static str, pub(super) address: u16, pub(super) bytes: Vec<u8> }`
  - `pub(super) fn build_transport_routine(origin: u16) -> Result<RuntimeRoutine>`

- [ ] **Step 1: 실패하는 테스트를 쓴다**

`transport.rs`를 만들고 아래를 넣는다.

```rust
//! 페이지 `2E`에 놓이는 전송 루틴이다.
//!
//! 한 프레임에 정확히 열 타일을 올린다. 이 수는 `$C179`의 vblank 잔여 1,704사이클에서
//! 안전 여유 20%와 소비자 고정 비용을 뺀 값에서 유도했다. 의사결정 64번을 따른다.

use anyhow::{Context, Result, ensure};

use super::super::runtime_cursor_storage::{
    CURSOR_DESTINATION_HIGH, CURSOR_DESTINATION_LOW, CURSOR_REMAINING_TILES, CURSOR_SOURCE_HIGH,
    CURSOR_SOURCE_LOW,
};
use crate::rp2a03::{Instruction, assemble_at};

/// 한 프레임에 올리는 타일 수다. 사이클 예산에서 유도한 값이라 늘리려면 예산을
/// 다시 유도해야 한다.
pub(super) const TILES_PER_FRAME: u8 = 10;
/// 타일 하나가 CHR에서 차지하는 바이트다. 2bpp 8×8.
pub(super) const CHR_TILE_BYTE_COUNT: u8 = 16;
/// atlas가 타일 하나에 쓰는 바이트다. 1bpp 8×8.
pub(super) const ATLAS_TILE_BYTE_COUNT: u8 = 8;

pub(super) const REQUEST_STATE: u16 = 0x07F4;
pub(super) const STATE_READY: u8 = 3;

const PPU_DATA: u16 = 0x2007;
/// 소비자가 쓰는 제로 페이지 임시값이다. NMI 프롤로그가 이미 스택에 밀어 둔다.
const SCRATCH_POINTER: u8 = 0x00;

pub(super) struct RuntimeRoutine {
    pub(super) role: &'static str,
    pub(super) address: u16,
    pub(super) bytes: Vec<u8>,
}

pub(super) fn build_transport_routine(origin: u16) -> Result<RuntimeRoutine> {
    let mut instructions = Vec::new();

    // 남은 타일이 0이면 할 일이 없다.
    instructions.push(Instruction::LdaAbsolute(CURSOR_REMAINING_TILES));
    let finished_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));

    // 이번 프레임에 올릴 타일 수를 정한다. 남은 것이 예산보다 적으면 남은 만큼.
    instructions.extend([
        Instruction::CmpImmediate(TILES_PER_FRAME),
        Instruction::BccAbsolute(origin),
    ]);
    let use_remaining_placeholder = instructions.len() - 1;
    instructions.push(Instruction::LdaImmediate(TILES_PER_FRAME));
    let batch_selected = next_address(origin, &instructions)?;
    instructions[use_remaining_placeholder] = Instruction::BccAbsolute(batch_selected);
    instructions.push(Instruction::StaZeroPage(SCRATCH_POINTER));

    // atlas 포인터를 제로 페이지에 세운다.
    instructions.extend([
        Instruction::LdaAbsolute(CURSOR_SOURCE_LOW),
        Instruction::StaZeroPage(SCRATCH_POINTER + 1),
        Instruction::LdaAbsolute(CURSOR_SOURCE_HIGH),
        Instruction::StaZeroPage(SCRATCH_POINTER + 2),
    ]);

    // PPU 주소를 목적지 타일로 맞춘다. `$2002` 읽기로 래치를 초기화한다.
    instructions.extend([
        Instruction::LdaAbsolute(0x2002),
        Instruction::LdaAbsolute(CURSOR_DESTINATION_HIGH),
        Instruction::StaAbsolute(0x2006),
        Instruction::LdaAbsolute(CURSOR_DESTINATION_LOW),
        Instruction::StaAbsolute(0x2006),
    ]);

    // 타일 루프. 몸통은 언롤이라 반복 횟수만 자료가 정하고 그 상한이 예산이다.
    let tile_loop = next_address(origin, &instructions)?;
    instructions.push(Instruction::LdyImmediate(0));
    for _ in 0..ATLAS_TILE_BYTE_COUNT {
        instructions.extend([
            Instruction::LdaIndirectY(SCRATCH_POINTER + 1),
            Instruction::StaAbsolute(PPU_DATA),
            Instruction::Iny,
        ]);
    }
    instructions.push(Instruction::LdaImmediate(0));
    for _ in 0..ATLAS_TILE_BYTE_COUNT {
        instructions.push(Instruction::StaAbsolute(PPU_DATA));
    }
    // atlas 포인터를 한 타일 전진.
    instructions.extend([
        Instruction::Clc,
        Instruction::LdaZeroPage(SCRATCH_POINTER + 1),
        Instruction::AdcImmediate(ATLAS_TILE_BYTE_COUNT),
        Instruction::StaZeroPage(SCRATCH_POINTER + 1),
        Instruction::LdaZeroPage(SCRATCH_POINTER + 2),
        Instruction::AdcImmediate(0),
        Instruction::StaZeroPage(SCRATCH_POINTER + 2),
        Instruction::DecAbsolute(CURSOR_REMAINING_TILES),
        Instruction::DecAbsolute(u16::from(SCRATCH_POINTER)),
        Instruction::LdaZeroPage(SCRATCH_POINTER),
        Instruction::BneAbsolute(tile_loop),
    ]);

    // 커서를 저장한다. 목적지는 올린 타일 수 × 16바이트만큼 나아갔다.
    instructions.extend([
        Instruction::LdaZeroPage(SCRATCH_POINTER + 1),
        Instruction::StaAbsolute(CURSOR_SOURCE_LOW),
        Instruction::LdaZeroPage(SCRATCH_POINTER + 2),
        Instruction::StaAbsolute(CURSOR_SOURCE_HIGH),
    ]);

    // 다 올렸으면 준비 완료를 알린다.
    instructions.push(Instruction::LdaAbsolute(CURSOR_REMAINING_TILES));
    let still_pending_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    instructions.extend([
        Instruction::LdaImmediate(STATE_READY),
        Instruction::StaAbsolute(REQUEST_STATE),
    ]);

    let done = next_address(origin, &instructions)?;
    instructions[finished_placeholder] = Instruction::BeqAbsolute(done);
    instructions[still_pending_placeholder] = Instruction::BneAbsolute(done);
    instructions.push(Instruction::Rts);

    let bytes = assemble_at(origin, &instructions)?;
    ensure!(
        !bytes.is_empty(),
        "the dialogue transport routine assembled to nothing"
    );
    Ok(RuntimeRoutine {
        role: "dialogue transport",
        address: origin,
        bytes,
    })
}

fn next_address(origin: u16, instructions: &[Instruction]) -> Result<u16> {
    let length = assemble_at(origin, instructions)
        .context("cannot measure the dialogue transport routine")?
        .len();
    u16::try_from(usize::from(origin) + length)
        .context("dialogue transport routine crosses the CPU address space")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 프레임당 전송량은 사이클 예산에서 나온 값이다. 복사 몸통이 언롤이라
    /// 자료가 반복 횟수를 늘릴 수 없다는 것이 예산이 성립하는 근거다.
    #[test]
    fn one_frame_uploads_exactly_the_budgeted_tile_count() {
        let routine = build_transport_routine(0xB000).unwrap();

        let ppu_data_writes = routine
            .bytes
            .windows(3)
            .filter(|window| window[0] == 0x8D && window[1] == 0x07 && window[2] == 0x20)
            .count();

        // 언롤 몸통은 타일 하나분이고, 루프가 그것을 최대 TILES_PER_FRAME번 돈다.
        assert_eq!(ppu_data_writes, usize::from(CHR_TILE_BYTE_COUNT));
    }

    /// 한 타일이 CHR에서 차지하는 만큼 정확히 쓰지 않으면 다음 타일이 밀린다.
    #[test]
    fn a_tile_expands_from_one_bitplane_to_two() {
        assert_eq!(CHR_TILE_BYTE_COUNT, ATLAS_TILE_BYTE_COUNT * 2);
    }

    /// 다 올리기 전에 준비 완료를 알리면 안전 성질이 깨진다.
    #[test]
    fn readiness_is_published_only_after_the_last_tile() {
        let routine = build_transport_routine(0xB000).unwrap();
        let ready_store = [0x8D, REQUEST_STATE as u8, (REQUEST_STATE >> 8) as u8];
        let ready_at = routine
            .bytes
            .windows(3)
            .position(|window| window == ready_store)
            .expect("the routine publishes readiness");
        let last_ppu_write = routine
            .bytes
            .windows(3)
            .rposition(|window| window == [0x8D, 0x07, 0x20])
            .expect("the routine writes PPU data");

        assert!(ready_at > last_ppu_write);
    }
}
```

- [ ] **Step 2: 모듈 껍데기를 만든다**

`runtime_code/mod.rs`:

```rust
//! 대사 런타임이 ROM에 넣는 실행 코드다.

pub(super) mod transport;
```

`full_translation_install.rs`에 `mod runtime_code;`를 넣는다.

- [ ] **Step 3: 테스트를 돌려 통과를 확인한다**

Run: `cargo test --workspace transport -- --nocapture`
Expected: PASS

`Instruction::DecAbsolute(u16::from(SCRATCH_POINTER))`가 `DEC $0000`으로 내려가 3바이트를 쓴다. 제로 페이지 형식이 필요하면 Task 2와 같은 방법으로 `DecZeroPage`를 추가하고 바꾼다. 기능은 같으므로 이 과제를 막지 않는다.

- [ ] **Step 4: 사이클 예산을 테스트로 고정한다**

`transport.rs`의 `mod tests`에 추가한다.

```rust
/// 복사 몸통의 사이클 비용이 예산 안에 있어야 vblank를 넘지 않는다.
/// 타일 하나: 하위 평면 8 × (LDA (zp),Y 5 + STA abs 4 + INY 2) = 88,
/// 상위 평면 LDA #0 2 + 8 × STA abs 4 = 34, 포인터 전진과 루프 27.
#[test]
fn the_budgeted_batch_fits_the_measured_vblank_remainder() {
    const PER_TILE_CYCLES: u32 = 88 + 34 + 27;
    const CONSUMER_FIXED_CYCLES: u32 = 63;
    /// `$C179` 진입 시점의 vblank 잔여를 실측한 값이다.
    const MEASURED_VBLANK_REMAINDER: u32 = 1_704;
    const SAFETY_MARGIN_PERCENT: u32 = 20;

    let allowed =
        MEASURED_VBLANK_REMAINDER * (100 - SAFETY_MARGIN_PERCENT) / 100 - CONSUMER_FIXED_CYCLES;
    let worst_case = PER_TILE_CYCLES * u32::from(TILES_PER_FRAME);

    assert!(
        worst_case <= allowed,
        "one frame costs {worst_case} cycles but only {allowed} are budgeted"
    );
}
```

Run: `cargo test --workspace the_budgeted_batch_fits -- --nocapture`
Expected: PASS — 1,490 ≤ 1,300이 아니라 **실패**한다면 `TILES_PER_FRAME`을 8로 낮춘다(8 × 149 = 1,192 ≤ 1,300). 낮췄다면 스펙의 «프레임당 10타일»을 8로 고치고 그 사유를 함께 적는다. 실측 사이클이 초안의 8사이클/바이트 가정보다 크다는 사실이 여기서 드러나는 것이 이 테스트의 목적이다.

- [ ] **Step 5: clippy와 전체 테스트, 커밋**

```bash
cargo test --workspace && cargo clippy --workspace --all-targets
git add apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/ \
        apps/fc_fire_emblem_patch/src/full_translation_install.rs
git commit -m "Emit the budgeted CHR RAM transport routine"
```

---

### Task 6: 고정 뱅크 트램폴린을 방출한다

`$C179`의 `JSR $C3A5`를 `JSR $F400`으로 바꾸고, `$F400`이 게이트를 본 뒤 페이지 `2E`를 걸어 전송 루틴을 부르고 뱅크를 되돌린 다음 `JMP $C3A5`로 원본에 넘긴다.

**Files:**
- Create: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/trampoline.rs`
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/mod.rs`

**Interfaces:**
- Consumes: Task 1의 `BankRestoreContract`, Task 3의 `CONSUMER_HOOK`·`DISPLACED_CALL`·`QUEUE_FLAGS`, Task 5의 `REQUEST_STATE`
- Produces:
  - `pub(super) const TRAMPOLINE_ORIGIN: u16 = 0xF400;`
  - `pub(super) const TRAMPOLINE_CAVE_END: u16 = 0xF4B0;`
  - `pub(super) const RUNTIME_CODE_MMC3_PAGE: u8 = 0x2E;`
  - `pub(super) fn build_trampoline(contract: BankRestoreContract, transport_entry: u16) -> Result<RuntimeRoutine>`
  - `pub(super) fn hook_bytes() -> [u8; 3]`

- [ ] **Step 1: 실패하는 테스트를 쓴다**

```rust
//! `$C179`에 들어가는 고정 뱅크 트램폴린이다.
//!
//! 원본의 `JSR $C3A5`를 밀어내고 대신 불린다. 조용한 프레임이 아니면 아무것도
//! 하지 않고 곧바로 원본 호출로 넘어간다. 의사결정 64번을 따른다.

use anyhow::{Context, Result, ensure};

use super::super::{
    runtime_bank_contract::BankRestoreContract,
    runtime_nmi_contract::{DISPLACED_CALL, QUEUE_FLAGS},
};
use super::transport::{REQUEST_STATE, RuntimeRoutine};
use crate::rp2a03::{Instruction, assemble_at};

pub(super) const TRAMPOLINE_ORIGIN: u16 = 0xF400;
pub(super) const TRAMPOLINE_CAVE_END: u16 = 0xF4B0;
pub(super) const RUNTIME_CODE_MMC3_PAGE: u8 = 0x2E;
/// `inactive`. 이 값이면 요청이 없다.
const STATE_INACTIVE: u8 = 0;
/// `ready`. 이미 다 올렸으면 더 할 일이 없다.
const STATE_READY: u8 = 3;

pub(super) fn hook_bytes() -> [u8; 3] {
    [
        0x20,
        TRAMPOLINE_ORIGIN as u8,
        (TRAMPOLINE_ORIGIN >> 8) as u8,
    ]
}

pub(super) fn build_trampoline(
    contract: BankRestoreContract,
    transport_entry: u16,
) -> Result<RuntimeRoutine> {
    let origin = TRAMPOLINE_ORIGIN;
    let mut instructions = vec![
        // 원본이 이 프레임에 PPU 자료를 쓸 예정이면 비켜난다.
        Instruction::LdaZeroPage(QUEUE_FLAGS[0]),
        Instruction::OraZeroPage(QUEUE_FLAGS[1]),
        Instruction::OraZeroPage(QUEUE_FLAGS[2]),
        Instruction::OraZeroPage(QUEUE_FLAGS[3]),
    ];
    let busy_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));

    // 요청이 없거나 이미 준비됐으면 할 일이 없다.
    instructions.push(Instruction::LdaAbsolute(REQUEST_STATE));
    let inactive_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.push(Instruction::CmpImmediate(STATE_READY));
    let ready_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));

    // 순차 증가를 강제한다. `$D4E7`이 쓰는 방식과 같다.
    instructions.extend([
        Instruction::LdaZeroPage(0xCD),
        Instruction::AndImmediate(0xFB),
        Instruction::StaZeroPage(0xCD),
        Instruction::StaAbsolute(0x2000),
    ]);

    // 실행 코드 페이지를 `$A000`에 건다.
    instructions.extend([
        Instruction::LdaImmediate(contract.prg_a000_register),
        Instruction::StaAbsolute(contract.select_register_address),
        Instruction::LdaImmediate(RUNTIME_CODE_MMC3_PAGE),
        Instruction::StaAbsolute(contract.select_value_address),
        Instruction::JsrAbsolute(transport_entry),
    ]);

    // 원본이 기대하는 뱅크로 되돌린다.
    instructions.extend([
        Instruction::LdaZeroPage(contract.a000_shadow),
        Instruction::JsrAbsolute(contract.a000_helper),
    ]);

    let done = next_address(origin, &instructions)?;
    instructions[busy_placeholder] = Instruction::BneAbsolute(done);
    instructions[inactive_placeholder] = Instruction::BeqAbsolute(done);
    instructions[ready_placeholder] = Instruction::BeqAbsolute(done);
    // 밀어낸 원본 호출로 넘긴다. `$C3A5`의 RTS가 `$C17C`로 돌아간다.
    instructions.push(Instruction::JmpAbsolute(DISPLACED_CALL));

    let bytes = assemble_at(origin, &instructions)?;
    ensure!(
        origin as usize + bytes.len() <= TRAMPOLINE_CAVE_END as usize,
        "the dialogue trampoline is {} bytes and overruns the {}-byte fixed cave",
        bytes.len(),
        TRAMPOLINE_CAVE_END - TRAMPOLINE_ORIGIN
    );
    let _ = STATE_INACTIVE;
    Ok(RuntimeRoutine {
        role: "dialogue trampoline",
        address: origin,
        bytes,
    })
}

fn next_address(origin: u16, instructions: &[Instruction]) -> Result<u16> {
    let length = assemble_at(origin, instructions)
        .context("cannot measure the dialogue trampoline")?
        .len();
    u16::try_from(usize::from(origin) + length)
        .context("dialogue trampoline crosses the CPU address space")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> BankRestoreContract {
        BankRestoreContract {
            select_register_address: 0x8000,
            select_value_address: 0x8001,
            a000_helper: 0xFAA0,
            a000_shadow: 0x5F,
            prg_a000_register: 7,
        }
    }

    /// 원본에 일이 있는 프레임에는 PPU를 건드리지 않고 넘어가야 한다.
    /// 그 경로에 `$2007` 쓰기가 있으면 게이트가 무의미하다.
    #[test]
    fn the_skip_path_touches_no_ppu_data_register() {
        let routine = build_trampoline(contract(), 0xB000).unwrap();
        let gate_branch = routine
            .bytes
            .iter()
            .position(|byte| *byte == 0xD0)
            .expect("the trampoline branches on the queue flags");
        let skip_target = gate_branch + 2 + usize::from(routine.bytes[gate_branch + 1]);

        assert!(
            !routine.bytes[skip_target..]
                .windows(3)
                .any(|window| window == [0x8D, 0x07, 0x20]),
            "the skip path writes PPU data"
        );
    }

    /// 트램폴린은 반드시 밀어낸 원본 호출로 끝나야 한다. 그러지 않으면
    /// 원본의 블록 큐가 영원히 비워지지 않는다.
    #[test]
    fn every_path_reaches_the_displaced_source_call() {
        let routine = build_trampoline(contract(), 0xB000).unwrap();
        let tail = &routine.bytes[routine.bytes.len() - 3..];

        assert_eq!(
            tail,
            [0x4C, DISPLACED_CALL as u8, (DISPLACED_CALL >> 8) as u8]
        );
    }

    /// 고정 뱅크 동굴을 넘으면 원본 자료를 덮는다.
    #[test]
    fn the_trampoline_fits_the_reserved_fixed_cave() {
        let routine = build_trampoline(contract(), 0xB000).unwrap();

        assert!(
            routine.address as usize + routine.bytes.len() <= TRAMPOLINE_CAVE_END as usize
        );
    }
}
```

- [ ] **Step 2: 모듈을 선언한다**

`runtime_code/mod.rs`에 `pub(super) mod trampoline;`을 추가한다.

- [ ] **Step 3: 테스트를 돌려 통과를 확인한다**

Run: `cargo test --workspace trampoline -- --nocapture`
Expected: PASS

- [ ] **Step 4: clippy와 전체 테스트, 커밋**

```bash
cargo test --workspace && cargo clippy --workspace --all-targets
git add apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/
git commit -m "Emit the fixed-bank trampoline for the dialogue consumer"
```

---

### Task 7: 디스패처 보류 게이트와 콜드 초기화를 방출한다

전송이 끝나기 전에는 대사 처리기를 돌리지 않는다. 이것이 안전 성질을 구조적으로 만드는 자리다. 그리고 대사 진입 `0A:$809B`에서 휘발 상태 다섯 바이트와 커서를 콜드 초기화한다.

`0A:$8000`의 원본은 `LDA $77F7` 뒤 `JSR $C34C`이고, `0A:$809B`의 원본은 `JSR $E6B2`다. 둘 다 `runtime_control_flow.rs`가 이미 바이트로 결속하고 있다.

**Files:**
- Create: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/dispatcher_gate.rs`
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/mod.rs`

**Interfaces:**
- Consumes: Task 5의 `REQUEST_STATE`·`STATE_READY`, Task 4의 커서 상수
- Produces:
  - `pub(super) const DISPATCHER_ENTRY: u16 = 0x8000;`
  - `pub(super) const COLD_ENTRY: u16 = 0x809B;`
  - `pub(super) fn build_dispatcher_gate(origin: u16, dispatcher_body: u16) -> Result<RuntimeRoutine>`
  - `pub(super) fn build_cold_initializer(origin: u16, source_resolver: u16, atlas_base: u16, chr_destination: u16, tile_count: u8) -> Result<RuntimeRoutine>`

- [ ] **Step 1: 실패하는 테스트를 쓴다**

```rust
//! 디스패처 보류 게이트와 콜드 초기화다.
//!
//! 게이트가 처리기를 붙잡고 있는 동안 원본은 큐에 아무것도 넣지 않는다. 그래서
//! 그 프레임들이 조용해지고, 전송이 굶지 않는다. 이것이 설계의 안전 성질이
//! 구조적으로 성립하는 이유다.

use anyhow::{Context, Result, ensure};

use super::super::runtime_cursor_storage::{
    CURSOR_DESTINATION_HIGH, CURSOR_DESTINATION_LOW, CURSOR_REMAINING_TILES, CURSOR_SOURCE_HIGH,
    CURSOR_SOURCE_LOW,
};
use super::transport::{REQUEST_STATE, RuntimeRoutine, STATE_READY};
use crate::rp2a03::{Instruction, assemble_at};

pub(super) const DISPATCHER_ENTRY: u16 = 0x8000;
pub(super) const COLD_ENTRY: u16 = 0x809B;
/// `cold_requested`.
pub(super) const STATE_COLD_REQUESTED: u8 = 1;

/// 요청이 걸려 있으면 처리기를 돌리지 않고 그대로 돌아간다.
pub(super) fn build_dispatcher_gate(origin: u16, dispatcher_body: u16) -> Result<RuntimeRoutine> {
    let mut instructions = vec![Instruction::LdaAbsolute(REQUEST_STATE)];
    let inactive_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.push(Instruction::CmpImmediate(STATE_READY));
    let ready_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.push(Instruction::Rts);

    let run_body = next_address(origin, &instructions)?;
    instructions[inactive_placeholder] = Instruction::BeqAbsolute(run_body);
    instructions[ready_placeholder] = Instruction::BeqAbsolute(run_body);
    instructions.push(Instruction::JmpAbsolute(dispatcher_body));

    Ok(RuntimeRoutine {
        role: "dialogue dispatcher gate",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 다섯 바이트와 커서를 전부 쓴 뒤에 요청을 발행한다. 순서가 요구사항이다.
pub(super) fn build_cold_initializer(
    origin: u16,
    source_resolver: u16,
    atlas_base: u16,
    chr_destination: u16,
    tile_count: u8,
) -> Result<RuntimeRoutine> {
    ensure!(tile_count > 0, "a cold request with no tiles never completes");
    let instructions = vec![
        Instruction::LdaImmediate(atlas_base as u8),
        Instruction::StaAbsolute(CURSOR_SOURCE_LOW),
        Instruction::LdaImmediate((atlas_base >> 8) as u8),
        Instruction::StaAbsolute(CURSOR_SOURCE_HIGH),
        Instruction::LdaImmediate(chr_destination as u8),
        Instruction::StaAbsolute(CURSOR_DESTINATION_LOW),
        Instruction::LdaImmediate((chr_destination >> 8) as u8),
        Instruction::StaAbsolute(CURSOR_DESTINATION_HIGH),
        Instruction::LdaImmediate(tile_count),
        Instruction::StaAbsolute(CURSOR_REMAINING_TILES),
        // 요청은 마지막에 발행한다. 그 전에 소비자가 깨어나면 커서가 반쯤 세워진다.
        Instruction::LdaImmediate(STATE_COLD_REQUESTED),
        Instruction::StaAbsolute(REQUEST_STATE),
        Instruction::JmpAbsolute(source_resolver),
    ];
    Ok(RuntimeRoutine {
        role: "dialogue cold initializer",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

fn next_address(origin: u16, instructions: &[Instruction]) -> Result<u16> {
    let length = assemble_at(origin, instructions)
        .context("cannot measure the dispatcher gate")?
        .len();
    u16::try_from(usize::from(origin) + length)
        .context("dispatcher gate crosses the CPU address space")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 요청이 걸린 동안 처리기가 돌면 아직 없는 타일이 화면에 나온다.
    /// 그것이 0원칙 위반이므로 게이트는 반드시 되돌아가야 한다.
    #[test]
    fn a_pending_request_returns_without_running_the_handler() {
        let routine = build_dispatcher_gate(0xF480, 0x8003).unwrap();
        let jump_to_body = [0x4C, 0x03, 0x80];
        let body_jump_at = routine
            .bytes
            .windows(3)
            .position(|window| window == jump_to_body)
            .expect("the gate can reach the handler");
        let first_rts = routine
            .bytes
            .iter()
            .position(|byte| *byte == 0x60)
            .expect("the gate has an early return");

        assert!(
            first_rts < body_jump_at,
            "the early return must come before the handler jump"
        );
    }

    /// 요청 발행이 커서 설정보다 앞서면 소비자가 반쯤 세워진 커서를 읽는다.
    #[test]
    fn the_request_is_published_after_every_cursor_byte() {
        let routine = build_cold_initializer(0xF4A0, 0xE6B2, 0xA100, 0x1000, 40).unwrap();
        let request_store = [0x8D, REQUEST_STATE as u8, (REQUEST_STATE >> 8) as u8];
        let request_at = routine
            .bytes
            .windows(3)
            .position(|window| window == request_store)
            .expect("the initializer publishes a request");

        for cursor in [
            CURSOR_SOURCE_LOW,
            CURSOR_SOURCE_HIGH,
            CURSOR_DESTINATION_LOW,
            CURSOR_DESTINATION_HIGH,
            CURSOR_REMAINING_TILES,
        ] {
            let store = [0x8D, cursor as u8, (cursor >> 8) as u8];
            let at = routine
                .bytes
                .windows(3)
                .position(|window| window == store)
                .unwrap_or_else(|| panic!("cursor {cursor:04X} is never written"));
            assert!(at < request_at, "cursor {cursor:04X} is written too late");
        }
    }

    /// 타일이 0인 요청은 영원히 끝나지 않아 대사가 멈춘다.
    #[test]
    fn a_zero_tile_request_is_refused() {
        let error = build_cold_initializer(0xF4A0, 0xE6B2, 0xA100, 0x1000, 0).unwrap_err();

        assert!(error.to_string().contains("never completes"));
    }
}
```

- [ ] **Step 2: 모듈을 선언하고 테스트를 돌린다**

`runtime_code/mod.rs`에 `pub(super) mod dispatcher_gate;`를 추가한다.

Run: `cargo test --workspace dispatcher_gate -- --nocapture`
Expected: PASS

- [ ] **Step 3: 트램폴린과 게이트가 같은 동굴에서 겹치지 않는지 검사한다**

`runtime_code/mod.rs`에 추가한다.

```rust
use anyhow::{Result, ensure};

use transport::RuntimeRoutine;

/// 고정 뱅크 동굴에 놓이는 루틴들이 서로 겹치지 않아야 한다.
pub(super) fn ensure_disjoint(routines: &[RuntimeRoutine], cave_end: u16) -> Result<()> {
    let mut ordered: Vec<&RuntimeRoutine> = routines.iter().collect();
    ordered.sort_by_key(|routine| routine.address);
    for pair in ordered.windows(2) {
        ensure!(
            pair[0].address as usize + pair[0].bytes.len() <= pair[1].address as usize,
            "{} ends at {:04X} and overlaps {} at {:04X}",
            pair[0].role,
            pair[0].address as usize + pair[0].bytes.len(),
            pair[1].role,
            pair[1].address
        );
    }
    if let Some(last) = ordered.last() {
        ensure!(
            last.address as usize + last.bytes.len() <= cave_end as usize,
            "{} reaches past the reserved cave end {cave_end:04X}",
            last.role
        );
    }
    Ok(())
}
```

`mod.rs`의 `mod tests`에 추가한다.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// 동굴이 좁아 루틴이 서로를 덮으면 조용히 잘못된 코드가 실행된다.
    #[test]
    fn overlapping_routines_are_refused() {
        let routines = vec![
            RuntimeRoutine {
                role: "first",
                address: 0xF400,
                bytes: vec![0; 16],
            },
            RuntimeRoutine {
                role: "second",
                address: 0xF408,
                bytes: vec![0; 4],
            },
        ];

        let error = ensure_disjoint(&routines, 0xF4B0).unwrap_err();

        assert!(error.to_string().contains("overlaps"));
    }
}
```

Run: `cargo test --workspace runtime_code -- --nocapture`
Expected: PASS

- [ ] **Step 4: clippy와 전체 테스트, 커밋**

```bash
cargo test --workspace && cargo clippy --workspace --all-targets
git add apps/fc_fire_emblem_patch/src/full_translation_install/runtime_code/
git commit -m "Emit the dispatcher hold gate and the cold initializer"
```

---

### Task 8: 방출한 바이트를 누적 이미지에 결속한다

여기까지는 코드를 만들기만 했다. 이 과제가 그것을 ROM에 넣고, `runtime_control_flow.rs`의 훅 자리를 `$C179`로 옮기고, 계획 보고서가 «방출됨»을 말하게 한다.

**Files:**
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_control_flow.rs:15` (`NMI_HOOK`)
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install/integrated_write_set.rs`
- Modify: `apps/fc_fire_emblem_patch/src/full_translation_install.rs`

**Interfaces:**
- Consumes: Task 6의 `build_trampoline`·`hook_bytes`, Task 5의 `build_transport_routine`, Task 7의 `build_dispatcher_gate`·`build_cold_initializer`
- Produces: `pub(super) fn dialogue_runtime_writes(source: &Rom, candidate: &Rom, runtime_code_cpu_start: u16) -> Result<Vec<(usize, Vec<u8>)>>` — 파일 오프셋과 바이트의 쌍 목록

- [ ] **Step 1: 실패하는 테스트를 쓴다**

`integrated_write_set.rs`의 `mod tests`에 넣는다.

```rust
/// 대사 런타임의 쓰기는 반드시 예약된 자리 안에만 떨어져야 한다.
/// 원본 바이트를 덮으면 되돌릴 수 없는 손상이다.
#[test]
fn dialogue_runtime_writes_land_only_in_reserved_caves() {
    let rom = crate::test_support::release_rom();

    let writes = dialogue_runtime_writes(&rom, &rom, 0xB000).unwrap();

    assert!(!writes.is_empty());
    for (offset, bytes) in &writes {
        let existing = &rom.data()[*offset..*offset + bytes.len()];
        let is_hook = bytes.len() == 3 && bytes[0] == 0x20;
        assert!(
            is_hook || existing.iter().all(|byte| *byte == 0xFF),
            "write at {offset:#X} overwrites non-reserved bytes"
        );
    }
}
```

- [ ] **Step 2: 테스트를 돌려 실패를 확인한다**

Run: `cargo test --workspace dialogue_runtime_writes_land_only`
Expected: FAIL — `cannot find function dialogue_runtime_writes`

- [ ] **Step 3: 쓰기 집합을 구현한다**

`integrated_write_set.rs`에 추가한다.

```rust
use super::{
    runtime_bank_contract::bind_bank_restore_contract,
    runtime_code::{
        dispatcher_gate::{build_cold_initializer, build_dispatcher_gate},
        ensure_disjoint,
        trampoline::{TRAMPOLINE_CAVE_END, build_trampoline, hook_bytes},
        transport::build_transport_routine,
    },
    runtime_nmi_contract::{CONSUMER_HOOK, bind_quiet_frame_gate},
};

/// 대사 런타임이 누적 이미지에 넣는 쓰기다. 파일 오프셋과 바이트의 쌍이다.
pub(super) fn dialogue_runtime_writes(
    source: &Rom,
    candidate: &Rom,
    runtime_code_cpu_start: u16,
) -> Result<Vec<(usize, Vec<u8>)>> {
    bind_quiet_frame_gate(source, candidate)?;
    let bank = bind_bank_restore_contract(source)?;

    let transport = build_transport_routine(runtime_code_cpu_start)?;
    let trampoline = build_trampoline(bank, transport.address)?;
    let gate = build_dispatcher_gate(
        trampoline.address + trampoline.bytes.len() as u16,
        0x8003,
    )?;
    let fixed_routines = vec![trampoline, gate];
    ensure_disjoint(&fixed_routines, TRAMPOLINE_CAVE_END)?;

    let mut writes = Vec::new();
    for routine in &fixed_routines {
        writes.push((fixed_file_offset(candidate, routine.address), routine.bytes.clone()));
    }
    writes.push((
        runtime_code_file_offset(candidate, transport.address)?,
        transport.bytes,
    ));
    writes.push((
        fixed_file_offset(candidate, CONSUMER_HOOK),
        hook_bytes().to_vec(),
    ));
    let _ = build_cold_initializer;
    Ok(writes)
}

fn fixed_file_offset(rom: &Rom, address: u16) -> usize {
    HEADER_SIZE + rom.prg().len() - 16 * 1024 + usize::from(address) - 0xC000
}

fn runtime_code_file_offset(rom: &Rom, address: u16) -> Result<usize> {
    let page_offset = usize::from(0x2E_u8 - 0x2C) * 8 * 1024;
    let within_window = usize::from(address)
        .checked_sub(0xA000)
        .context("runtime code address is outside the A000 window")?;
    Ok(HEADER_SIZE + page_offset + within_window - 2 * 8 * 1024 + runtime_page_base(rom))
}
```

`runtime_page_base`는 MMC3 페이지 `2C`가 PRG 안에서 시작하는 파일 오프셋이다. `installation_layout.rs`가 이미 그 값을 계산하고 있으므로 그 함수를 재사용한다. 다음으로 확인한다.

```bash
grep -n "0x2C\|MMC3_PAGE\|page_offset\|fn.*offset" apps/fc_fire_emblem_patch/src/full_translation_install/installation_layout.rs | head -20
```

`installation_layout.rs`에 페이지→오프셋 변환이 있으면 `pub(super)`로 올려 쓰고, 없으면 `fn runtime_page_base(_rom: &Rom) -> usize { 0x2C * 8 * 1024 }`로 두되 Step 4의 테스트가 그 값을 검증하게 한다.

- [ ] **Step 4: 오프셋이 맞는지 검증하는 테스트를 추가한다**

```rust
/// 실행 코드 쓰기가 페이지 `2E`의 예약 꼬리에 정확히 떨어져야 한다.
/// 오프셋이 어긋나면 다른 도메인의 자료를 덮는다.
#[test]
fn the_transport_write_lands_in_the_reserved_tail_of_page_2e() {
    let rom = crate::test_support::release_rom();
    let runtime_code_cpu_start = 0xB000;

    let writes = dialogue_runtime_writes(&rom, &rom, runtime_code_cpu_start).unwrap();
    let (offset, bytes) = writes
        .iter()
        .max_by_key(|(_, bytes)| bytes.len())
        .expect("the transport routine is the longest write");

    assert!(
        rom.data()[*offset..*offset + bytes.len()]
            .iter()
            .all(|byte| *byte == 0xFF),
        "the transport routine would overwrite emitted material"
    );
}
```

Run: `cargo test --workspace integrated_write_set -- --nocapture`
Expected: PASS. 실패하면 `runtime_code_file_offset`의 계산이 틀린 것이므로, 실패 메시지가 가리키는 오프셋의 실제 내용을 `xxd -s <offset> -l 32 out/fire-emblem-fe1-korean-release.nes`로 확인해 맞춘다.

- [ ] **Step 5: `NMI_HOOK`을 옮긴다**

`runtime_control_flow.rs:15`의 `const NMI_HOOK: u16 = 0xC191;`를 다음으로 바꾼다.

```rust
/// 대사 소비자가 들어가는 자리다. `$C191`이 아닌 이유는 의사결정 64번에 있다.
const NMI_HOOK: u16 = 0xC179;
/// 전투 합성이 계속 쓰는 자리다. 소유자가 다르므로 그대로 둔다.
const BATTLE_NMI_HOOK: u16 = 0xC191;
```

`NMI_HOOK`을 쓰던 검사(`fixed_bytes(inputs.candidate, NMI_HOOK, 3)? == [0x20, 0xFC20…]`)는 `BATTLE_NMI_HOOK`으로 바꾼다. 전투 훅 검사는 그대로 유지돼야 한다.

`NmiConsumer`의 `source_hook_cpu_address_hex`를 `"0xC179"`로 바꾸고, 필드 `battle_composition_priority_preserved`를 `quiet_frame_gate_bound: bool`로 바꾼다. 후자는 `bind_quiet_frame_gate`가 성공했음을 담는다.

- [ ] **Step 6: clippy와 전체 테스트**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS, 경고 없음

- [ ] **Step 7: 커밋**

```bash
git add apps/fc_fire_emblem_patch/src/full_translation_install/
git commit -m "Bind the dialogue runtime writes to the cumulative image"
```

---

### Task 9: 누적 빌드와 배포 이미지를 다시 만들고 정적 산출물을 확인한다

**Files:**
- 없음(빌드 실행)

- [ ] **Step 1: 누적 빌드를 돌린다**

```bash
cd "$(git rev-parse --show-toplevel)"
cargo run --release -- --help 2>&1 | head -40
```

출력에서 누적 패치를 만드는 하위 명령 이름을 확인한다(`BuildReleaseImage`가 받는 `--cumulative` 입력을 만드는 명령이다).

- [ ] **Step 2: 배포 이미지를 만든다**

```bash
cargo run --release -- build-release-image \
  --cumulative out/<누적 산출물>.nes \
  --output out/fire-emblem-fe1-korean-release.nes \
  --report out/release-image.json
```

Expected: 종료 코드 0, `out/release-image.json`의 `header_declares_chr_ram`이 `true`

- [ ] **Step 3: 훅이 실제로 들어갔는지 확인한다**

```bash
python3 - <<'PY'
data = open("out/fire-emblem-fe1-korean-release.nes","rb").read()
prg = data[16:16+512*1024]
fixed = prg[-16*1024:]
def at(addr, n): return fixed[addr-0xC000:addr-0xC000+n].hex(" ")
print("C179:", at(0xC179, 3), "(expect 20 00 f4)")
print("F400:", at(0xF400, 32))
PY
```

Expected: `C179: 20 00 f4`

- [ ] **Step 4: 커밋**

```bash
git add out/release-image.json
git commit -m "Rebuild the release image with the dialogue transport installed"
```

---

### Task 10: 에뮬레이터에서 vblank 미초과를 실측한다

정적 논증은 «게이트가 통과한 프레임에 원본이 0바이트를 쓴다»와 «복사 루프가 언롤이다»로 닫혀 있다. 실행 검증은 그 둘이 실제 ROM에서도 참인지 본다.

**Files:**
- 없음(측정)

- [ ] **Step 1: 배포 이미지를 띄운다**

emucap MCP로 `bootstrap` → `launch(content_path=out/fire-emblem-fe1-korean-release.nes, system="nes", start_frozen=true)`.

- [ ] **Step 2: 소비자가 도는 프레임의 vblank 잔여를 잰다**

`$C3A5`에 `pause_on_hit=true` 실행 중단점을 건다. 트램폴린은 모든 경로에서 `JMP $C3A5`로 끝나므로 이 자리가 소비자 종료 시점이다.

`resume` → `get_state(groups=["ppu"])`로 `ppu.scanline`과 `ppu.cycle`을 읽는다. 남은 vblank CPU 사이클은 `((261 - scanline) * 341 - cycle) / 3`이다.

Expected: `scanline`이 241..=260 안에 있고 잔여가 0보다 크다. **`scanline`이 0..=240이면 예산 위반이므로 `TILES_PER_FRAME`을 낮추고 Task 5부터 다시 한다.**

- [ ] **Step 3: 게이트가 통과한 프레임에 원본 쓰기가 없는지 확인한다**

`$2007`에 `pause_on_hit=false` 쓰기 중단점을, `$F400`에 `pause_on_hit=false` 실행 중단점(`snapshot=["nesMemory:0x0021:2","nesMemory:0x0089:2"]`)을 건다. 300프레임을 `step`으로 진행한 뒤 `poll_events`를 파일로 받는다.

`evidence/private/nmi-vblank-budget/gate.py`로 분석한다.

```bash
python3 evidence/private/nmi-vblank-budget/gate.py <poll 결과 파일>
```

Expected: `quiet-frame $2007 writes (must be 0)`가 소비자 자신의 쓰기만 담는다. 소비자 쓰기의 PC는 `$A000..$C000` 안이므로 원본 쓰기(`0xC3E0`·`0xD504`)와 구분된다.

- [ ] **Step 4: 대사가 진행되는지 확인한다**

`evidence/private/chapter7-maximum-page-reload/next-story.mss`를 `load_state`로 올리고 `tap`으로 대사를 진행시킨다. `screenshot`으로 창이 열리고 글자가 나오는지 본다.

Expected: 대사가 잠깐 멈춘 뒤 진행된다. 멈춘 채 돌아오지 않으면 `read_memory(nesMemory, 0x07F4, 1)`로 상태를 읽어 `ready(3)`에 도달하는지 본다. 도달하지 않으면 `$07EF`(남은 타일)가 줄어드는지 확인해 전송이 도는지 굶는지 가른다.

- [ ] **Step 5: 측정 결과를 의사결정 로그에 남긴다**

`docs/decisions.md`의 64번 아래에 실측 결과 한 문단을 추가한다. 최소한 다음을 적는다 — 소비자 종료 시점의 최악 scanline, 표본 프레임 수, 조용한 프레임에서 원본 쓰기가 0건인지.

- [ ] **Step 6: 커밋**

```bash
git add docs/decisions.md
git commit -m "Record the measured vblank headroom of the installed consumer"
```

---

## 자체 검토

**1. 스펙 대응.** 실기 불변 조건 넷 중 «vblank 밖 `$2007` 금지»는 Task 5·6·10이, «에뮬레이터 관용 불의존»은 Task 10의 정적+실행 이중 확인이 진다. 나머지 둘(매퍼 주소 범위, 파일 형식)은 이미 `release_image.rs`가 닫아 두었다. 안전 성질은 Task 7의 `a_pending_request_returns_without_running_the_handler`와 Task 5의 `readiness_is_published_only_after_the_last_tile`이 함께 진다. 휘발 상태·소비자·게이트·프레임 예산·실패 처리는 각각 Task 4·6·7·5에 대응한다. **합성 절차와 나머지 생산자 넷, 동적 remap 투영은 이 계획에 없다** — 위 «범위» 절에 명시했고 후속 계획 B·C가 받는다.

**2. 플레이스홀더.** 없음. Task 1과 Task 8 Step 3은 «먼저 값을 읽고 그 값을 넣어라»는 지시인데, 읽는 방법과 넣을 자리를 실행 가능한 명령으로 적었으므로 TBD가 아니다.

**3. 타입 일관성.** `RuntimeRoutine`은 `transport.rs`에서 한 번 정의하고 `trampoline.rs`·`dispatcher_gate.rs`·`mod.rs`가 그것을 쓴다. `REQUEST_STATE`·`STATE_READY`도 `transport.rs`가 단일 출처다. 커서 상수 다섯은 `runtime_cursor_storage.rs`가 단일 출처이고 Task 5·7이 그것을 가져다 쓴다. `BankRestoreContract`의 필드 이름은 Task 1의 정의와 Task 6의 사용이 일치한다.

**4. 알려진 위험.** Task 5 Step 4의 사이클 테스트가 실패할 가능성이 높다. 실제 복사 몸통이 바이트당 8사이클보다 비싸기 때문이다(`LDA (zp),Y`가 5사이클). 그 경우 `TILES_PER_FRAME`을 8로 낮추는 것이 정답이고, 그 사실이 테스트로 드러나는 것이 이 계획의 설계 의도다. 예산을 지키려고 언롤을 `LDA abs,X`로 바꾸면 atlas가 페이지 경계를 넘을 때 주소 계산이 복잡해지므로, 속도가 문제가 될 때 별도로 다룬다.
