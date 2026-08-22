# 빌드 파이프라인

이 문서는 반복 제품 빌드와 필요할 때만 다시 여는 조사 명령을 구분한다. 현재 산출물 해시는 `status.md`, 구조 소유권은 `refactoring.md`를 따른다.

## 전제

지원 원본은 SHA-1 `0179c550d424e0397496078789e7b116601d120c`인 일본판이다.

```sh
ROM="roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- verify-source "$ROM"
```

제품 빌드는 다음 입력을 읽는다.

- 공개 소규모 번역: `assets/translation/`
- 비공개 주·전투 대사와 전투 고정문: `private/`
- 채택된 화면·시간축 근거: `evidence/private/`
- 결정적으로 만든 제목 로고 자산: `out/title-logo.asset`

원본 ROM, 생성 ROM, 대규모 대사 작업공간, SaveRAM과 캡처는 Git 입력이 아니다.

## 반복 제품 빌드

번역·고정 에셋이나 제품 코드가 바뀔 때 실행하는 기본 경로다.

```sh
cargo fmt --all -- --check
cargo check -p fc-fire-emblem-patch
cargo clippy -p fc-fire-emblem-patch --all-targets -- -D warnings
cargo test -p fc-fire-emblem-patch --no-fail-fast

cargo run -p fc-fire-emblem-patch -- build-kr-patch "$ROM" \
  --defer-runtime-evidence

cargo run -p fc-fire-emblem-patch -- plan-full-translation-installation "$ROM" \
  --output out/fire-emblem-fe1-korean-integrated.nes
```

기본 산출물은 다음과 같다.

- `out/fire-emblem-fe1-korean-patch.nes`
- `out/kr-patch-build.json`
- `out/fire-emblem-fe1-korean-integrated.nes`
- `out/full-translation-installation.json`

`build-kr-patch --defer-runtime-evidence`는 exact 출력이 바뀌는 개발 중에 과거 ROM용 실행 manifest를 현재 성공으로 잘못 승계하지 않는다. 클래스·상점·최대 대사·제목의 exact 누적 실행 manifest를 현재 산출물에 결속할 때는 이 플래그를 빼고 해당 `--*-runtime-evidence` 경로를 넘긴다.

`plan-full-translation-installation`은 `--output`이 없으면 보고서만 만든다. ROM을 방출할 때는 `--output`을 명시한다. `--transport-probe`는 과거 호환 별칭일 뿐 새 자동화에서는 쓰지 않는다.

## 실행 증거를 결속한 최종 후보

에뮬레이터 실행은 먼저 개발 ROM에서 결함을 찾고, 후보가 고정된 뒤 exact ROM 해시를 가진 manifest로 결속한다.

```sh
cargo run -p fc-fire-emblem-patch -- plan-full-translation-installation "$ROM" \
  --output out/fire-emblem-fe1-korean-integrated.nes \
  --final-runtime-evidence evidence/private/final-runtime/manifest.json
```

manifest는 적어도 ROM 해시, 콜드 부팅 또는 SaveRAM 파일 계보, 입력 순서, 화면·메모리 체크포인트와 이탈을 포함해야 한다. 다른 ROM의 성공은 현재 보고서에 자동 승계하지 않는다.

## 조사와 제품 빌드의 분리

확정 에셋을 설치할 때 원본 매퍼 분석, 포인터 표 조사, 전투 표면 추출과 역사적 프로브를 다시 실행하지 않는다. 다음 중 하나일 때만 해당 조사를 다시 연다.

- 지원 원본 또는 원본 ABI 가정이 바뀐다.
- 매퍼·런타임 코드나 저장소 소유 범위가 바뀐다.
- 동적 반례가 기존 생산자·소비자 분모의 누락을 보인다.
- 채택된 보고서가 현재 입력과 더 이상 결속되지 않는다.

대표 조사 명령은 다음과 같다.

```sh
cargo run -p fc-fire-emblem-patch -- analyze-screen-contracts "$ROM"
cargo run -p fc-fire-emblem-patch -- analyze-dialogue-structure "$ROM"
cargo run -p fc-fire-emblem-patch -- analyze-translation-coverage "$ROM"
cargo run -p fc-fire-emblem-patch -- analyze-chapter-transitions "$ROM"
cargo run -p fc-fire-emblem-patch -- analyze-item-flow "$ROM"
cargo run -p fc-fire-emblem-patch -- analyze-shop-flow "$ROM"
cargo run -p fc-fire-emblem-patch -- analyze-battle-codebook-plan "$ROM"
cargo run -p fc-fire-emblem-patch -- analyze-battle-surface-constraints "$ROM"
cargo run -p fc-fire-emblem-patch -- analyze-temporal-surfaces "$ROM" \
  evidence/private/temporal-surfaces/manifest.json
```

전체 명령과 인자는 `cargo run -p fc-fire-emblem-patch -- --help`와 각 하위 명령의 `--help`가 권위다. 문서는 명령 목록을 복제하지 않는다.

## 프로브

프로브는 조사·회귀 진단 산출물이며 제품 빌드의 선행 단계가 아니다. 채택된 구현은 역할 기반 제품 모듈에 있어야 하고, 프로브 CLI는 그 API를 호출하는 얇은 어댑터만 둔다.

현재 남긴 대표 진단은 다음과 같다.

- mapper 165 무번역 패리티
- 전투 조합 런타임의 독립 CHR-RAM 재합성 비교
- 주·전투 대사의 제한된 end-to-end slice
- MMC4/MMC5 대안 조사

한 전투 조합을 고정하던 `build-battle-combination-probe`와 옛 고정 캐시 업로드 `build-battle-cache-upload-probe`는 현재 제품 구조와 겹쳐 제거했다.

## Rust와 Python

Rust가 원본 결속, 인코딩, 페이지 계획, typed ISA, ROM 쓰기와 합격 판정을 소유한다. Python은 JSON 기반 비공개 집계나 선택적 후보 계산만 할 수 있다. Python이 ROM을 고치거나 주소·페이지 정책의 단일 출처가 되어서는 안 된다.

## 재현성과 출력 정리

같은 원본과 입력은 같은 ROM과 보고서 바이트를 만들어야 한다. 구조만 옮긴 리팩터링은 누적·통합 ROM의 byte-identical 비교를 통과해야 한다. 보고서 스키마나 문구만 의도적으로 바꾸었다면 ROM 동일성과 보고서 변화 이유를 분리해 기록한다.

`out/cumulative-stages/`는 빌드 내부 단계이고 배포 입력이 아니다. 제품 확인에 필요한 최종 ROM·보고서와 현재 비교 자료만 남기며, 오래된 ROM을 실행 증거의 묵시적 기준선으로 사용하지 않는다.
