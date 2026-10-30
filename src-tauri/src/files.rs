use serde::Serialize;
use std::fs::File;
use std::io;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct OpenError {
    pub code: String,
    pub detail: String,
}

fn io_error(error: io::Error) -> OpenError {
    let code = match error.kind() {
        io::ErrorKind::NotFound => "not_found",
        io::ErrorKind::PermissionDenied => "access_denied",
        _ => "unreadable",
    };
    OpenError {
        code: code.into(),
        detail: error.to_string(),
    }
}

pub fn inspect_file(input: &str) -> Result<crate::meta::FileMeta, OpenError> {
    let path = Path::new(input);
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("parquet"))
    {
        return Err(OpenError {
            code: "wrong_extension".into(),
            detail: input.into(),
        });
    }
    // Windows canonicalization produces extended-length paths, including UNC.
    let canonical = path.canonicalize().map_err(io_error)?;
    if !canonical.metadata().map_err(io_error)?.is_file() {
        return Err(OpenError {
            code: "not_a_file".into(),
            detail: input.into(),
        });
    }
    let mut file = File::open(&canonical).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    let (footer, bytes_read) = crate::meta::read_footer(&mut file, metadata.len())?;
    let after = file.metadata().map_err(io_error)?;
    if after.len() != metadata.len() || after.modified().ok() != metadata.modified().ok() {
        return Err(crate::meta::failure(
            "file_changed",
            "File changed while reading metadata; reopen to retry",
        ));
    }
    let mut result = crate::meta::summarize(
        &footer,
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        canonical.to_string_lossy().into(),
        metadata.len(),
        bytes_read,
    )?;
    result.modified = metadata.modified().ok();
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn opens_unicode_space_path_and_preserves_file() {
        let directory =
            std::env::temp_dir().join(format!("parquetlens-test-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("中文 样例 🦀.PARQUET");
        let contents = include_bytes!("../../tests/fixtures/中文 样例.parquet");
        fs::write(&path, contents).unwrap();
        let info = inspect_file(path.to_str().unwrap()).unwrap();
        assert_eq!(info.name, "中文 样例 🦀.PARQUET");
        assert_eq!(info.size_bytes, contents.len().to_string());
        assert_eq!(fs::read(&path).unwrap(), contents);
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn rejects_wrong_extension() {
        assert_eq!(
            inspect_file("data.csv").unwrap_err().code,
            "wrong_extension"
        );
    }

    #[test]
    fn rejects_directory_with_parquet_extension() {
        let path =
            std::env::temp_dir().join(format!("parquetlens-dir-{}.parquet", std::process::id()));
        fs::create_dir(&path).unwrap();
        let result = inspect_file(path.to_str().unwrap());
        fs::remove_dir(&path).unwrap();
        assert_eq!(result.unwrap_err().code, "not_a_file");
    }

    #[test]
    fn rejects_empty_file() {
        let path =
            std::env::temp_dir().join(format!("parquetlens-empty-{}.parquet", std::process::id()));
        fs::write(&path, []).unwrap();
        let result = inspect_file(path.to_str().unwrap());
        fs::remove_file(&path).unwrap();
        assert_eq!(result.unwrap_err().code, "parse_failed");
    }

    #[cfg(windows)]
    #[test]
    fn locked_file_returns_error_then_recovers() {
        use std::os::windows::fs::OpenOptionsExt;
        let path =
            std::env::temp_dir().join(format!("parquetlens-locked-{}.parquet", std::process::id()));
        fs::write(
            &path,
            include_bytes!("../../tests/fixtures/中文 样例.parquet"),
        )
        .unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert!(inspect_file(path.to_str().unwrap()).is_err());
        drop(lock);
        assert!(inspect_file(path.to_str().unwrap()).is_ok());
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn reports_missing_file() {
        let path = std::env::temp_dir().join(format!(
            "parquetlens-missing-{}.parquet",
            std::process::id()
        ));
        assert_eq!(
            inspect_file(path.to_str().unwrap()).unwrap_err().code,
            "not_found"
        );
    }
}
