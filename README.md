# FC Fire Emblem Korean patch

패미컴판 《파이어 엠블렘 암흑룡과 빛의 검》의 한글화 프로젝트다.

## 번역 범위

- 지원 원본은 SHA-1 `0179c550d424e0397496078789e7b116601d120c`인 일본판이다.
- **일본어만 한국어로 번역한다.** 일본판에 원래 들어 있는 영어, 숫자, 로마자 약어는 번역하거나 글꼴 슬롯을 덮지 않는다.
- 영문 패치는 주소와 자료 구조를 교차 확인하는 조사 자료일 뿐, 번역 원문이나 제품 빌드 입력이 아니다.

## 현재 게이트

`plan-full-translation-installation`은 지원 일본판 원본과 누적 후보를 결속해 13개 필수 번역 도메인, 504개 대사 레코드, 928개 페이지 작업집합을 하나의 통합 개발 ROM에 설치한다. 현재 정확 출력 SHA-1은 `5e8eaaf096dc5a18e6501be1007064f1831d7a62`, SHA-256은 `332a1c1931509e778ae22f2a7921c6d2b3dd7ec3b312b2e59396c877b74e9458`다. 정적 소비자 결속은 **13 / 13**이며 원본 영어 `MAP`, `LV`, `HP`, `STR`, `SKI`, `WLV`, `AGI`, `DEF`, `MOV`, `H.P`, `EXP`, `NEXT STORY`, 숫자와 `※`는 보존한다.

`analyze-translation-coverage`는 45개 화면을 일본어 대상 36개·원문 문자만 보존 5개·텍스트 없음 4개로 나누고 일본어 대상 전부를 번역 도메인에 연결한다. 글꼴 화면의 전역 최악은 저장 질문·전원 종료 안내·중단 메시지의 `209/210`, 별도 그래픽 예산을 쓰는 제목은 `117/121`이다. 수용량과 정적 설치 관문은 닫혔지만, 동일한 최종 산출물에서 대표·최악·패배·저장·엔딩 경로를 다시 실행하고 일본어 0건과 보호 원문·그래픽 손상 0건을 검증하는 일은 남아 있다.

설정 항목 `サウンド`, `アニメーション`, `ウエイトタイマー`를 각각 `사운드`, `애니메이션`, `대기시간`으로 바꾸는 페이지 전환 PoC를 제공한다. 부대 목록의 기존 A/B 증명 페이지는 실제 아군명 페이지로 교체했고, 한글 `이름`·아군명과 원본 `LV`·`HP`·숫자를 함께 표시한다. 맵 유닛 요약·상태도 같은 의미 번역 자산에서 별도 코드북을 만들어 이름만 한글로 표시하고 아직 미설치인 일본어 병종·아이템·능력치 라벨은 원형을 유지한다. 자동 병종 소개는 216개 한글 합집합을 프로필 인덱스 11에서 두 페이지로 나누고, 원본 영문 능력치 표와 주변 그래픽을 유지한다. 동적 글꼴 공급 관문 G3는 통과했지만 아직 정식 패치나 배포 후보는 아니다.

매퍼 변환은 원본 CHR 래치와 배터리 SRAM을 유지하는 mapper 165를 채택했고 G2·G3를 통과했다. 주 대사 번역 뷰의 의미 있는 일본어 2,541줄은 검토 대기 한국어로 모두 채워졌지만 사람 승인 완료는 아직 0줄이다. 가장 큰 한글 집합은 `village-and-outro-dialogue:024`의 175자이며, 생산자는 7장 성 좌표 `(27,10)`의 `C0:18`이다. 전체 상주 수요 `275/210` 대신 실제 페이지 최대 `135/210`을 기준으로 완료 페이지마다 공급한다. 과거 전용 단계의 `C8 → CC → D0` 정적 세 묶음은 현재 전역 런타임의 통과 조건이 아니다. 정확 통합 출력에서는 게임이 직렬화한 중단 SaveRAM을 현재 ROM reset 뒤 재개하고 실제 턴 종료와 성 명령을 거쳐, 선택자 `C0:18`·완료 상태 `0E`에서 가시 페이지 `0..14`가 모두 전진하고 마지막 원본 영문 `NEXT STORY`로 이탈함을 결속했다.

전투는 원본 4 KiB 페이지와 현재 이름·병종·장비·지형·대사 레시피를 런타임에 합성한다. 가능한 모델의 정확 최대는 텍스트 131자·보호 코드 포함 `170/210`이다. 자동 병종 소개의 큰 설치 묶음은 `173/210`, 맵 메뉴 전체 라벨은 `203/210`이다. 저장 질문과 정확한 `B0:01` 전원 종료·중단 문구는 다른 모든 활성 코드를 보존하는 상한에서도 각각 `209/210`이다. `B0:00` 저장 완료 선택지는 같은 상한이 `214/210`으로 넘었으므로, 기존 상태를 8개 불규칙 프레임에서 다시 읽어 대상 셀 밖의 코드 합집합을 결속한 `78/210`을 사용한다. 유닛 UI는 전체 합집합 229자와 요약·상태 공유군 218자가 한 페이지를 넘지만 실제 화면 상한은 요약 36, 상태 30, 명령 30이다. 소지품 사용 결과는 가능한 18개 대사 경로의 최대가 `43/210`이다. 25개 장의 장 제목과 완전한 도입 대사 사슬은 전체 상주 대신 네 줄 완료 페이지마다 재적재하며 최대 `43+100=143/210`이다. 전체 36개 수명은 각 예산 안에 들어간다. 현재 통합 출력은 대표 주 대사의 서로 다른 페이지·다음 레코드·맵 이탈과 7장 최악 대사의 15페이지·`NEXT STORY` 이탈을 통과했다. 반면 같은 실행에서 맵 진행 메뉴와 유닛·성 명령 라벨의 코드북 가블을 확인했으므로 아직 개발 빌드이며 배포 후보가 아니다. 세부 근거와 남은 작업은 [현재 상태](docs/status.md)를 따른다.

제목 그래픽은 원본 다섯 행의 최대 `27×5` 영역에서 마지막 행의 두 `TM` 셀을 제외한 134셀만 교체한다. 원본 일본어 로고 전용 코드는 121개이고, 원본 화면 기반 ImageGen 로고를 200픽셀 폭으로 결정적으로 줄인 자산은 고유 타일 117개를 쓴다. 완성 연출이 로고 위 `$2182`의 원작 장식 26셀과 칼 교차부 11셀을 별도 전송하므로, 설치기는 전자는 빈 타일로 지우고 후자는 같은 한국어 로고 셀로 다시 고정한다. 누적 ROM은 원본 칼, 두 셀 `TM`, `©1990 Nintendo`와 점멸 팔레트를 보존하며 프레임 1206·1331·1458의 초기·완성·저작권 위상과 1912의 자동 병종 소개 전환을 정확한 출력 해시에 결속했다. 사람 시각 승인과 패배 뒤 제목 복귀는 아직 별도 관문이다.

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
