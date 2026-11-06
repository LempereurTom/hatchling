//! The command line. File manager menus call these exact forms, so changing
//! one breaks every installed right-click entry. Each task form takes one
//! archive or several, which run one after another.

use std::path::PathBuf;

use clap::{ArgGroup, Parser};

#[derive(Parser, Debug)]
#[command(
    name = "hatchzip-gui",
    version,
    about = "Extract large archives in place, removing the archive progressively.",
    group(ArgGroup::new("what").args([
        "archive",
        "extract_here",
        "extract_to",
        "extract_files",
        "test",
        "add",
        "add_zip",
        "add_7z",
        "add_each_zip",
        "extract_dialog",
        "register_shell",
        "unregister_shell",
    ])),
    group(ArgGroup::new("extracting").args(["extract_here", "extract_to", "extract_files"]))
)]
pub struct Args {
    /// The archive to open, or any part of a split archive
    pub archive: Option<PathBuf>,
    /// Extract into the folder each archive is in
    #[arg(long, value_name = "ARCHIVE", num_args = 1..)]
    pub extract_here: Option<Vec<PathBuf>>,
    /// Extract into a new folder named after each archive
    #[arg(long, value_name = "ARCHIVE", num_args = 1..)]
    pub extract_to: Option<Vec<PathBuf>>,
    /// Ask for a folder, then extract every archive into it
    #[arg(long, value_name = "ARCHIVE", num_args = 1..)]
    pub extract_files: Option<Vec<PathBuf>>,
    /// Check every file for damage, without writing anything
    #[arg(long, value_name = "ARCHIVE", num_args = 1..)]
    pub test: Option<Vec<PathBuf>>,
    /// Open "Archive name and parameters" for these files and folders
    #[arg(long, value_name = "PATH", num_args = 1..)]
    pub add: Option<Vec<PathBuf>>,
    /// Put these files and folders in one .zip next to them, with the saved settings
    #[arg(long, value_name = "PATH", num_args = 1..)]
    pub add_zip: Option<Vec<PathBuf>>,
    /// Put these files and folders in one .7z next to them, with the saved settings
    #[arg(long = "add-7z", value_name = "PATH", num_args = 1..)]
    pub add_7z: Option<Vec<PathBuf>>,
    /// Put each of these files and folders in its own .zip
    #[arg(long, value_name = "PATH", num_args = 1..)]
    pub add_each_zip: Option<Vec<PathBuf>>,
    /// Open the extraction window for one archive
    #[arg(long, value_name = "ARCHIVE")]
    pub extract_dialog: Option<PathBuf>,
    /// With an extract option: keep the archive, removing nothing
    #[arg(long, requires = "extracting")]
    pub keep_archive: bool,
    /// Add HatchZip to the file manager's right-click menu, then exit
    #[arg(long)]
    pub register_shell: bool,
    /// Remove HatchZip from the file manager's right-click menu, then exit
    #[arg(long)]
    pub unregister_shell: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskKind {
    ExtractHere,
    ExtractTo,
    /// Into a folder the person picks, which is asked for first when it is
    /// `None`.
    ExtractFiles(Option<PathBuf>),
    Test,
}

/// What a right-click asked for.
#[derive(Debug, PartialEq, Eq)]
pub struct TaskArgs {
    pub kind: TaskKind,
    /// One or more, done in this order.
    pub archives: Vec<PathBuf>,
    /// Extract without removing the archives.
    pub keep_archive: bool,
}

/// How "Add" was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddKind {
    /// Show "Archive name and parameters" first.
    Dialog,
    /// Straight to the progress, in this format.
    Quick(hatchzip_core::create::Format),
    /// One .zip per item, straight to the progress.
    EachZip,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Launch {
    /// The WinRAR-style browser, in a folder or inside an archive.
    Explorer(Option<PathBuf>),
    /// The extraction window for one archive.
    Main(Option<PathBuf>),
    /// Make an archive.
    Add(AddKind, Vec<PathBuf>),
    /// A small window that runs one task, started from a right-click.
    Task(TaskArgs),
    /// Installers run these; no window opens.
    RegisterShell,
    UnregisterShell,
}

impl Args {
    pub fn launch(self) -> Launch {
        let task = |kind, archives: Vec<PathBuf>| {
            Launch::Task(TaskArgs {
                kind,
                archives: expand_lists(archives),
                keep_archive: self.keep_archive,
            })
        };
        let add = |kind, paths| Launch::Add(kind, expand_lists(paths));
        use hatchzip_core::create::Format;
        if self.register_shell {
            Launch::RegisterShell
        } else if self.unregister_shell {
            Launch::UnregisterShell
        } else if let Some(a) = self.extract_here {
            task(TaskKind::ExtractHere, a)
        } else if let Some(a) = self.extract_to {
            task(TaskKind::ExtractTo, a)
        } else if let Some(a) = self.extract_files {
            task(TaskKind::ExtractFiles(None), a)
        } else if let Some(a) = self.test {
            task(TaskKind::Test, a)
        } else if let Some(a) = self.add {
            add(AddKind::Dialog, a)
        } else if let Some(a) = self.add_zip {
            add(AddKind::Quick(Format::Zip), a)
        } else if let Some(a) = self.add_7z {
            add(AddKind::Quick(Format::SevenZ), a)
        } else if let Some(a) = self.add_each_zip {
            add(AddKind::EachZip, a)
        } else if let Some(a) = self.extract_dialog {
            Launch::Main(Some(a))
        } else {
            Launch::Explorer(self.archive)
        }
    }
}

/// The Windows 11 menu passes a long selection as `@list.hzlist`, one path
/// per line, which it wrote to the temporary folder for this process.
fn expand_lists(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        let s = p.to_string_lossy();
        if let Some(list) = s.strip_prefix('@')
            && list.ends_with(".hzlist")
            && let Ok(text) = std::fs::read_to_string(list)
        {
            let _ = std::fs::remove_file(list);
            out.extend(text.lines().filter(|l| !l.is_empty()).map(PathBuf::from));
        } else {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    fn parse(args: &[&str]) -> Result<Launch, clap::Error> {
        let mut full = vec!["hatchzip-gui"];
        full.extend_from_slice(args);
        Args::try_parse_from(full).map(Args::launch)
    }

    #[test]
    fn command_line_is_well_formed() {
        Args::command().debug_assert();
    }

    fn task(kind: TaskKind, archives: &[&str], keep_archive: bool) -> Launch {
        Launch::Task(TaskArgs {
            kind,
            archives: archives.iter().map(PathBuf::from).collect(),
            keep_archive,
        })
    }

    #[test]
    fn every_documented_form_parses() {
        assert_eq!(parse(&[]).unwrap(), Launch::Explorer(None));
        assert_eq!(
            parse(&["game.zip"]).unwrap(),
            Launch::Explorer(Some("game.zip".into()))
        );
        assert_eq!(
            parse(&["--extract-dialog", "game.zip"]).unwrap(),
            Launch::Main(Some("game.zip".into()))
        );
        assert_eq!(
            parse(&["--add", "a", "b c"]).unwrap(),
            Launch::Add(AddKind::Dialog, vec!["a".into(), "b c".into()])
        );
        assert_eq!(
            parse(&["--add-7z", "a"]).unwrap(),
            Launch::Add(AddKind::Quick(hatchzip_core::create::Format::SevenZ), vec!["a".into()])
        );
        assert_eq!(
            parse(&["--extract-here", "a b/game.7z.001"]).unwrap(),
            task(TaskKind::ExtractHere, &["a b/game.7z.001"], false)
        );
        assert_eq!(
            parse(&["--extract-to", "game.zip"]).unwrap(),
            task(TaskKind::ExtractTo, &["game.zip"], false)
        );
        assert_eq!(
            parse(&["--extract-files", "game.zip"]).unwrap(),
            task(TaskKind::ExtractFiles(None), &["game.zip"], false)
        );
        assert_eq!(
            parse(&["--test", "game.tar"]).unwrap(),
            task(TaskKind::Test, &["game.tar"], false)
        );
        assert_eq!(
            parse(&["--extract-here=game.zip"]).unwrap(),
            task(TaskKind::ExtractHere, &["game.zip"], false)
        );
    }

    /// What the "keep archive" menu entries run, and what file managers
    /// that hand over every selected file at once pass.
    #[test]
    fn keeping_and_several_archives_parse() {
        assert_eq!(
            parse(&["--keep-archive", "--extract-here", "game.zip"]).unwrap(),
            task(TaskKind::ExtractHere, &["game.zip"], true)
        );
        assert_eq!(
            parse(&["--extract-to", "game.zip", "--keep-archive"]).unwrap(),
            task(TaskKind::ExtractTo, &["game.zip"], true)
        );
        assert_eq!(
            parse(&["--keep-archive", "--extract-here", "a.zip", "b c.7z.001"]).unwrap(),
            task(TaskKind::ExtractHere, &["a.zip", "b c.7z.001"], true)
        );
        assert_eq!(
            parse(&["--test", "a.zip", "b.tar"]).unwrap(),
            task(TaskKind::Test, &["a.zip", "b.tar"], false)
        );
        // Keeping only means something when extracting.
        assert!(parse(&["--keep-archive"]).is_err());
        assert!(parse(&["--keep-archive", "--test", "a.zip"]).is_err());
        assert!(parse(&["--keep-archive", "a.zip"]).is_err());
    }

    #[test]
    fn installers_can_add_and_remove_the_menu() {
        assert_eq!(parse(&["--register-shell"]).unwrap(), Launch::RegisterShell);
        assert_eq!(
            parse(&["--unregister-shell"]).unwrap(),
            Launch::UnregisterShell
        );
        assert!(parse(&["--register-shell", "--unregister-shell"]).is_err());
        assert!(parse(&["--register-shell", "a.zip"]).is_err());
    }

    #[test]
    fn one_thing_at_a_time() {
        assert!(parse(&["--test", "a.zip", "--extract-to", "b.zip"]).is_err());
        assert!(parse(&["a.zip", "--extract-here", "b.zip"]).is_err());
        assert!(parse(&["--extract-here"]).is_err());
        assert!(parse(&["a.zip", "b.zip"]).is_err());
    }
}
