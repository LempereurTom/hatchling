//! Creating archives: ZIP (Deflate, or stored) and 7z (LZMA2, or stored),
//! optionally encrypted with AES-256 and split into volumes.
//!
//! 7z archives are written in solid blocks of a bounded size rather than one
//! block for everything, so HatchZip can later extract them in place: the
//! extra space an in-place run needs is about one block.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Format {
    Zip,
    SevenZ,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Zip => "zip",
            Format::SevenZ => "7z",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    pub format: Format,
    /// 0 stores without compression, 9 is the slowest and smallest.
    pub level: u32,
    /// Empty or `None` means no encryption.
    pub password: Option<String>,
    /// 7z only: also encrypt the file names, so the archive can't even be
    /// listed without the password. ZIP can't hide names.
    pub encrypt_names: bool,
    /// 7z only: the most data one solid block holds. `None` compresses each
    /// file on its own, `Some(u64::MAX)` puts everything in one block.
    pub solid_block: Option<u64>,
    /// Split the archive into volumes of this many bytes, named
    /// `name.7z.001`, `name.7z.002` and so on.
    pub volume_size: Option<u64>,
    /// Compression threads, for 7z. 0 means one per core.
    pub threads: u32,
    /// ZIP only.
    pub comment: Option<String>,
    /// Read every file back from the new archive before reporting success.
    pub test_after: bool,
    /// Remove the source files once the archive is written (and tested,
    /// when `test_after` is set).
    pub delete_sources: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            format: Format::Zip,
            level: 5,
            password: None,
            encrypt_names: false,
            solid_block: Some(DEFAULT_SOLID_BLOCK),
            volume_size: None,
            threads: 0,
            comment: None,
            test_after: false,
            delete_sources: false,
        }
    }
}

/// Small enough that an in-place extraction needs little headroom, large
/// enough that the ratio stays close to fully solid.
pub const DEFAULT_SOLID_BLOCK: u64 = 256 * 1024 * 1024;

pub const PHASE_SCAN: u8 = 0;
pub const PHASE_COMPRESS: u8 = 1;
pub const PHASE_TEST: u8 = 2;
pub const PHASE_DELETE: u8 = 3;

/// Shared between the worker and the window that shows it.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub phase: AtomicU8,
    pub current: Mutex<String>,
    pub cancel: AtomicBool,
    /// Held while set: the encoder waits for the next read.
    pub pause: AtomicBool,
}

impl Progress {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    fn start(&self, phase: u8, total: u64) {
        self.phase.store(phase, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
    }
    fn name(&self, name: &str) {
        if let Ok(mut c) = self.current.lock() {
            name.clone_into(&mut c);
        }
    }
}

#[derive(Debug)]
pub struct Report {
    /// Every file written, the first volume first.
    pub volumes: Vec<PathBuf>,
    pub files: usize,
    pub unpacked: u64,
    pub packed: u64,
}

/// One file or folder to put in the archive.
#[derive(Debug, Clone)]
pub struct Item {
    pub source: PathBuf,
    /// The path inside the archive, with `/` separators.
    pub name: String,
    pub dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// Where an archive of `sources` goes by default: next to the first one,
/// named after it when there is one, after the folder they share otherwise.
pub fn default_destination(sources: &[PathBuf], format: Format) -> PathBuf {
    let first = &sources[0];
    let parent = first.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = if sources.len() == 1 {
        let name = first
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "archive".into());
        if first.is_dir() {
            name
        } else {
            first
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(name)
        }
    } else {
        parent
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "archive".into())
    };
    parent.join(format!("{stem}.{}", format.extension()))
}

/// Lists everything under `sources`, each named from its own folder: a
/// folder `C:\a\Game` becomes `Game/...` in the archive. `skip` is the
/// archive being written, which must not end up inside itself.
pub fn scan(sources: &[PathBuf], skip: &[PathBuf]) -> Result<Vec<Item>> {
    let skip: Vec<PathBuf> = skip
        .iter()
        .map(|p| fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
        .collect();
    let mut items = Vec::new();
    for source in sources {
        let name = source
            .file_name()
            .with_context(|| format!("{} has no name to store it under", source.display()))?
            .to_string_lossy()
            .into_owned();
        walk(source, name, &skip, &mut items)?;
    }
    Ok(items)
}

fn walk(path: &Path, name: String, skip: &[PathBuf], out: &mut Vec<Item>) -> Result<()> {
    let meta = fs::symlink_metadata(path).with_context(|| format!("reading {}", path.display()))?;
    // Links are left out, as WinRAR and 7-Zip do by default: following one
    // can loop, and storing it means nothing on another machine.
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if let Ok(real) = fs::canonicalize(path)
        && skip.contains(&real)
    {
        return Ok(());
    }
    let modified = meta.modified().ok();
    if meta.is_dir() {
        out.push(Item {
            source: path.to_path_buf(),
            name: name.clone(),
            dir: true,
            size: 0,
            modified,
        });
        let mut children: Vec<_> = fs::read_dir(path)
            .with_context(|| format!("listing {}", path.display()))?
            .collect::<io::Result<_>>()
            .with_context(|| format!("listing {}", path.display()))?;
        children.sort_by_key(|c| c.file_name());
        for child in children {
            let child_name = format!("{name}/{}", child.file_name().to_string_lossy());
            walk(&child.path(), child_name, skip, out)?;
        }
    } else {
        out.push(Item {
            source: path.to_path_buf(),
            name,
            dir: false,
            size: meta.len(),
            modified,
        });
    }
    Ok(())
}

/// Every file name the archive at `dest` will be written as.
pub fn volume_paths(dest: &Path, count: usize, split: bool) -> Vec<PathBuf> {
    if !split {
        return vec![dest.to_path_buf()];
    }
    (1..=count).map(|i| volume_path(dest, i)).collect()
}

fn volume_path(dest: &Path, index: usize) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(format!(".{index:03}"));
    PathBuf::from(s)
}

/// Writes one logical file across fixed-size volumes. Seeking back works,
/// which the 7z writer needs to fill in its start header at the end.
struct VolumeWriter {
    dest: PathBuf,
    size: Option<u64>,
    files: Vec<File>,
    pos: u64,
    written: u64,
}

impl VolumeWriter {
    fn new(dest: &Path, size: Option<u64>) -> Result<Self> {
        let mut w = Self {
            dest: dest.to_path_buf(),
            size: size.filter(|s| *s > 0),
            files: Vec::new(),
            pos: 0,
            written: 0,
        };
        w.file(0)?;
        Ok(w)
    }

    fn path(&self, index: usize) -> PathBuf {
        match self.size {
            None => self.dest.clone(),
            Some(_) => volume_path(&self.dest, index + 1),
        }
    }

    fn file(&mut self, index: usize) -> io::Result<&mut File> {
        while self.files.len() <= index {
            let path = self.path(self.files.len());
            let f = File::options()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| {
                    io::Error::new(e.kind(), format!("creating {}: {e}", path.display()))
                })?;
            self.files.push(f);
        }
        Ok(&mut self.files[index])
    }

    fn paths(&self) -> Vec<PathBuf> {
        (0..self.files.len()).map(|i| self.path(i)).collect()
    }

    fn finish(mut self) -> Result<Vec<PathBuf>> {
        for f in &mut self.files {
            f.flush()?;
            f.sync_all().context("flushing the archive to disk")?;
        }
        Ok(self.paths())
    }
}

impl Write for VolumeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let (index, offset, room) = match self.size {
            None => (0, self.pos, u64::MAX),
            Some(s) => ((self.pos / s) as usize, self.pos % s, s - self.pos % s),
        };
        let n = (buf.len() as u64).min(room) as usize;
        let f = self.file(index)?;
        f.seek(SeekFrom::Start(offset))?;
        f.write_all(&buf[..n])?;
        self.pos += n as u64;
        self.written = self.written.max(self.pos);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for VolumeWriter {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
            SeekFrom::End(d) => self.written as i128 + d as i128,
        };
        if pos < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before start",
            ));
        }
        self.pos = pos as u64;
        Ok(self.pos)
    }
}

/// Reads volumes back as one stream, for the test after creating.
pub struct VolumeReader {
    files: Vec<(File, u64)>,
    total: u64,
    pos: u64,
}

impl VolumeReader {
    pub fn open(paths: &[PathBuf]) -> Result<Self> {
        let mut files = Vec::new();
        let mut total = 0;
        for p in paths {
            let f = File::open(p).with_context(|| format!("opening {}", p.display()))?;
            let len = f.metadata()?.len();
            total += len;
            files.push((f, len));
        }
        Ok(Self {
            files,
            total,
            pos: 0,
        })
    }
}

impl Read for VolumeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut start = 0;
        for (f, len) in &mut self.files {
            if self.pos < start + *len {
                let offset = self.pos - start;
                let n = (buf.len() as u64).min(*len - offset) as usize;
                f.seek(SeekFrom::Start(offset))?;
                let n = f.read(&mut buf[..n])?;
                self.pos += n as u64;
                return Ok(n);
            }
            start += *len;
        }
        Ok(0)
    }
}

impl Seek for VolumeReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
            SeekFrom::End(d) => self.total as i128 + d as i128,
        };
        if pos < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before start",
            ));
        }
        self.pos = pos as u64;
        Ok(self.pos)
    }
}

/// A source file opened only when the encoder first reads it, counting what
/// it hands over and refusing to go on once the person cancels.
struct Source<'a> {
    path: PathBuf,
    file: Option<File>,
    progress: &'a Progress,
}

impl<'a> Source<'a> {
    fn new(path: &Path, progress: &'a Progress) -> Self {
        Self {
            path: path.to_path_buf(),
            file: None,
            progress,
        }
    }
}

fn cancelled_error() -> io::Error {
    io::Error::other(CANCELLED)
}

const CANCELLED: &str = "cancelled";

impl Read for Source<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.progress.pause.load(Ordering::Relaxed) && !self.progress.cancelled() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if self.progress.cancelled() {
            return Err(cancelled_error());
        }
        if self.file.is_none() {
            let f = File::open(&self.path).map_err(|e| {
                io::Error::new(e.kind(), format!("opening {}: {e}", self.path.display()))
            })?;
            self.file = Some(f);
        }
        let n = self.file.as_mut().expect("opened above").read(buf)?;
        self.progress.done.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// True when `e` is the person cancelling, not a failure.
pub fn is_cancelled(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.to_string().contains(CANCELLED))
}

/// Writes the archive. On any failure, or when cancelled, the volumes
/// written so far are removed and the sources are untouched.
pub fn create(
    sources: &[PathBuf],
    dest: &Path,
    options: &Options,
    progress: &Progress,
) -> Result<Report> {
    if sources.is_empty() {
        bail!("Nothing to archive.");
    }
    progress.start(PHASE_SCAN, 0);
    let split = options.volume_size.is_some_and(|s| s > 0);
    // Refuse before writing anything if any volume name is taken.
    let first = volume_paths(dest, 1, split).remove(0);
    if first.exists() {
        bail!(
            "{} already exists. Choose another name, or delete it first.",
            first.display()
        );
    }
    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let skip: Vec<PathBuf> = vec![first.clone()];
    let items = scan(sources, &skip)?;
    let total: u64 = items.iter().map(|i| i.size).sum();
    progress.start(PHASE_COMPRESS, total);

    let mut writer = VolumeWriter::new(dest, options.volume_size)?;
    let result = match options.format {
        Format::Zip => write_zip(&items, &mut writer, options, progress),
        Format::SevenZ => write_7z(&items, &mut writer, options, progress),
    };
    if let Err(e) = result {
        let paths = writer.paths();
        drop(writer);
        for p in paths {
            let _ = fs::remove_file(p);
        }
        return Err(e);
    }
    let volumes = writer.finish()?;
    let packed = volumes
        .iter()
        .filter_map(|p| fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();

    // The originals are only ever removed once every file has been read
    // back from the new archive, whether or not a test was asked for.
    if options.test_after || options.delete_sources {
        test(
            &volumes,
            options.format,
            options.password.as_deref(),
            progress,
        )
        .context("The new archive failed its test, so the source files were kept")?;
    }
    if options.delete_sources {
        progress.start(PHASE_DELETE, items.len() as u64);
        delete_sources(&items, progress)?;
    }
    Ok(Report {
        volumes,
        files: items.iter().filter(|i| !i.dir).count(),
        unpacked: total,
        packed,
    })
}

fn password(options: &Options) -> Option<&str> {
    options.password.as_deref().filter(|p| !p.is_empty())
}

fn zip_time(t: Option<SystemTime>) -> zip::DateTime {
    let Some(t) = t else {
        return zip::DateTime::default();
    };
    let utc = time::OffsetDateTime::from(t);
    let local = time::UtcOffset::current_local_offset()
        .map(|o| utc.to_offset(o))
        .unwrap_or(utc);
    zip::DateTime::from_date_and_time(
        local.year().clamp(1980, 2107) as u16,
        local.month() as u8,
        local.day(),
        local.hour(),
        local.minute(),
        local.second(),
    )
    .unwrap_or_default()
}

fn write_zip(
    items: &[Item],
    out: &mut VolumeWriter,
    options: &Options,
    progress: &Progress,
) -> Result<()> {
    use zip::CompressionMethod;
    use zip::write::SimpleFileOptions;

    let mut zip = zip::ZipWriter::new(out);
    let pw = password(options);
    for item in items {
        if progress.cancelled() {
            return Err(cancelled_error().into());
        }
        progress.name(&item.name);
        let mut opts = SimpleFileOptions::default().last_modified_time(zip_time(item.modified));
        if item.dir {
            zip.add_directory(format!("{}/", item.name), opts)
                .with_context(|| format!("adding {}", item.name))?;
            continue;
        }
        opts = if options.level == 0 {
            opts.compression_method(CompressionMethod::Stored)
        } else {
            opts.compression_method(CompressionMethod::Deflated)
                .compression_level(Some(options.level.min(9) as i64))
        };
        opts = opts.large_file(item.size >= u32::MAX as u64);
        if let Some(pw) = pw {
            opts = opts.with_aes_encryption(zip::AesMode::Aes256, pw);
        }
        zip.start_file(item.name.as_str(), opts)
            .with_context(|| format!("adding {}", item.name))?;
        let mut src = Source::new(&item.source, progress);
        io::copy(&mut src, &mut zip).with_context(|| format!("compressing {}", item.name))?;
    }
    if let Some(c) = options.comment.as_deref().filter(|c| !c.is_empty()) {
        zip.set_comment(c)?;
    }
    zip.finish().context("finishing the ZIP archive")?;
    Ok(())
}

fn sevenz_time(entry: &mut sevenz_rust2::ArchiveEntry, t: Option<SystemTime>) {
    if let Some(t) = t
        && let Ok(nt) = sevenz_rust2::NtTime::try_from(t)
    {
        entry.last_modified_date = nt;
        entry.has_last_modified_date = true;
    }
}

fn write_7z(
    items: &[Item],
    out: &mut VolumeWriter,
    options: &Options,
    progress: &Progress,
) -> Result<()> {
    use sevenz_rust2::encoder_options::{AesEncoderOptions, Lzma2Options};
    use sevenz_rust2::{
        ArchiveEntry, ArchiveWriter, EncoderConfiguration, EncoderMethod, Password, SourceReader,
    };

    let mut writer = ArchiveWriter::new(out).context("starting the 7z archive")?;
    let mut methods: Vec<EncoderConfiguration> = Vec::new();
    if let Some(pw) = password(options) {
        methods.push(AesEncoderOptions::new(Password::new(pw)).into());
    }
    if options.level == 0 {
        methods.push(EncoderMethod::COPY.into());
    } else {
        let threads = if options.threads == 0 {
            std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
        } else {
            options.threads
        };
        let lzma = if threads > 1 {
            Lzma2Options::from_level_mt(options.level.min(9), threads, 16 * 1024 * 1024)
        } else {
            Lzma2Options::from_level(options.level.min(9))
        };
        methods.push(lzma.into());
    }
    writer.set_content_methods(methods);
    writer.set_encrypt_header(password(options).is_some() && options.encrypt_names);

    let entry_for = |item: &Item| {
        let mut e = if item.dir {
            ArchiveEntry::new_directory(&item.name)
        } else {
            ArchiveEntry::new_file(&item.name)
        };
        sevenz_time(&mut e, item.modified);
        e
    };

    // Folders and empty files carry no data and go in no block.
    for item in items.iter().filter(|i| i.dir || i.size == 0) {
        let entry = entry_for(item);
        if item.dir {
            writer.push_archive_entry::<&[u8]>(entry, None)
        } else {
            writer.push_archive_entry(entry, Some(&[][..]))
        }
        .map_err(|e| anyhow::anyhow!("adding {}: {e}", item.name))?;
    }

    let files: Vec<&Item> = items.iter().filter(|i| !i.dir && i.size > 0).collect();
    let limit = options.solid_block.unwrap_or(0);
    let mut i = 0;
    while i < files.len() {
        if progress.cancelled() {
            return Err(cancelled_error().into());
        }
        let mut j = i;
        let mut size = 0u64;
        while j < files.len() && (j == i || size.saturating_add(files[j].size) <= limit) {
            size = size.saturating_add(files[j].size);
            j += 1;
        }
        progress.name(&files[i].name);
        let block = &files[i..j];
        let result = if block.len() == 1 {
            writer
                .push_archive_entry(
                    entry_for(block[0]),
                    Some(Source::new(&block[0].source, progress)),
                )
                .map(|_| ())
        } else {
            let entries = block.iter().map(|f| entry_for(f)).collect();
            let readers = block
                .iter()
                .map(|f| SourceReader::new(Source::new(&f.source, progress)))
                .collect();
            writer.push_archive_entries(entries, readers).map(|_| ())
        };
        result.map_err(|e| {
            if progress.cancelled() {
                anyhow::Error::from(cancelled_error())
            } else {
                anyhow::anyhow!("compressing {}: {e}", block[0].name)
            }
        })?;
        i = j;
    }
    writer.finish().context("finishing the 7z archive")?;
    Ok(())
}

/// Decompresses every file of a new archive and checks it against its
/// stored checksum, without writing anything.
pub fn test(
    volumes: &[PathBuf],
    format: Format,
    password: Option<&str>,
    progress: &Progress,
) -> Result<()> {
    let password = password.filter(|p| !p.is_empty());
    let reader = VolumeReader::open(volumes)?;
    progress.start(PHASE_TEST, reader.total);
    let mut sink = Counter { progress };
    match format {
        Format::Zip => {
            let mut zip = zip::ZipArchive::new(reader).context("reading the ZIP archive")?;
            progress.total.store(
                (0..zip.len())
                    .filter_map(|i| zip.by_index_raw(i).ok().map(|f| f.size()))
                    .sum(),
                Ordering::Relaxed,
            );
            for i in 0..zip.len() {
                if progress.cancelled() {
                    return Err(cancelled_error().into());
                }
                let mut f = match password {
                    Some(pw) => zip.by_index_decrypt(i, pw.as_bytes()),
                    None => zip.by_index(i),
                }
                .context("reading the ZIP archive")?;
                let name = f.name().map(|n| n.into_owned()).unwrap_or_default();
                progress.name(&name);
                io::copy(&mut f, &mut sink).with_context(|| format!("{name} is damaged"))?;
            }
        }
        Format::SevenZ => {
            let pw =
                password.map_or_else(sevenz_rust2::Password::empty, sevenz_rust2::Password::new);
            let mut archive = sevenz_rust2::ArchiveReader::new(reader, pw)
                .map_err(|e| anyhow::anyhow!("reading the 7z archive: {e}"))?;
            progress.total.store(
                archive.archive().files.iter().map(|f| f.size).sum(),
                Ordering::Relaxed,
            );
            archive
                .for_each_entries(|entry, data| {
                    if progress.cancelled() {
                        return Err(cancelled_error().into());
                    }
                    progress.name(entry.name());
                    io::copy(data, &mut sink)?;
                    Ok(true)
                })
                .map_err(|e| anyhow::anyhow!("the archive is damaged: {e}"))?;
        }
    }
    Ok(())
}

struct Counter<'a> {
    progress: &'a Progress,
}

impl Write for Counter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.progress
            .done
            .fetch_add(buf.len() as u64, Ordering::Relaxed);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Files first, then folders deepest first, and only folders left empty,
/// so nothing that was not archived is ever removed.
fn delete_sources(items: &[Item], progress: &Progress) -> Result<()> {
    for item in items.iter().filter(|i| !i.dir) {
        progress.name(&item.name);
        fs::remove_file(&item.source)
            .with_context(|| format!("removing {}", item.source.display()))?;
        progress.done.fetch_add(1, Ordering::Relaxed);
    }
    for item in items.iter().rev().filter(|i| i.dir) {
        // A folder that still holds something we skipped (a link) stays.
        let _ = fs::remove_dir(&item.source);
        progress.done.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(root: &Path) -> PathBuf {
        let src = root.join("Game");
        fs::create_dir_all(src.join("data/sub")).unwrap();
        fs::create_dir_all(src.join("empty")).unwrap();
        fs::write(src.join("readme.txt"), b"hello").unwrap();
        fs::write(src.join("zero.bin"), b"").unwrap();
        // Pseudo-random, so it doesn't compress below the volume size.
        let mut x = 1u32;
        let big: Vec<u8> = (0..3_000_000)
            .map(|_| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (x >> 16) as u8
            })
            .collect();
        fs::write(src.join("data/big.bin"), &big).unwrap();
        fs::write(src.join("data/sub/a.txt"), b"a".repeat(10_000)).unwrap();
        src
    }

    fn roundtrip(options: Options) {
        let dir = tempfile::tempdir().unwrap();
        let src = tree(dir.path());
        let dest = dir
            .path()
            .join(format!("out.{}", options.format.extension()));
        let progress = Progress::default();
        let report = create(std::slice::from_ref(&src), &dest, &options, &progress).unwrap();
        assert_eq!(report.files, 4);
        if options.volume_size.is_some() {
            assert!(report.volumes.len() > 1, "{:?}", report.volumes);
        }
        test(
            &report.volumes,
            options.format,
            options.password.as_deref(),
            &progress,
        )
        .unwrap();
        assert_eq!(progress.done.load(Ordering::Relaxed), report.unpacked);
        if options.delete_sources {
            assert!(!src.exists());
        }
    }

    #[test]
    fn zip_roundtrips() {
        roundtrip(Options::default());
    }

    #[test]
    fn zip_stored_encrypted_split() {
        roundtrip(Options {
            level: 0,
            password: Some("secret".into()),
            volume_size: Some(1_000_000),
            comment: Some("hi".into()),
            test_after: true,
            ..Options::default()
        });
    }

    #[test]
    fn sevenz_solid_blocks() {
        roundtrip(Options {
            format: Format::SevenZ,
            solid_block: Some(1_000_000),
            ..Options::default()
        });
    }

    #[test]
    fn sevenz_encrypted_names_split_and_deleted() {
        roundtrip(Options {
            format: Format::SevenZ,
            level: 9,
            password: Some("secret".into()),
            encrypt_names: true,
            solid_block: Some(u64::MAX),
            volume_size: Some(200_000),
            test_after: true,
            delete_sources: true,
            ..Options::default()
        });
    }

    #[test]
    fn wrong_password_fails_the_test() {
        let dir = tempfile::tempdir().unwrap();
        let src = tree(dir.path());
        let dest = dir.path().join("out.7z");
        let options = Options {
            format: Format::SevenZ,
            password: Some("right".into()),
            ..Options::default()
        };
        let progress = Progress::default();
        let report = create(&[src], &dest, &options, &progress).unwrap();
        assert!(test(&report.volumes, Format::SevenZ, Some("wrong"), &progress).is_err());
    }

    #[test]
    fn cancelling_removes_the_partial_archive() {
        let dir = tempfile::tempdir().unwrap();
        let src = tree(dir.path());
        let dest = dir.path().join("out.zip");
        let progress = Progress::default();
        progress.cancel.store(true, Ordering::Relaxed);
        let err = create(
            std::slice::from_ref(&src),
            &dest,
            &Options::default(),
            &progress,
        )
        .unwrap_err();
        assert!(is_cancelled(&err), "{err:#}");
        assert!(!dest.exists());
        assert!(src.join("readme.txt").exists());
    }

    #[test]
    fn never_archives_itself() {
        let dir = tempfile::tempdir().unwrap();
        let src = tree(dir.path());
        let dest = src.join("Game.zip");
        let progress = Progress::default();
        let report = create(&[src], &dest, &Options::default(), &progress).unwrap();
        let zip = zip::ZipArchive::new(File::open(&report.volumes[0]).unwrap()).unwrap();
        assert!(zip.file_names().all(|n| !n.unwrap().ends_with("Game.zip")));
    }

    #[test]
    fn refuses_an_existing_archive() {
        let dir = tempfile::tempdir().unwrap();
        let src = tree(dir.path());
        let dest = dir.path().join("out.zip");
        fs::write(&dest, b"keep me").unwrap();
        assert!(create(&[src], &dest, &Options::default(), &Progress::default()).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"keep me");
    }
}
