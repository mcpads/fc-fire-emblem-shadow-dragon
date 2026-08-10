# FC Fire Emblem Korean patch

패미컴판 《파이어 엠블렘 암흑룡과 빛의 검》의 한글화 프로젝트다.

## 번역 범위

- 지원 원본은 SHA-1 `0179c550d424e0397496078789e7b116601d120c`인 일본판이다.
- **일본어만 한국어로 번역한다.** 일본판에 원래 들어 있는 영어, 숫자, 로마자 약어는 번역하거나 글꼴 슬롯을 덮지 않는다.
- 영문 패치는 주소와 자료 구조를 교차 확인하는 조사 자료일 뿐, 번역 원문이나 제품 빌드 입력이 아니다.

## 현재 게이트

`build-kr-patch`는 지원 일본판 원본에서 mapper 165 설정·부대 목록, 1·2장 도입부, 시작·기록 메뉴, 아군명 52개와 자동 병종 소개 22개를 순서대로 다시 만들어 하나의 누적 개발 ROM을 낸다. 주 대사 번역 입력은 의미 있는 일본어 2,541줄이 모두 채워졌지만 사람 승인 완료는 0줄이며, 현재 설치 범위는 5레코드·29줄이다. 자동 병종 소개는 일본어 제목 22개와 설명 75줄을 모두 설치하고 첫·중간·페이지 경계·마지막 소개와 자동 이탈을 무입력 실행으로 확인했다. 원본 영어 `MAP`, `LV`, `HP`, `STR`, `SKI`, `WLV`, `AGI`, `DEF`, `MOV`, `H.P`, `EXP`, 숫자와 `※`는 보존한다. 전투·엔딩의 이름 소비자는 각 화면 글꼴 수명을 붙일 때 같은 번역 자산을 별도 투영한다.

설정 항목 `サウンド`, `アニメーション`, `ウエイトタイマー`를 각각 `사운드`, `애니메이션`, `대기시간`으로 바꾸는 페이지 전환 PoC를 제공한다. 부대 목록의 기존 A/B 증명 페이지는 실제 아군명 페이지로 교체했고, 한글 `이름`·아군명과 원본 `LV`·`HP`·숫자를 함께 표시한다. 맵 유닛 요약·상태도 같은 의미 번역 자산에서 별도 코드북을 만들어 이름만 한글로 표시하고 아직 미설치인 일본어 병종·아이템·능력치 라벨은 원형을 유지한다. 자동 병종 소개는 216개 한글 합집합을 프로필 인덱스 11에서 두 페이지로 나누고, 원본 영문 능력치 표와 주변 그래픽을 유지한다. 동적 글꼴 공급 관문 G3는 통과했지만 아직 정식 패치나 배포 후보는 아니다.

매퍼 변환은 MMC2식 CHR 래치와 MMC3식 PRG·SRAM을 함께 제공하는 mapper 165를 채택했다. 무번역 프로브는 원본 CHR을 보존한 채 재배치하고, 관측한 FD/FE 쌍용 변형 페이지 1개를 자동 생성한다. 타이틀·자동 병종 설명·1장 인트로 대화·전투·게임 오버·중단 저장과 재개·1장 완료 저장·콜드 로드·2장 전환 표본이 원본과 동등해 G2를 통과했다. 화면 계약 보고서는 실제 화면 역할 45개를 모두 실행 관측했고 42개는 CHR 쌍까지 결속했다. 실제 이동·턴 종료·전투와 `しろ` 선택으로 11장 종료 대사, 원본 영어 `NEXT STORY`, 저장 제안, 저장 완료, 검은 자동 전환, 12장 도입까지 연속 관측했다. 사운드 테스트 공용 전투 11개, 자동 엔딩 18개, 턴 경계 게임 오버 12개, 적 선공 정규 전투 11개, 플레이어 선공 정규 전투 10개 시간 표본으로 패배와 전투 경로 극성을 닫았다. 인물 후일담은 112개 보이는 엔트리의 560개 불규칙 표본과 선택자 이벤트로 13개 CHR 쌍 및 직접·라우팅 제어 흐름을 닫았다. 현재 주 대사 번역 뷰 2,812줄에서 의미 있는 일본어 2,541줄을 검토 대기 한국어로 모두 채웠고, 비소비 원문 잔편 1줄은 명시적으로 보존한다. 채워진 고유 한글은 697자이고 명시적인 `E4`/`E6` 대사 전이 사슬은 최대 175자로 활성 슬롯 210칸에 들어가며, 논리 재삽입 계획도 11개 소유 구간 안에서 성립한다. 누적 ROM은 원본 CHR을 건드리지 않고 1·2장 도입부에 서로 다른 확장 페이지를 공급한다. 이는 도입 화면군 두 변형의 누적 수직 슬라이스일 뿐 1장 전체나 정식 패치·배포 후보를 뜻하지 않는다.

전투는 원본 4 KiB 페이지와 현재 이름·병종·장비·지형·대사 레시피를 런타임에 합성하는 제한 로더까지 구현했다. 관측한 다섯 조합 중 한 게임플레이 전투에서 독립 재구성 페이지와 실제 CHR-RAM이 4,096바이트 모두 일치했고 한글, 원본 `LV`·`HIT`·숫자와 자동 지도 복귀를 확인했다. 나머지 조합과 전체 시각 변형은 아직 미검증이다. 목표 밖 일본어·그래픽이 깨지는 기존 개발 프로브도 남아 있으므로, 현재 산출물은 게임 전반의 표시 완성도를 주장하지 않는다. 1장 전체에서 번역한 화면·미번역 일본어·보호 영어와 그래픽의 깨짐 0건을 닫기 전까지 배포 후보가 아니다.

```sh
cargo run -p fc-fire-emblem-patch -- verify-source "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-font-supply "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-text-tables "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-dialogue-structure "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-screen-contracts "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
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
cargo run -p fc-fire-emblem-patch -- verify-battle-composition-runtime evidence/private/battle-composition-loader/participant-fixed-compose-return-event.json --dialogue-selector 62
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
