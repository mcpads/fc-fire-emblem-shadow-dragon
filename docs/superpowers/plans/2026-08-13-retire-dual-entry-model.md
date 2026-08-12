# 이중 진입 모델 폐기 실행 계획

> **에이전트 작업자에게:** 이 계획은 `superpowers:subagent-driven-development` 또는 `superpowers:executing-plans`로 한 과제씩 실행한다. 단계는 체크박스(`- [ ]`)로 추적한다.

**목표:** 레코드 프리픽스 파서 결함이 만들어 낸 「직접 진입 / 전이 진입」 구분과 그 위에 세운 구조를 코드와 자산에서 제거한다.

**접근:** 잎에서 뿌리로 올라간다. 소비자 쪽 결속을 먼저 끊어 각 단계가 컴파일과 테스트를 통과하게 만들고, 아무도 부르지 않게 된 모듈을 마지막에 지운다. 중간에 빌드가 깨진 채로 다음 단계로 넘어가지 않는다.

**기술 스택:** Rust 2024, `cargo test --workspace`, `cargo clippy --workspace --all-targets`

## 전역 제약

- 지원 원본은 SHA-1 `0179c550d424e0397496078789e7b116601d120c`인 일본판 하나다.
- `cargo test --workspace`는 매 과제 끝에서 484개 이상 통과하고 실패 0이어야 한다.
- `cargo clippy --workspace --all-targets` 경고는 기존 6건을 넘지 않는다.
- `verify-main-dialogue-source-roundtrip`은 매 과제 끝에서 원본 SHA-1을 그대로 재생산해야 한다.
- 원본 ROM과 빌드 결과는 커밋하지 않는다. `private/`, `out/`, `evidence/private/`는 무시 대상이다.
- 과제마다 커밋한다. 한 과제가 실패하면 다음 과제로 넘어가지 않는다.

## 폐기 근거

의사결정 59번을 따른다. 139개 전이 대상의 직접·전이 진입 차이는 자료의 성질이 아니라 두 경로가 같은 바이트를 다른 규칙으로 읽어서 생긴 것이고, 세 형태를 각각 실행으로 확인해 전이 파서 쪽이 화면과 일치함을 확정했다. 파서를 실제 배치에 맞춘 뒤 139개 전부에서 두 값이 같아졌다.

## 파일 구조

제거 대상과 각각의 책임이다. 괄호 안은 줄 수다.

| 경로 | 책임 | 처리 |
| --- | --- | --- |
| `dialogue_assets/entry_mode_workspace/` 6파일 | 139레코드 417파트 작업공간, 초벌 반입, 검증, 표시 계획 | 삭제 |
| `dialogue_assets/bundle/paired_entry_storage.rs` | 두 모드 중 긴 길이를 예약하는 저장 계산 | 삭제 |
| `dialogue_inventory/main_dialogue_entry_modes.rs` (167) | 전이 대상의 모드별 델타 조사 | 삭제 |
| `full_translation_install/consumer_visible_prefixes.rs` (20,459바이트) | 직접 선두·공통 본문·전이 선두 정규화 | 삭제 |
| `full_translation_install/relocated_dialogue_banks.rs` + `relocated_dialogue_banks/` | 확장 미러 뱅크 `11`~`15`와 전이 resolver·reader·NMI 복귀 routine | 삭제 |
| `private/dialogue/entry-mode-workspace.json` | 이중 진입 번역 자산 | 삭제 (git 무시 대상) |

수정 대상이다.

| 경로 | 수정 내용 |
| --- | --- |
| `main.rs` | CLI 3개(`extract`/`validate`/`import` entry-mode)와 `PlanFullTranslationInstallation`의 `main_dialogue_entry_mode_workspace` 인자 제거 |
| `dialogue_assets.rs` | `entry_mode_workspace` 모듈 선언과 재수출 제거 |
| `dialogue_assets/bundle.rs` | `paired_entry_storage` 참조 제거 |
| `dialogue_inventory.rs` | `main_dialogue_entry_modes` 모듈 선언과 호출 제거 |
| `full_translation_install.rs` | 이중 진입 입력·필드·게이트 제거, 표시 경로를 일반 365경로로 재계산 |
| `full_translation_install/installation_layout.rs` | 미러 뱅크 `11`~`15` 배치 제거 |
| `full_translation_install/integrated_write_set.rs` | 전이 미러 Expected Write 제거 |
| `full_translation_install/runtime_state_storage.rs` | 전이 미러 뱅크 참조 제거 |
| `docs/status.md`, `docs/roadmap.md`, `docs/build-pipeline.md` | 이중 진입 기술을 폐기 사실로 교체 |

---

### 과제 1: CLI에서 이중 진입 명령 세 개를 뗀다

**파일:**
- 수정: `apps/fc_fire_emblem_patch/src/main.rs`

**인터페이스:**
- 소비: 없음
- 생산: `Command` enum에서 `ExtractMainDialogueEntryModeWorkspace`, `ValidateMainDialogueEntryModeWorkspace`, `ImportMainDialogueEntryModeDraft`가 사라진다. `PlanFullTranslationInstallation`은 `main_dialogue_entry_mode_workspace` 인자를 더 이상 받지 않는다.

이 과제는 스크립트로 하지 않는다. 앞선 시도에서 정규식 삭제가 `enum Command` 헤더까지 지웠다. 편집기로 블록을 눈으로 확인하며 지운다.

- [ ] **1단계: 현재 상태를 확인한다**

```bash
grep -n "EntryMode" apps/fc_fire_emblem_patch/src/main.rs
```

기대: 6곳. variant 정의 3곳과 match 팔 3곳이다.

- [ ] **2단계: `Command` enum에서 variant 세 개를 지운다**

각 variant는 `/// ...` 주석 줄부터 닫는 `},`까지가 한 덩어리다. 세 개를 지운다. `enum Command {` 헤더와 다른 variant는 건드리지 않는다.

- [ ] **3단계: `match` 팔 세 개를 지운다**

`Command::ExtractMainDialogueEntryModeWorkspace { .. } => { .. }` 형태다. 여는 중괄호와 닫는 중괄호가 맞는지 세면서 지운다.

- [ ] **4단계: `PlanFullTranslationInstallation`에서 인자를 뗀다**

variant 정의의 `#[arg(long, default_value = "private/dialogue/entry-mode-workspace.json")]`와 그 아래 `main_dialogue_entry_mode_workspace: PathBuf,`를 지운다. match 팔의 구조 분해 목록에서 `main_dialogue_entry_mode_workspace,`를 지우고, `FullTranslationInstallInputs` 생성부에서 `main_dialogue_entry_mode_workspace_path: &main_dialogue_entry_mode_workspace,`를 지운다.

- [ ] **5단계: 컴파일을 확인한다**

```bash
cargo build -p fc-fire-emblem-patch 2>&1 | head -30
```

기대: `full_translation_install`이 아직 그 필드를 요구하므로 `FullTranslationInstallInputs` 관련 오류만 난다. 다른 오류가 나면 2~4단계에서 잘못 지운 것이므로 `git checkout -- apps/fc_fire_emblem_patch/src/main.rs`로 되돌리고 다시 한다.

- [ ] **6단계: 입력 구조체에서 필드를 뗀다**

`full_translation_install.rs`의 `FullTranslationInstallInputs`에서 `pub(crate) main_dialogue_entry_mode_workspace_path: &'a Path,`를 지운다. 그 필드를 쓰던 `validate_main_dialogue_entry_mode_workspace`와 `plan_normalized_main_dialogue_display` 호출은 다음 과제에서 지우므로, 지금은 컴파일이 통과하도록 임시로 `inputs.main_dialogue_workspace_path`를 넘기지 않는다. 대신 두 호출을 지우고 그 결과를 쓰는 자리를 과제 2에서 정리한다. 이 과제에서는 컴파일이 깨진 채로 두지 않기 위해 과제 2와 함께 한 커밋으로 묶는다.

- [ ] **7단계: 커밋하지 않고 과제 2로 넘어간다**

CLI만 떼면 컴파일이 통과하지 않는다. 과제 2 끝에서 함께 커밋한다.

---

### 과제 2: 설치 계획에서 이중 진입 결속을 끊는다

**파일:**
- 수정: `apps/fc_fire_emblem_patch/src/full_translation_install.rs`
- 수정: `apps/fc_fire_emblem_patch/src/full_translation_install/installation_layout.rs`
- 수정: `apps/fc_fire_emblem_patch/src/full_translation_install/integrated_write_set.rs`
- 수정: `apps/fc_fire_emblem_patch/src/full_translation_install/runtime_state_storage.rs`

**인터페이스:**
- 소비: 과제 1의 `FullTranslationInstallInputs`(이중 진입 필드 없음)
- 생산: `FullTranslationInstallReport`에서 `normalized_entry_mode_*` 다섯 필드, `consumer_visible_prefixes`, `selected_relocated_bank_plan`, `normalized_entry_mode_bodies_bound`가 사라진다. `dialogue_codebook.display_path_count`는 643이 아니라 일반 365경로만 센다.

- [ ] **1단계: 호출을 지운다**

`full_translation_install.rs`의 다음을 지운다.

```rust
let entry_mode_validation = validate_main_dialogue_entry_mode_workspace(
    // ...
)?;
let display = plan_normalized_main_dialogue_display(
    // ...
)?;
```

`display`를 쓰던 자리는 `MainDialogueDisplayPlan::from_canonical_bundle(&dialogue)?`가 이미 만드는 값으로 대체한다. 그 줄은 367행에 있다.

- [ ] **2단계: 보고서 필드를 지운다**

`TranslationInputs`에서 `normalized_entry_mode_record_count`, `normalized_entry_mode_part_count`, `normalized_entry_mode_leading_japanese_occurrence_count`, `normalized_entry_mode_common_body_japanese_source_byte_count`, `normalized_entry_mode_untranslated_japanese_part_count`, `mode_specific_visible_prefix_japanese_source_byte_count`, `mode_specific_visible_prefix_translation_input_complete`를 지운다. `DialogueStorage`에서 `normalized_entry_mode_bodies_bound`와 `selected_relocated_bank_plan`을 지운다. 최상위 보고서에서 `consumer_visible_prefixes`를 지운다.

- [ ] **3단계: 게이트에서 이중 진입 항을 뗀다**

```rust
let translation_input_complete = entry_mode_validation.translation_input_complete
    && consumer_visible_prefixes.translation_input_complete();
```

를 대사 워크스페이스의 입력 완료 판정만 쓰도록 바꾼다. `review_complete` 판정에서도 `entry_mode_validation.review_complete` 항을 뺀다.

- [ ] **4단계: 미러 뱅크 배치를 뗀다**

`installation_layout.rs`에서 `transition_mirror_banks` 필드와 그 계산을 지운다. `integrated_write_set.rs`에서 전이 미러 Expected Write 항목을 지운다. `runtime_state_storage.rs`에서 미러 뱅크 참조를 지운다.

- [ ] **5단계: 모듈 선언과 use를 지운다**

`full_translation_install.rs`에서 `mod consumer_visible_prefixes;`, `mod relocated_dialogue_banks;`와 대응하는 `use` 두 줄을 지운다.

- [ ] **6단계: 컴파일과 테스트**

```bash
cargo build -p fc-fire-emblem-patch 2>&1 | head -30
cargo test --workspace 2>&1 | tail -3
```

기대: 컴파일 통과. 테스트는 이중 진입을 참조하던 것이 남아 있으면 실패하므로, 실패한 테스트를 읽고 그 테스트가 폐기 대상 동작만 검증하는지 확인한 뒤 지운다. 다른 동작도 함께 보고 있으면 그 부분만 남긴다.

- [ ] **7단계: 왕복 검증**

```bash
cargo run -q -p fc-fire-emblem-patch -- verify-main-dialogue-source-roundtrip \
  "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes" 2>&1 | tail -2
```

기대: `output SHA-1: 0179c550d424e0397496078789e7b116601d120c`

- [ ] **8단계: 커밋**

```bash
git add apps/fc_fire_emblem_patch/src/main.rs apps/fc_fire_emblem_patch/src/full_translation_install.rs apps/fc_fire_emblem_patch/src/full_translation_install/
git commit -m "Stop binding the install plan to dual dialogue entry modes"
```

---

### 과제 3a: 미러 뱅크 계산 경로를 끊는다

과제 2를 실행하며 확인한 사실이다. 보고서 필드만 지웠을 뿐 `plan_relocated_dialogue_banks` 호출과 `installation_layout`·`integrated_write_set`의 사용이 남아 있다. 모듈을 지우기 전에 이 셋을 먼저 끊는다.

**파일:**
- 수정: `full_translation_install.rs`, `full_translation_install/installation_layout.rs`, `full_translation_install/integrated_write_set.rs`

- [ ] **1단계: 호출과 사용을 지운다**

`full_translation_install.rs`의 `plan_relocated_dialogue_banks(...)` 호출과 그 결과를 쓰는 `expected_dialogue_storage_write_count`를 정리한다. `installation_layout.rs`에서 미러 뱅크 배치를, `integrated_write_set.rs`에서 `append_relocated_dialogue_writes`를 뗀다.

- [ ] **2단계: 컴파일·테스트·왕복을 확인한다**

### 과제 3b: `MainDialogueDisplayPlan`을 이중 진입 밖으로 옮긴다

과제 2를 실행하며 확인한 사실이다. 이 타입은 `entry_mode_workspace/display_plan.rs` 안에 있지만 이중 진입과 무관하고, 6개 모듈 12곳이 쓴다. `from_canonical_bundle`은 이미 이중 진입 필드를 0과 빈 벡터로 채우므로 분리가 깨끗하다.

**파일:**
- 생성: `apps/fc_fire_emblem_patch/src/dialogue_assets/display_plan.rs`
- 수정: `dialogue_assets.rs`

- [ ] **1단계: 살아 있는 부분만 새 모듈로 옮긴다**

`MainDialogueDisplayPlan` struct에서 `dual_entry_record_count`, `direct_display_path_count`, `transition_display_path_count`, `normalized_record_storage` 네 필드를 뺀다. 소비자가 읽지 않는 것을 확인했다. `from_canonical_bundle`과 `unique_glyphs`만 남긴다.

- [ ] **2단계: 재수출을 새 모듈로 돌린다**

- [ ] **3단계: 컴파일·테스트·왕복을 확인한다**

### 과제 3: 이중 진입 모듈을 지운다

**파일:**
- 삭제: `apps/fc_fire_emblem_patch/src/dialogue_assets/entry_mode_workspace.rs`와 `entry_mode_workspace/` 6파일
- 삭제: `apps/fc_fire_emblem_patch/src/dialogue_assets/bundle/paired_entry_storage.rs`
- 삭제: `apps/fc_fire_emblem_patch/src/dialogue_inventory/main_dialogue_entry_modes.rs`
- 삭제: `apps/fc_fire_emblem_patch/src/full_translation_install/consumer_visible_prefixes.rs`
- 삭제: `apps/fc_fire_emblem_patch/src/full_translation_install/relocated_dialogue_banks.rs`와 `relocated_dialogue_banks/`
- 수정: `dialogue_assets.rs`, `dialogue_assets/bundle.rs`, `dialogue_inventory.rs`

**인터페이스:**
- 소비: 과제 2의 상태(아무도 이 모듈들을 부르지 않음)
- 생산: 없음. 순수 제거다.

- [ ] **1단계: 부르는 곳이 없는지 확인한다**

```bash
grep -rn "entry_mode\|EntryMode\|paired_entry\|PairedEntry\|consumer_visible_prefixes\|relocated_dialogue_banks" apps/fc_fire_emblem_patch/src/ | grep -v "^apps/fc_fire_emblem_patch/src/dialogue_assets/entry_mode_workspace" | grep -v "^apps/fc_fire_emblem_patch/src/full_translation_install/relocated_dialogue_banks" | grep -v "^apps/fc_fire_emblem_patch/src/full_translation_install/consumer_visible_prefixes"
```

기대: 모듈 선언(`mod ...;`)과 재수출(`pub(crate) use ...`)만 남는다. 실제 호출이 남아 있으면 과제 2가 덜 끝난 것이므로 돌아간다.

- [ ] **2단계: 모듈 선언과 재수출을 지운다**

`dialogue_assets.rs`의 `mod entry_mode_workspace;`와 44~48행의 `pub(crate) use entry_mode_workspace::{ ... };`를 지운다. `dialogue_assets/bundle.rs`의 `paired_entry_storage` 선언을 지운다. `dialogue_inventory.rs`의 `main_dialogue_entry_modes` 선언과 호출을 지운다.

- [ ] **3단계: 파일을 지운다**

```bash
git rm -r apps/fc_fire_emblem_patch/src/dialogue_assets/entry_mode_workspace.rs \
          apps/fc_fire_emblem_patch/src/dialogue_assets/entry_mode_workspace \
          apps/fc_fire_emblem_patch/src/dialogue_assets/bundle/paired_entry_storage.rs \
          apps/fc_fire_emblem_patch/src/dialogue_inventory/main_dialogue_entry_modes.rs \
          apps/fc_fire_emblem_patch/src/full_translation_install/consumer_visible_prefixes.rs \
          apps/fc_fire_emblem_patch/src/full_translation_install/relocated_dialogue_banks.rs \
          apps/fc_fire_emblem_patch/src/full_translation_install/relocated_dialogue_banks
```

- [ ] **4단계: 컴파일·테스트·clippy**

```bash
cargo build -p fc-fire-emblem-patch 2>&1 | head -20
cargo test --workspace 2>&1 | tail -3
cargo clippy --workspace --all-targets 2>&1 | grep generated
```

기대: 컴파일 통과, 테스트 실패 0, clippy 경고 6건 이하.

- [ ] **5단계: 커밋**

```bash
git add -A
git commit -m "Remove the dual dialogue entry-mode modules"
```

---

### 과제 4: 이중 진입 자산과 문서를 정리한다

**파일:**
- 삭제: `private/dialogue/entry-mode-workspace.json`과 그 백업들
- 수정: `docs/status.md`, `docs/roadmap.md`, `docs/build-pipeline.md`

**인터페이스:**
- 소비: 과제 3의 상태
- 생산: 문서가 폐기 사실을 반영한다.

- [ ] **1단계: 자산을 옮긴다**

지우기 전에 백업 디렉터리로 옮긴다. `private/`는 git 무시 대상이라 복구 수단이 백업뿐이다.

```bash
mkdir -p private/dialogue/retired
mv private/dialogue/entry-mode-workspace*.json private/dialogue/retired/
```

- [ ] **2단계: `docs/status.md`를 고친다**

이중 진입 417파트, 미러 뱅크 `11`~`15`, 전이 resolver·reader·NMI 복귀 routine, 643개 표시 경로, 최대 대사 수요 `187/210`을 서술한 문단을 찾아 폐기 사실과 재계산 필요로 바꾼다. 의사결정 59번을 참조한다.

- [ ] **3단계: `docs/roadmap.md`를 고친다**

「즉시 실행할 작업」 12~14번의 이중 진입 항목을 폐기 표시로 바꾸고, G4·G5 서술에서 417파트와 `187/210`을 뺀다.

- [ ] **4단계: `docs/build-pipeline.md`를 고친다**

전이 미러 뱅크와 네 routine을 서술한 문단을 지운다.

- [ ] **5단계: 커밋**

```bash
git add docs/
git commit -m "Record the retired dual entry-mode structure in the project docs"
```

---

### 과제 5: 표시 경로와 대사 수요를 다시 계산한다

**파일:**
- 수정: `apps/fc_fire_emblem_patch/src/full_translation_install.rs`

**인터페이스:**
- 소비: 과제 3의 상태
- 생산: `dialogue_codebook.display_path_count`가 일반 경로만 세고, `maximum_workset_slot_demand`가 그 경로 집합에서 다시 나온다.

- [ ] **1단계: 현재 값을 기록한다**

```bash
cargo run -q -p fc-fire-emblem-patch -- plan-full-translation-installation \
  "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes" 2>&1 | tail -5
```

기대: 과제 2에서 이중 진입 항을 뺐으므로 실행은 되지만 경로 수가 643이 아닌 값으로 나온다. 그 값을 적어 둔다.

- [ ] **2단계: 보고서에서 경로 수와 수요를 확인한다**

```bash
python3 -c "
import json
d=json.load(open('out/full-translation-installation.json'))
c=d['dialogue_codebook']
for k in ['display_path_count','ordinary_record_count','page_workset_count','maximum_workset_slot_demand','static_page_upper_bound_count']:
    print(k, c.get(k))
"
```

기대: `display_path_count`가 365(일반 경로)와 같고 `direct_display_path_count`·`transition_display_path_count`가 사라졌다.

- [ ] **3단계: 새 최대 수요를 문서에 적는다**

`docs/status.md`의 최대 대사 수요를 2단계에서 읽은 `maximum_workset_slot_demand`로 바꾸고, 그 값이 전역 최악 `209/210`을 넘지 않는지 함께 적는다. 넘으면 그 사실을 적고 다음 관문으로 남긴다.

- [ ] **4단계: 커밋**

```bash
git add docs/status.md out/full-translation-installation.json 2>/dev/null || git add docs/status.md
git commit -m "Recount dialogue display paths without the dual entry modes"
```

---

## 자체 검토

**폐기 범위 대조:** 의사결정 59번이 폐기 대상으로 적은 여섯 가지 — 417파트 작업공간(과제 3), 정규화(과제 3), 미러 뱅크(과제 2·3), routine 네 개(과제 2·3), 278개 이중 표시 경로(과제 5), `187/210` 재계산(과제 5) — 가 모두 과제에 배정됐다.

**빈칸 점검:** 각 단계에 실행할 명령이나 지울 대상이 이름으로 적혀 있다. "적절히 처리한다" 같은 표현은 없다.

**이름 일관성:** `FullTranslationInstallInputs`, `MainDialogueDisplayPlan::from_canonical_bundle`, `validate_main_dialogue_entry_mode_workspace`, `plan_normalized_main_dialogue_display`는 현재 코드에 있는 이름 그대로다.

**위험 지점:** 과제 1과 과제 2는 중간에 컴파일이 깨지므로 한 커밋으로 묶는다. 계획에 그렇게 적었다. 과제 1의 5단계에서 예상 밖 오류가 나면 `git checkout`으로 되돌리고 다시 하라고 명시했다.
