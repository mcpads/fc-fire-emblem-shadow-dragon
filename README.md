# FC Fire Emblem Korean patch

패미컴판 《파이어 엠블렘 암흑룡과 빛의 검》의 한글화 프로젝트다.

## 번역 범위

- 지원 원본은 SHA-1 `0179c550d424e0397496078789e7b116601d120c`인 일본판이다.
- **일본어만 한국어로 번역한다.** 일본판에 원래 들어 있는 영어, 숫자, 로마자 약어는 번역하거나 글꼴 슬롯을 덮지 않는다.
- 영문 패치는 주소와 자료 구조를 교차 확인하는 조사 자료일 뿐, 번역 원문이나 제품 빌드 입력이 아니다.

## 현재 게이트

`build-kr-patch`는 지원 일본판 원본에서 검증한 표면을 순서대로 다시 만드는 10단계 누적 개발 ROM을 낸다. 설정·부대 목록, 1·2장 도입부, 시작·기록 메뉴, 아군명, 자동 병종 소개, 무기점, 전투에 이어 7장 `C0:18` 최대 대사 15페이지를 세 글꼴 묶음으로 설치한다. 현재 출력 SHA-1은 `17911417a91fc8190e68d5e032107e97ca939dfe`이며 42대사 레코드·168번역 줄을 포함한다. 원본 영어 `MAP`, `LV`, `HP`, `STR`, `SKI`, `WLV`, `AGI`, `DEF`, `MOV`, `H.P`, `EXP`, `NEXT STORY`, 숫자와 `※`는 보존한다.

`analyze-translation-coverage`는 45개 화면을 일본어 대상 36개·원문 문자만 보존 5개·텍스트 없음 4개로 나누고, 일본어 대상 전부를 22개 번역 도메인에 연결한다. 22개 도메인은 모두 원천과 한국어 입력에 결속됐지만 모든 소비자에 설치된 도메인은 8개다. 현재 일본어 대상 수명 36개 중 24개를 계수했고 자동 병종 소개가 173/210칸으로 가장 크며 12개는 미계수다. 미계수 수명이 남아 있으므로 전역 최악 조건과 전체 한글화 완료는 아직 확정하지 않는다.

설정 항목 `サウンド`, `アニメーション`, `ウエイトタイマー`를 각각 `사운드`, `애니메이션`, `대기시간`으로 바꾸는 페이지 전환 PoC를 제공한다. 부대 목록의 기존 A/B 증명 페이지는 실제 아군명 페이지로 교체했고, 한글 `이름`·아군명과 원본 `LV`·`HP`·숫자를 함께 표시한다. 맵 유닛 요약·상태도 같은 의미 번역 자산에서 별도 코드북을 만들어 이름만 한글로 표시하고 아직 미설치인 일본어 병종·아이템·능력치 라벨은 원형을 유지한다. 자동 병종 소개는 216개 한글 합집합을 프로필 인덱스 11에서 두 페이지로 나누고, 원본 영문 능력치 표와 주변 그래픽을 유지한다. 동적 글꼴 공급 관문 G3는 통과했지만 아직 정식 패치나 배포 후보는 아니다.

매퍼 변환은 원본 CHR 래치와 배터리 SRAM을 유지하는 mapper 165를 채택했고 G2·G3를 통과했다. 주 대사 번역 뷰의 의미 있는 일본어 2,541줄은 검토 대기 한국어로 모두 채워졌지만 사람 승인 완료는 아직 0줄이다. 가장 큰 한글 집합은 `village-and-outro-dialogue:024`의 175자이며, 생산자는 7장 성 좌표 `(27,10)`의 `C0:18`이다. 전체 상주 수요 `275/210` 대신 실제 페이지 최대 `135/210`을 기준으로 15페이지를 세 묶음에 배치했다. 상태를 연결한 7장 성 명령 경로에서 초기 선택기, `C8 → CC → D0` 재적재, 모든 페이지의 6개 점멸 위상, 마지막 원본 영문 `NEXT STORY` 이탈을 현재 출력에 결속했다.

전투는 원본 4 KiB 페이지와 현재 이름·병종·장비·지형·대사 레시피를 런타임에 합성한다. 가능한 모델의 정확 최대는 텍스트 131자·보호 코드 포함 `170/210`이다. 자동 병종 소개는 이미 설치된 두 글꼴 묶음 중 큰 쪽의 한글 161자와 보존 코드 12개를 합친 `173/210`으로 현재까지 측정한 화면 중 가장 크다. 유닛 UI는 전체 합집합 229자와 요약·상태 공유군 218자가 한 페이지를 넘지만 실제 화면 상한은 요약 36, 상태 30, 명령 30이다. 소지품 사용 결과는 가능한 18개 대사 경로의 최대가 `43/210`이며, 승급 성공은 기존 전투 수명으로 옮겨 가고 대지의 오브 결과는 사용 수명에 남는다. 따라서 요약·상태는 현재 내용에 맞춘 페이지를 공유하고 명령은 별도 정적 페이지를 쓰는 방향이다. 다음 구조 관문은 미계수 일본어 화면 12개를 같은 기준으로 비교하는 것이다. 그 뒤 나머지 번역을 누적 설치하고 대상 일본어 0건, 보호 영어·숫자·그래픽 손상 0건을 최종 동일 ROM에서 검증한다. 현재 산출물은 개발 빌드이며 배포 후보가 아니다. 세부 근거와 남은 작업은 [현재 상태](docs/status.md)를 따른다.

```sh
cargo run -p fc-fire-emblem-patch -- verify-source "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-font-supply "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-text-tables "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-dialogue-structure "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-screen-contracts "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-translation-coverage "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
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
