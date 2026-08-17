use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};

pub(super) fn write_full_translation_artifacts(
    output_path: Option<&Path>,
    report_path: &Path,
    source_path: &Path,
    current_candidate_path: &Path,
    installed_image: &[u8],
    report_bytes: &[u8],
) -> Result<()> {
    let resolved_report = resolve_output_identity(report_path)?;
    for protected_path in [source_path, current_candidate_path] {
        ensure!(
            resolved_report != resolve_output_identity(protected_path)?,
            "full translation report must not overwrite protected input {}",
            protected_path.display()
        );
    }

    if let Some(output_path) = output_path {
        let resolved_output = resolve_output_identity(output_path)?;
        for protected_path in [source_path, current_candidate_path, report_path] {
            ensure!(
                resolved_output != resolve_output_identity(protected_path)?,
                "integrated output must not overwrite protected input {}",
                protected_path.display()
            );
        }
        write_and_verify(output_path, installed_image, "integrated output")?;
    }

    write_and_verify(report_path, report_bytes, "full translation report")
}

fn write_and_verify(path: &Path, bytes: &[u8], role: &str) -> Result<()> {
    fs::write(path, bytes).with_context(|| format!("write {role} {}", path.display()))?;
    let read_back = fs::read(path).with_context(|| format!("read {role} {}", path.display()))?;
    ensure!(read_back == bytes, "{role} read-back differs from its plan");
    Ok(())
}

fn resolve_output_identity(path: &Path) -> Result<std::path::PathBuf> {
    if path.exists() {
        return fs::canonicalize(path)
            .with_context(|| format!("resolve existing path {}", path.display()));
    }
    let name = path
        .file_name()
        .context("output or report path has no file name")?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create output directory {}", parent.display()))?;
    Ok(fs::canonicalize(parent)
        .with_context(|| format!("resolve output directory {}", parent.display()))?
        .join(name))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temporary_directory(role: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fc-fire-emblem-{role}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn writes_and_reads_back_both_full_translation_artifacts() {
        let directory = temporary_directory("artifact-output");
        let source = directory.join("source.nes");
        let candidate = directory.join("candidate.nes");
        let output = directory.join("final.nes");
        let report = directory.join("final.json");
        fs::write(&source, b"source").unwrap();
        fs::write(&candidate, b"candidate").unwrap();

        write_full_translation_artifacts(
            Some(&output),
            &report,
            &source,
            &candidate,
            b"integrated",
            b"{\"complete\":false}\n",
        )
        .unwrap();

        assert_eq!(fs::read(&output).unwrap(), b"integrated");
        assert_eq!(fs::read(&report).unwrap(), b"{\"complete\":false}\n");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn refuses_to_overwrite_any_full_translation_input() {
        let directory = temporary_directory("artifact-protection");
        let source = directory.join("source.nes");
        let candidate = directory.join("candidate.nes");
        fs::write(&source, b"source").unwrap();
        fs::write(&candidate, b"candidate").unwrap();

        let report_error = write_full_translation_artifacts(
            None,
            &source,
            &source,
            &candidate,
            b"integrated",
            b"report",
        )
        .unwrap_err();
        assert!(
            report_error
                .to_string()
                .contains("report must not overwrite")
        );

        let output_error = write_full_translation_artifacts(
            Some(&candidate),
            &directory.join("report.json"),
            &source,
            &candidate,
            b"integrated",
            b"report",
        )
        .unwrap_err();
        assert!(
            output_error
                .to_string()
                .contains("integrated output must not overwrite")
        );
        assert_eq!(fs::read(&source).unwrap(), b"source");
        assert_eq!(fs::read(&candidate).unwrap(), b"candidate");
        fs::remove_dir_all(directory).unwrap();
    }
}
