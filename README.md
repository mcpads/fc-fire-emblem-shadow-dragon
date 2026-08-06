# FC Fire Emblem Korean patch

패미컴판 《파이어 엠블렘 암흑룡과 빛의 검》의 한글화 프로젝트다.

## 번역 범위

- 지원 원본은 SHA-1 `0179c550d424e0397496078789e7b116601d120c`인 일본판이다.
- **일본어만 한국어로 번역한다.** 일본판에 원래 들어 있는 영어, 숫자, 로마자 약어는 번역하거나 글꼴 슬롯을 덮지 않는다.
- 영문 패치는 주소와 자료 구조를 교차 확인하는 조사 자료일 뿐, 번역 원문이나 제품 빌드 입력이 아니다.

## 현재 게이트

설정 항목 `サウンド`, `アニメーション`, `ウエイトタイマー`를 각각 `사운드`, `애니메이션`, `대기시간`으로 바꾸는 기술 PoC를 제공한다. Mesen의 실제 설정 화면에서 세 한글 항목과 기존 영어 능력치 약어가 함께 유지되는 것을 확인했다. 일본어 설정 문자열에 쓰인 가나 타일만 임시로 한글 타일로 바꾸므로, 다른 일본어 화면이 깨질 수 있다. 정식 패치나 배포 후보가 아니다.

현재 매퍼 변환의 우선 후보는 MMC2식 CHR 래치와 MMC3식 PRG·SRAM을 함께 제공하는 mapper 165다. 무번역 프로브는 원본 CHR을 보존한 채 재배치하고, 관측한 FD/FE 쌍용 변형 페이지 1개를 자동 생성한다. 타이틀·자동 능력치·1장 인트로 대화 표본은 원본 화면과 정확히 같아졌으며, 직접 writer와 저장·불러오기·전체 진행 검증이 남아 있다. 정식 패치나 배포 후보가 아니다.

```sh
cargo run -p fc-fire-emblem-patch -- verify-source "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-font-supply "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-text-tables "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-dialogue-structure "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- extract-main-dialogue-source "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- extract-main-dialogue-workspace "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- validate-main-dialogue-workspace "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- plan-main-dialogue-reinsertion "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- verify-main-dialogue-source-roundtrip "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-options-poc "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-mapper165-parity-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-mapper165-trigger-planes "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-mmc5-prg-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-mmc5-chr-writer-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-mmc5-expanded-chr-options-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- project-mmc4-latch-nametable path/to/nametable.bin --nametable-index 1 --fd-bank 0 --fe-bank 24 --initial-latch fe
cargo run -p fc-fire-emblem-patch -- replay-mmc4-latch-ppu-transfers path/to/ppu-transfers.json
cargo run -p fc-fire-emblem-patch -- build-mmc5-dialogue-exram-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes" out/mmc5-exram-attributes.bin
cargo run -p fc-fire-emblem-patch -- build-mmc5-nametable-shadow-probe "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
```

ROM과 빌드 결과는 저장소에 포함하지 않는다. 지금까지의 판단 흐름은 `docs/decisions.md`, 조사 근거와 현재 상태는 `docs/initial-survey.md`와 `docs/status.md`, MMC4 화면별 공급 근거는 `docs/render-paths.md`, 첫 텍스트 모집단은 `docs/text-tables.md`, 전체 한글화의 단계별 통과 조건은 `docs/roadmap.md`, 기본 조작과 치트를 포함한 실행 검증 원칙은 `docs/playtesting.md`에 정리한다.

이 저장소는 공개 가능성을 전제로 한다. 추출·재삽입 도구, 주소·해시·소비 경로 같은 구조 근거, 소규모 메뉴·UI 번역은 포함할 수 있다. 대사 중심의 대규모 원문 추출본·번역본·작업 중간 자산은 커밋하지 않으며, 무시되는 `private/dialogue/`, `out/` 또는 `evidence/private/`에서만 다룬다.
