//! Reads one file out of an archive into a folder, leaving the archive as
//! it is: what "View" in the browser opens. ZIP and 7z only, the formats
//! that can find one entry without reading everything before it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::create::VolumeReader;

/// `format_name` is [`crate::engine::Job::format_name`]; `inner` the entry's
/// path inside the archive with `/` separators. Returns the file written.
pub fn extract_entry(
    parts: &[PathBuf],
    format_name: &str,
    inner: &str,
    password: Option<&str>,
    out_dir: &Path,
) -> Result<PathBuf> {
    let file_name = inner.rsplit('/').next().unwrap_or(inner);
    if file_name.is_empty() || file_name == ".." || file_name.contains(['\\', ':']) {
        bail!("'{inner}' can't be opened on its own");
    }
    fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    let out = out_dir.join(file_name);
    let reader = VolumeReader::open(parts)?;
    let password = password.filter(|p| !p.is_empty());
    match format_name {
        "zip" => {
            let mut zip = zip::ZipArchive::new(reader).context("reading the ZIP archive")?;
            let index = zip
                .index_for_name(inner)
                .with_context(|| format!("'{inner}' is not in the archive"))?;
            let mut entry = match password {
                Some(pw) => zip.by_index_decrypt(index, pw.as_bytes()),
                None => zip.by_index(index),
            }
            .context("reading the ZIP archive")?;
            let mut f =
                fs::File::create(&out).with_context(|| format!("creating {}", out.display()))?;
            io::copy(&mut entry, &mut f).with_context(|| format!("{inner} is damaged"))?;
        }
        "7z" => {
            let pw =
                password.map_or_else(sevenz_rust2::Password::empty, sevenz_rust2::Password::new);
            let mut archive = sevenz_rust2::ArchiveReader::new(reader, pw)
                .map_err(|e| anyhow::anyhow!("reading the 7z archive: {e}"))?;
            let bytes = archive
                .read_file(inner)
                .map_err(|e| anyhow::anyhow!("reading {inner}: {e}"))?;
            fs::write(&out, bytes).with_context(|| format!("writing {}", out.display()))?;
        }
        other => bail!(
            "Viewing one file is available for ZIP and 7z archives; this is {other}. Extract the archive to open its files."
        ),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::{Format, Options, Progress, create};

    #[test]
    fn reads_one_file_from_each_format() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("Game");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("sub/readme.txt"), b"hello").unwrap();
        fs::write(src.join("other.bin"), vec![7u8; 100_000]).unwrap();
        for format in [Format::Zip, Format::SevenZ] {
            let dest = dir.path().join(format!("g.{}", format.extension()));
            let options = Options {
                format,
                password: Some("pw".into()),
                volume_size: Some(70_000),
                ..Options::default()
            };
            let report = create(
                std::slice::from_ref(&src),
                &dest,
                &options,
                &Progress::default(),
            )
            .unwrap();
            let out = dir.path().join(format!("view-{}", format.extension()));
            let got = extract_entry(
                &report.volumes,
                format.extension(),
                "Game/sub/readme.txt",
                Some("pw"),
                &out,
            )
            .unwrap();
            assert_eq!(fs::read(got).unwrap(), b"hello");
            assert!(
                extract_entry(
                    &report.volumes,
                    format.extension(),
                    "Game/missing",
                    Some("pw"),
                    &out
                )
                .is_err()
            );
        }
    }
}
