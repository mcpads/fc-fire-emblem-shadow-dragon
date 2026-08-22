# FC Fire Emblem Korean patch

패미컴판 《파이어 엠블렘 암흑룡과 빛의 검》의 한글화 프로젝트다.

## 범위

- 지원 원본은 SHA-1 `0179c550d424e0397496078789e7b116601d120c`인 일본판이다.
- 일본어만 한국어로 번역한다. 원본 영어, 숫자와 로마자 약어는 보존한다.
- 영문 패치는 주소와 자료 구조를 교차 확인하는 조사 자료일 뿐 번역 원문이나 제품 입력이 아니다.
- 현재 산출물은 개발 ROM이다. 정적 설치 성공이나 일부 화면 성공을 배포 완료로 세지 않는다.

현재 exact 기준선과 바로 다음 관문은 [현재 상태](docs/status.md), 남은 제품 블로커는 [열려 있는 문제](docs/open-problems.md)를 따른다.

## 기본 검증과 빌드

```sh
cargo test -p fc-fire-emblem-patch --no-fail-fast
cargo run -p fc-fire-emblem-patch -- verify-source "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-kr-patch \
  "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes" \
  --defer-runtime-evidence
cargo run -p fc-fire-emblem-patch -- plan-full-translation-installation \
  "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes" \
  --output out/fire-emblem-fe1-korean-integrated.nes
```

`build-kr-patch`는 채택된 에셋과 런타임을 설치하는 반복 제품 빌드다. 원본 매퍼 전수 분석과 과거 조사 프로브를 매번 다시 실행하지 않는다. 원본 ABI·매퍼 코드·소유 계약이 바뀌거나 반례가 생겼을 때만 [빌드 파이프라인](docs/build-pipeline.md)의 해당 분석을 다시 연다.

`plan-full-translation-installation`은 기본적으로 보고서만 만들며, ROM 파일을 쓰려면 `--output`을 지정한다. 과거 호환 별칭 `--transport-probe`도 같은 인자지만 새 문서와 자동화는 `--output`을 사용한다.

## 구조

제품 경로는 지원 원본 결속 → 번역 재료 → 화면·수명 계획 → typed RP2A03 런타임 → Expected Write 설치 → 최종 이미지 검증의 한 방향으로 흐른다. emucap 실행 증거는 exact ROM·SaveRAM·입력 계보에 별도로 묶는다.

- [소유권 구조](docs/refactoring.md): 모듈과 도구의 단일 책임
- [로드맵](docs/roadmap.md): G1~G8 통과 관문
- [플레이테스트](docs/playtesting.md): 실행 원칙과 exact-ROM 계보
- [결정 기록](docs/decisions.md): 원인과 채택 판단의 이력
- [AI 협업](docs/ai-collaboration.md): 작업·검증·커밋 협업 규칙

## 공개 자료 경계

공개 저장소에는 추출·재삽입 도구, 구조 근거와 소규모 UI 번역을 둘 수 있다. 원본 ROM, 생성 ROM, 대규모 원문·번역 작업공간, SaveRAM, 캡처와 내부 QA는 커밋하거나 배포하지 않는다. 이 자료는 무시되는 `private/`, `out/`, `evidence/private/`에서만 다룬다.
