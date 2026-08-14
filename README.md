# FC Fire Emblem Korean patch

패미컴판 《파이어 엠블렘 암흑룡과 빛의 검》의 한글화 프로젝트다.

## 번역 범위

- 지원 원본은 SHA-1 `0179c550d424e0397496078789e7b116601d120c`인 일본판이다.
- **일본어만 한국어로 번역한다.** 일본판에 원래 들어 있는 영어, 숫자, 로마자 약어는 번역하거나 글꼴 슬롯을 덮지 않는다.
- 영문 패치는 주소와 자료 구조를 교차 확인하는 조사 자료일 뿐, 번역 원문이나 제품 빌드 입력이 아니다.

## 현재 게이트

`plan-full-translation-installation`은 지원 일본판 원본과 누적 후보를 결속해 선언된 13개 번역 도메인, 504개 대사 레코드, 928개 페이지 작업집합을 하나의 통합 개발 ROM에 설치한다. 현재 정확 출력 SHA-1은 `5fda755073d41b56b1c9b695374151703a22a5f4`, SHA-256은 `3ab4eb7e2adabd22bfbed7271bc9a7d3a061a06da2c3cac312ded91af0328729`다. 이 13개 도메인의 기술 설치는 통과했지만 22개 번역 도메인의 소비 경로 전수 조사가 끝났다는 뜻은 아니다. 소스에 결속한 알려진 경로는 **4 / 22**이고, 전수 조사까지 완료한 도메인은 **0 / 22**다. `terrain_names`도 확인한 `battle_animation` 경로만 결속했으며, 나머지 **18 / 22**는 알려진 경로도 미해결이다. 22개 모두 전수 조사를 계속하며 이 수치는 기술 설치·실행 검증·사람 검토를 대신하지 않는다. 원본 영어 `MAP`, `LV`, `HP`, `STR`, `SKI`, `WLV`, `AGI`, `DEF`, `MOV`, `H.P`, `EXP`, `NEXT STORY`, 숫자와 `※`는 보존한다.

`analyze-translation-coverage`의 현재 모집단은 46개 화면이며 일본어 번역 대상 37개·원문 문자만 보존 5개·텍스트 없음 4개로 나뉜다. 글꼴 화면의 전역 최악은 저장 질문·전원 종료 안내·중단 메시지의 `209/210`, 별도 그래픽 예산을 쓰는 제목은 `117/121`이다. 선언 범위의 수용량과 기술 설치는 닫혔지만, 전체 소비 경로 조사와 동일한 최종 산출물의 대표·최악·패배·저장·엔딩 실행, 사람 검토, 배포 판정은 각각 남아 있다.

현재 정확 산출물에는 `unit_summary`, `unit_status`, `map_menu`, `unit_roster`, `unit_command_menu`, `item_inventory_list`, `item_action_menu`, `battle_animation` 여덟 대표 역할의 실행 증거가 결속돼 있다. 지도·부대·유닛·소지품 화면의 한글 라벨·이름·아이템과 원본 `LV`·`HP`·숫자, 전투 지형 효과 창의 한글과 원본 `HIT`를 함께 확인했다. 콜드 실행 3회의 32개 표본이 확인한 변형만 닫은 것이며, 나머지 대표·최악 변형과 전체 플레이를 대신하지 않는다.

매퍼 변환은 원본 CHR 래치와 배터리 SRAM을 유지하는 mapper 165를 채택했고 G2·G3를 통과했다. 주 대사 번역 뷰의 의미 있는 일본어 2,548줄은 검토 대기 한국어로 모두 채워졌지만 사람 승인 완료는 아직 0줄이다. 가장 큰 한글 집합은 `village-and-outro-dialogue:024`의 175자이며, 생산자는 7장 성 좌표 `(27,10)`의 `C0:18`이다. 전체 상주 수요 `275/210` 대신 실제 페이지 최대 `135/210`을 기준으로 완료 페이지마다 공급한다. 과거 전용 단계의 `C8 → CC → D0` 정적 세 묶음은 현재 전역 런타임의 통과 조건이 아니다. 현재 통합 출력에는 선택자 `C0:18`과 페이지 진행 구조가 기술적으로 설치돼 있지만, 15페이지와 원본 영문 `NEXT STORY`의 과거 실행 증거는 현재 산출물에 승계하지 않았다.

전투는 원본 4 KiB 페이지와 현재 이름·병종·장비·지형·대사 레시피를 런타임에 합성한다. 가능한 모델의 정확 최대는 텍스트 131자·보호 코드 포함 `170/210`이다. 자동 병종 소개의 큰 설치 묶음은 `172/210`, 맵 메뉴 전체 라벨은 `203/210`이다. 저장 질문과 정확한 `B0:01` 전원 종료·중단 문구는 다른 모든 활성 코드를 보존하는 상한에서도 각각 `209/210`이다. `B0:00` 저장 완료 선택지는 같은 상한이 `214/210`으로 넘었으므로, 기존 상태를 8개 불규칙 프레임에서 다시 읽어 대상 셀 밖의 코드 합집합을 결속한 `78/210`을 사용한다. 유닛 UI는 전체 합집합 229자와 요약·상태 공유군 218자가 한 페이지를 넘지만 실제 화면 상한은 요약 36, 상태 30, 명령 30이다. 소지품 사용 결과는 가능한 18개 대사 경로의 최대가 `43/210`이다. 25개 장의 장 제목과 완전한 도입 대사 사슬은 전체 상주 대신 네 줄 완료 페이지마다 재적재하며 최대 `43+101=144/210`이다. 전체 37개 수명은 각 예산 안에 들어간다. 현재 정확 산출물에 결속된 실행 증거는 위 여덟 대표 역할뿐이고 최악 역할은 아직 0개다. 과거 산출물에서 확인한 대표 주 대사와 7장 최장 대사 경로는 현재 산출물의 증거로 승계하지 않는다. 따라서 아직 개발 빌드이며 배포 후보가 아니다. 세부 근거와 남은 작업은 [현재 상태](docs/status.md)를 따른다.

제목 그래픽은 원본 다섯 행의 최대 `27×5` 영역에서 마지막 행의 두 `TM` 셀을 제외한 134셀만 교체한다. 원본 일본어 로고 전용 코드는 121개이고, 원본 화면 기반 ImageGen 로고를 200픽셀 폭으로 결정적으로 줄인 자산은 고유 타일 117개를 쓴다. 완성 연출이 로고 위 `$2182`의 원작 장식 26셀과 칼 교차부 11셀을 별도 전송하므로, 설치기는 전자는 빈 타일로 지우고 후자는 같은 한국어 로고 셀로 다시 고정한다. 현재 누적 후보 `87176c60d9ebb3cfd28e9cc44c58954113863d0e`의 새 무입력 실행에서 원본 칼, 두 셀 `TM`, `©1990 Nintendo`, 점멸 팔레트와 프레임 1206·1331·1458·1912의 네 위상을 다시 확인했다. 누적 빌드 보고서는 실행 증거 유예 상태이므로 이 실행을 아직 런타임 완료 역할로 세지 않으며, 사람 시각 승인과 패배 뒤 제목 복귀도 별도 관문이다.

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

ROM과 빌드 결과는 저장소에 포함하지 않는다. 지금까지의 판단 흐름은 `docs/decisions.md`, 조사 근거와 현재 상태는 `docs/initial-survey.md`와 `docs/status.md`, MMC4 화면별 공급 근거는 `docs/render-paths.md`, 장 종료부터 다음 장 도입까지의 화면 계약은 `docs/chapter-transitions.md`, 첫 텍스트 모집단은 `docs/text-tables.md`, 전체 한글화의 단계별 통과 조건은 `docs/roadmap.md`, 대사 초벌의 작업 순서와 보류·검증 기준은 `docs/dialogue-drafting.md`, 기본 조작과 치트를 포함한 실행 검증 원칙은 `docs/playtesting.md`에 정리한다.

이 저장소는 공개 가능성을 전제로 한다. 추출·재삽입 도구, 주소·해시·소비 경로 같은 구조 근거, 소규모 메뉴·UI 번역은 포함할 수 있다. 대사 중심의 대규모 원문 추출본·번역본·작업 중간 자산은 커밋하지 않으며, 무시되는 `private/dialogue/`, `out/` 또는 `evidence/private/`에서만 다룬다.
