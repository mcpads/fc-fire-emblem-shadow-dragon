# FC Fire Emblem Korean patch

패미컴판 《파이어 엠블렘 암흑룡과 빛의 검》의 한글화 프로젝트다.

## 번역 범위

- 지원 원본은 SHA-1 `0179c550d424e0397496078789e7b116601d120c`인 일본판이다.
- **일본어만 한국어로 번역한다.** 일본판에 원래 들어 있는 영어, 숫자, 로마자 약어는 번역하거나 글꼴 슬롯을 덮지 않는다.
- 영문 패치는 주소와 자료 구조를 교차 확인하는 조사 자료일 뿐, 번역 원문이나 제품 빌드 입력이 아니다.

## 현재 게이트

설정 항목 `サウンド`, `アニメーション`, `ウエイトタイマー`를 각각 `사운드`, `애니메이션`, `대기시간`으로 바꾸는 기술 PoC를 제공한다. Mesen의 실제 설정 화면에서 세 한글 항목과 기존 영어 능력치 약어가 함께 유지되는 것을 확인했다. 일본어 설정 문자열에 쓰인 가나 타일만 임시로 한글 타일로 바꾸므로, 다른 일본어 화면이 깨질 수 있다. 정식 패치나 배포 후보가 아니다.

```sh
cargo run -p fc-fire-emblem-patch -- verify-source "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- analyze-font-supply "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
cargo run -p fc-fire-emblem-patch -- build-options-poc "roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
```

ROM과 빌드 결과는 저장소에 포함하지 않는다. 조사 근거와 현재 상태는 `docs/initial-survey.md`와 `docs/status.md`, 전체 한글화의 단계별 통과 조건은 `docs/roadmap.md`에 정리한다.
