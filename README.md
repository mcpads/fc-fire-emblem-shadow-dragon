# FC Fire Emblem Korean patch

패미컴판 《파이어 엠블렘 암흑룡과 빛의 검》의 한글화 프로젝트다.

## 번역 범위

- 지원 원본은 SHA-1 `0179c550d424e0397496078789e7b116601d120c`인 일본판이다.
- **일본어만 한국어로 번역한다.** 일본판에 원래 들어 있는 영어, 숫자, 로마자 약어는 번역하거나 글꼴 슬롯을 덮지 않는다.
- 영문 패치는 주소와 자료 구조를 교차 확인하는 조사 자료일 뿐, 번역 원문이나 제품 빌드 입력이 아니다.

## 현재 게이트

`plan-full-translation-installation`은 지원 일본판 원본과 누적 후보를 결속해 선언된 14개 번역 도메인, 504개 대사 레코드, 928개 페이지 작업집합을 하나의 통합 개발 ROM에 설치한다. 현재 누적 ROM SHA-1은 `66E96A63663EAF2CB31C4E6B2BE6EBED61D0B572`, 보고서 SHA-1은 `6D38471C76C2F037231604681B31A26DB352A762`다. 이를 입력으로 만든 정확 통합 ROM SHA-1은 `6E7604E23F506B87C8C800B4DDF86A4B6E441489`, 스키마 40 보고서 SHA-1은 `52222B8F63958860711B7AA1AA00089F274D5CFA`다. 715개 글리프의 정적 페이지 상한은 36페이지다. 기술 설치는 통과했지만 소비 경로 전수 조사와 나머지 최종 산출물 실행 결속을 대신하지 않는다. 직접 합성 상태 `02..26`의 37개는 모두 명시적인 페이지 정책에 결속됐다. 상태 `11/17`은 원본 handler와 생산자 가족 전체가 글리프 없이 `ED/EF` 제어만 만드는 경우로 한정해 현재 페이지를 유지한다. 상태 `25`의 원본 유닛 선택 도움말 6줄은 뒤이어 열리는 `B1:52` 대사와 같은 전역 코드 배정을 쓰며, 도움말 타일이 화면에 남아 있는 동안 대사 페이지가 그 40개 한글 글리프를 계속 보유한다. 보관소와 소지품 초과의 품목 목록은 같은 합성 상태값만 보지 않고 원본 호출자 상태까지 구분하며, 목록 취소 뒤 남는 대사와 고정 라벨도 한 물리 코드 배정을 쓴다. 아이템명 appender가 직접 쓰는 `/` 코드 `AD`는 번역 글리프 배정에서 제외한다. 보관소는 원천의 보관 레코드 `2A`와 찾기 레코드 `2C`를 함께 품목 목록 수명으로 묶고, 판매 시설은 9개 대사 선택 표와 무기점·도구점·비밀상점 선택자에서 27개 대상 레코드를 유도한다. 저장 후속 질문의 대사 레코드 `45`도 공용 `예/아니오` 코드북과 한 수명으로 결속한다. 이 화면의 실제 수요는 대사 글리프 11개와 보존 활성 코드 15개, 합계 `26/210`이다. 품목·유닛·적군·병종 이름은 화면별 우회가 아니라 원본 공용 appender 진입점에서 카탈로그 페이지를 게시한다. 원본 영어 `MAP`, `LV`, `HP`, `STR`, `SKI`, `WLV`, `AGI`, `DEF`, `MOV`, `H.P`, `EXP`, `NEXT STORY`와 숫자는 보존한다. `{EA}`가 화자명 앞에 합성하던 일본어 표식 `9E AB`는 두 빈 타일로 투영한다.

`analyze-translation-coverage`의 현재 모집단은 53개 화면이며 일본어 번역 대상 44개·원문 문자만 보존 5개·텍스트 없음 4개로 나뉜다. 일본어 대상 44개 글꼴 수명을 모두 비교했고 미측정 역할은 0개다. 유닛 선택·게임 속도·보관 행동·소지품 초과 행동·보관 용량 안내도 한 고정 메뉴 가족으로 묶어, 설치된 공용 정적 페이지는 `86/210`, 품목 목록·고정 라벨·대사가 겹치는 공용 선택지 전체의 최대 수요는 `195/210`으로 결속했다. 저장 후속 선택창은 별도 실제 작업집합 `26/210`으로 계수한다. 전역 최악은 저장 질문·전원 종료 안내·중단 메시지의 `209/210`, 별도 그래픽 예산을 쓰는 제목은 `117/121`이다. 현재 번역안은 전체 회귀의 기준안으로 채택했고, 띄어쓰기·오역·용어 점검은 목적별 실행 묶음과 함께 계속한다. 남은 제품 관문은 전체 소비 경로 조사, 동일한 최종 산출물의 대표·최악·패배·저장·엔딩 실행과 배포 판정이다.

직전 `04F36432…` 보고서에서 선언된 실행 역할을 모두 충족한 도메인 **2 / 23**은 현재 산출물에 자동 승계하지 않는다. 현재 `6E7604E2…` 보고서는 최종 실행 manifest를 아직 받지 않았으므로 실행 완료를 승격하지 않는다. 별도 exact-ROM 실행 장부에서는 보관소의 목록·취소·실제 보관과 8장 마르스 패배를 확인했다. 패배 경로는 실제 `턴종료`에서 선택자 `B0:06`을 만들었고, 한글 패배 문구와 계속 선택창, `B`의 제목 복귀와 `A`의 8장 재개가 모두 유지됐다. 같은 ROM에서 시다의 실제 반격 사망과 한글 사망 대사, 7장 완료·저장·8장 도입을 거친 뒤 명단에서 ID `02`가 사라진 것도 한 계보로 확인했다. 이는 영향을 받은 저장소, 게임 오버와 일반 아군 영구 이탈 경로의 회귀이며 자연 전투 보상, 엔딩과 나머지 소비자를 대신하지 않는다.

매퍼 변환은 원본 CHR 래치와 배터리 SRAM을 유지하는 mapper 165를 채택했다. 알려진 직접 writer와 최종 설치는 결속했지만 whole-program 매퍼 쓰기 분모와 exact 통합 실행 회귀가 남아 있어 G2는 다시 진행 중으로 본다. 동적 한글 글꼴도 최종 산출물 실행 결속이 남아 G3를 완료로 세지 않는다. 주 대사 번역 뷰의 의미 있는 일본어 2,548줄은 한국어 초벌로 모두 채워졌고 이 초벌을 전체 회귀의 기준안으로 채택했다. 문장별 검토 상태는 기존 작업 기록으로 보존하며, 띄어쓰기·오역·용어 문제는 실제 화면 문맥과 함께 계속 교정한다. 가장 큰 한글 집합은 `village-and-outro-dialogue:024`의 175자이며, 생산자는 7장 성 좌표 `(27,10)`의 `C0:18`이다. 전체 상주 수요 `275/210` 대신 실제 페이지 최대 `135/210`을 기준으로 완료 페이지마다 공급한다. 과거 전용 단계의 `C8 → CC → D0` 정적 세 묶음은 현재 전역 런타임의 통과 조건이 아니다. 선택자 `C0:18`, 완료 15페이지와 원본 영문 `NEXT STORY` 이탈은 과거 exact 산출물에서 한 실행으로 확인했지만 현재 `04F36432…`에는 승계하지 않았다.

전투는 원본 4 KiB 페이지와 현재 이름·병종·장비·지형·대사 레시피를 런타임에 합성한다. 가능한 모델의 정확 최대는 텍스트 131자·보호 코드 포함 `170/210`이다. 자동 병종 소개의 큰 설치 묶음은 `172/210`, 맵 메뉴 전체 라벨은 `203/210`이다. 저장 질문과 정확한 `B0:01` 전원 종료·중단 문구는 다른 모든 활성 코드를 보존하는 상한에서도 각각 `209/210`이다. `B0:00` 저장 완료 선택지는 같은 상한이 `214/210`으로 넘었으므로, 기존 상태를 8개 불규칙 프레임에서 다시 읽어 대상 셀 밖의 코드 합집합을 결속한 `78/210`을 사용한다. 유닛 UI는 전체 합집합 229자와 요약·상태 공유군 218자가 한 페이지를 넘지만 실제 화면 상한은 요약 36, 상태 30, 명령 30이다. 소지품 사용 결과는 가능한 18개 대사 경로의 최대가 `43/210`이다. 25개 장의 장 제목과 완전한 도입 대사 사슬은 전체 상주 대신 네 줄 완료 페이지마다 재적재하며 최대 `43+101=144/210`이다. 일본어 대상 44개 수명은 모두 각 예산 안에 들어간다. 현재 정확 보고서의 실행 결속은 완전 도메인 기준 `2 / 23`이며, 소비자 전수 조사는 `0 / 23`이다. 나머지 목적별 화면 묶음과 전체 플레이는 아직 남아 있다. 따라서 아직 개발 빌드이며 배포 후보가 아니다. 세부 근거와 남은 작업은 [현재 상태](docs/status.md)를 따른다.

제목 그래픽은 원본 다섯 행의 최대 `27×5` 영역에서 마지막 행의 두 `TM` 셀을 제외한 134셀만 교체한다. 원본 일본어 로고 전용 코드는 121개이고, 원본 화면 기반 ImageGen 로고를 200픽셀 폭으로 결정적으로 줄인 자산은 고유 타일 117개를 쓴다. 완성 연출이 로고 위 `$2182`의 원작 장식 26셀과 칼 교차부 11셀을 별도 전송하므로, 설치기는 전자는 빈 타일로 지우고 후자는 같은 한국어 로고 셀로 다시 고정한다. 과거 누적 후보 `87176c60d9ebb3cfd28e9cc44c58954113863d0e`의 무입력 실행에서 원본 칼, 두 셀 `TM`, `©1990 Nintendo`, 점멸 팔레트와 프레임 1206·1331·1458·1912의 네 위상을 확인했다. 이 증거는 현재 누적 후보 `0b507b7c9d0d97a483a04a27bf2821a82b1a9a14`에 승계하지 않으며, 사람 시각 승인과 패배 뒤 제목 복귀도 별도 관문이다.

```sh
cargo run -p fc-fire-emblem-patch -- verify-source "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-font-supply "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-text-tables "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-dialogue-structure "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-screen-contracts "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-translation-coverage "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-title-logo-asset "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-chapter-transitions "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-temporal-surfaces "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes" evidence/private/temporal-surfaces/manifest.json
cargo run -p fc-fire-emblem-patch -- analyze-chapter-victory "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-item-flow "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-glyph-demand --population "storage-lifetime=shop-and-item-dialogue:041,shop-and-item-dialogue:006" --coresident "storage-screen=storage-lifetime,item-names"
cargo run -p fc-fire-emblem-patch -- extract-main-dialogue-source "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- extract-main-dialogue-workspace "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- validate-main-dialogue-workspace "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-main-dialogue-glyph-workset "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- plan-main-dialogue-reinsertion "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- verify-main-dialogue-source-roundtrip "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-options-poc "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-mapper165-parity-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-mapper165-trigger-planes "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-mapper165-direct-chr-pairs "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- plan-hangul-page-proof "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-mapper165-hangul-page-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-kr-patch "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-main-dialogue-slice-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-battle-composition-loader-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- verify-battle-composition-runtime evidence/private/battle-composition-loader/dynamic-lifetime-compose-return-event.json
cargo run -p fc-fire-emblem-patch -- build-mmc5-prg-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-mmc5-chr-writer-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-mmc5-expanded-chr-options-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- project-mmc4-latch-nametable path/to/nametable.bin --nametable-index 1 --fd-bank 0 --fe-bank 24 --initial-latch fe
cargo run -p fc-fire-emblem-patch -- replay-mmc4-latch-ppu-transfers path/to/ppu-transfers.json
cargo run -p fc-fire-emblem-patch -- build-mmc5-dialogue-exram-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes" out/mmc5-exram-attributes.bin
cargo run -p fc-fire-emblem-patch -- build-mmc5-nametable-shadow-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
```

ROM과 빌드 결과는 저장소에 포함하지 않는다. 지금까지의 판단 흐름은 `docs/decisions.md`, 조사 근거와 현재 상태는 `docs/initial-survey.md`와 `docs/status.md`, MMC4 화면별 공급 근거는 `docs/render-paths.md`, 장 종료부터 다음 장 도입까지의 화면 계약은 `docs/chapter-transitions.md`, 첫 텍스트 모집단은 `docs/text-tables.md`, 전체 한글화의 단계별 통과 조건은 `docs/roadmap.md`, 화면 상주권과 도구 책임의 전수 정리 순서는 `docs/refactoring.md`, 대사 초벌의 작업 순서와 보류·검증 기준은 `docs/dialogue-drafting.md`, 기본 조작과 치트를 포함한 실행 검증 원칙은 `docs/playtesting.md`에 정리한다.

이 저장소는 공개 가능성을 전제로 한다. 추출·재삽입 도구, 주소·해시·소비 경로 같은 구조 근거, 소규모 메뉴·UI 번역은 포함할 수 있다. 대사 중심의 대규모 원문 추출본·번역본·작업 중간 자산은 커밋하지 않으며, 무시되는 `private/dialogue/`, `out/` 또는 `evidence/private/`에서만 다룬다.
