//! 테스트가 쓰는 ROM 적재 도우미다. 산출물이 없으면 명확한 안내로 실패한다.

use std::path::PathBuf;

use crate::rom::Rom;

/// 누적 빌드가 만든 배포 이미지를 읽는다.
pub(crate) fn release_rom() -> Rom {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../out/fire-emblem-fe1-korean-release.nes");
    let data = std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "release image {} is missing ({error}); run the cumulative build and BuildReleaseImage first",
            path.display()
        )
    });
    Rom::parse(data).expect("release image parses")
}
