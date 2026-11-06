//! "Archive name and parameters" and the progress of making the archive.
//! The right-click entries that name a format skip the dialog and start
//! with the saved settings, as WinRAR's "Add to name.rar" does.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use hatchzip_core::create::{self, Format, Options, Progress};
use hatchzip_core::human;
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::args::AddKind;
use crate::settings::{self, AddDefaults, Settings};
use crate::{AddScreen, AddWindow};

const LEVELS: [u32; 6] = [0, 1, 3, 5, 7, 9];
const VOLUMES: [u64; 7] = [
    0,
    100_000_000,
    700_000_000,
    1_000_000_000,
    // The largest file FAT32 holds.
    4_294_967_295,
    4_700_000_000,
    25_000_000_000,
];
const MIB: u64 = 1024 * 1024;
const SOLID: [u64; 6] = [0, 64 * MIB, 256 * MIB, 1024 * MIB, 4096 * MIB, u64::MAX];

fn method_index(level: u32) -> i32 {
    LEVELS
        .iter()
        .position(|l| *l >= level)
        .unwrap_or(LEVELS.len() - 1) as i32
}

/// "700 MB", "2,5 Go", "4GB", "100": bytes, decimal units as WinRAR shows
/// them, French unit names too. A bare number is megabytes.
pub fn parse_size(text: &str) -> Option<u64> {
    let t = text.trim().replace(',', ".").replace(' ', "");
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let num: f64 = num.parse().ok()?;
    let mult = match unit.to_ascii_lowercase().as_str() {
        "" | "m" | "mb" | "mo" => 1e6,
        "k" | "kb" | "ko" => 1e3,
        "g" | "gb" | "go" => 1e9,
        "t" | "tb" | "to" => 1e12,
        "b" | "o" => 1.0,
        _ => return None,
    };
    let bytes = (num * mult) as u64;
    (bytes >= 64 * 1024).then_some(bytes)
}

/// `name.zip` becomes `name.7z` when the format changes, and the other way.
fn with_extension(path: &str, format: Format) -> String {
    let p = Path::new(path);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("zip") || ext.eq_ignore_ascii_case("7z") {
        p.with_extension(format.extension()).display().to_string()
    } else if path.is_empty() {
        path.to_string()
    } else {
        format!("{path}.{}", format.extension())
    }
}

/// `name.zip`, or `name (2).zip` and so on when that is taken, so a quick
/// add never fails on a name left from an earlier run.
fn free_name(dest: PathBuf, split: bool) -> PathBuf {
    let taken = |p: &Path| {
        if split {
            create::volume_paths(p, 1, true)[0].exists()
        } else {
            p.exists()
        }
    };
    if !taken(&dest) {
        return dest;
    }
    let stem = dest
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let ext = dest
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    for i in 2.. {
        let candidate = dest.with_file_name(format!("{stem} ({i}).{ext}"));
        if !taken(&candidate) {
            return candidate;
        }
    }
    unreachable!()
}

fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// One archive to make.
struct Job {
    sources: Vec<PathBuf>,
    dest: PathBuf,
}

struct App {
    ui: slint::Weak<AddWindow>,
    sources: Vec<PathBuf>,
    settings: RefCell<Settings>,
    password: RefCell<Option<String>>,
    progress: RefCell<Option<Arc<Progress>>>,
    timer: Timer,
    output_dir: RefCell<Option<PathBuf>>,
}

pub fn run(kind: AddKind, sources: Vec<PathBuf>) -> Result<()> {
    let sources: Vec<PathBuf> = sources
        .into_iter()
        .map(|p| std::path::absolute(&p).unwrap_or(p))
        .collect();
    if sources.is_empty() {
        bail!("Nothing to add.");
    }
    let saved = Settings::saved();
    let ui = AddWindow::new()?;
    let app = Rc::new(App {
        ui: ui.as_weak(),
        sources,
        settings: RefCell::new(saved),
        password: RefCell::new(None),
        progress: RefCell::new(None),
        timer: Timer::default(),
        output_dir: RefCell::new(None),
    });
    app.fill();
    wire(&ui, &app);
    match kind {
        AddKind::Dialog => {}
        AddKind::Quick(format) => {
            ui.set_format(if format == Format::Zip { 0 } else { 1 });
            app.format_changed();
            app.start(false);
        }
        AddKind::EachZip => {
            ui.set_format(0);
            ui.set_each_separately(true);
            app.format_changed();
            app.start(false);
        }
    }
    ui.run()?;
    Ok(())
}

fn wire(ui: &AddWindow, app: &Rc<App>) {
    let a = app.clone();
    ui.on_browse(move || a.browse());
    let a = app.clone();
    ui.on_ok(move || a.start(true));
    let a = app.clone();
    ui.on_cancel(move || a.cancel());
    let a = app.clone();
    ui.on_save_defaults(move || a.save_defaults());
    let a = app.clone();
    ui.on_set_password(move |x, y| a.set_password(x.to_string(), y.to_string()));
    let a = app.clone();
    ui.on_clear_password(move || {
        *a.password.borrow_mut() = None;
        let ui = a.ui();
        ui.set_password_a("".into());
        ui.set_password_b("".into());
        ui.set_password_open(false);
        ui.set_password_state(ui.invoke_no_password());
    });
    let a = app.clone();
    ui.on_pause_toggled(move || {
        if let Some(p) = &*a.progress.borrow() {
            let now = !p.pause.load(Ordering::Relaxed);
            p.pause.store(now, Ordering::Relaxed);
            a.ui().set_paused(now);
        }
    });
    let a = app.clone();
    ui.on_open_folder(move || {
        if let Some(d) = &*a.output_dir.borrow() {
            let _ = open::that_detached(d);
        }
    });
    let a = app.clone();
    ui.on_format_changed(move || a.format_changed());
    let a = app.clone();
    ui.on_pick_history(move |i| {
        let history = a.settings.borrow().archive_history.clone();
        if let Some(p) = history.get(i as usize) {
            let ui = a.ui();
            ui.set_archive_name(p.display().to_string().into());
            let fmt = if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("7z")) {
                1
            } else {
                0
            };
            ui.set_format(fmt);
        }
    });
    let a = app.clone();
    ui.window().on_close_requested(move || {
        // Closing while it runs is cancelling it. The window stays until the
        // worker has removed the partial archive, or it would be left behind.
        if a.ui().get_screen() == AddScreen::Working {
            a.cancel();
            return slint::CloseRequestResponse::KeepWindowShown;
        }
        slint::CloseRequestResponse::HideWindow
    });
}

impl App {
    fn ui(&self) -> AddWindow {
        self.ui
            .upgrade()
            .expect("the window outlives its callbacks")
    }

    fn format(&self) -> Format {
        if self.ui().get_format() == 1 {
            Format::SevenZ
        } else {
            Format::Zip
        }
    }

    fn fill(&self) {
        let ui = self.ui();
        let s = self.settings.borrow();
        let d = &s.add;
        ui.set_format(if d.format == Format::Zip { 0 } else { 1 });
        ui.set_method(method_index(d.level));
        match VOLUMES.iter().position(|v| *v == d.volume_size) {
            Some(i) => ui.set_volume_choice(i as i32),
            None => {
                ui.set_volume_choice(7);
                ui.set_volume_custom(human(d.volume_size).into());
            }
        }
        ui.set_solid_choice(SOLID.iter().position(|v| *v == d.solid_block).unwrap_or(2) as i32);
        ui.set_threads(d.threads as i32);
        ui.set_test_after(d.test_after);
        ui.set_delete_after(d.delete_sources);
        ui.set_encrypt_names(d.encrypt_names);
        ui.set_password_state(ui.invoke_no_password());
        let dest = create::default_destination(&self.sources, d.format);
        ui.set_archive_name(dest.display().to_string().into());
        ui.set_sources_text(
            self.sources
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n")
                .into(),
        );
        let history: Vec<SharedString> = s
            .archive_history
            .iter()
            .map(|p| p.display().to_string().into())
            .collect();
        ui.set_history(ModelRc::new(VecModel::from(history)));
    }

    fn format_changed(&self) {
        let ui = self.ui();
        let name = with_extension(&ui.get_archive_name(), self.format());
        ui.set_archive_name(name.into());
    }

    fn browse(&self) {
        let ui = self.ui();
        let current = PathBuf::from(ui.get_archive_name().as_str());
        let ext = self.format().extension();
        let mut dialog = rfd::FileDialog::new()
            .set_title(ui.invoke_save_title().as_str())
            .add_filter(ext.to_uppercase(), &[ext]);
        if let Some(dir) = current.parent() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(name) = current.file_name() {
            dialog = dialog.set_file_name(name.to_string_lossy());
        }
        if let Some(p) = dialog.save_file() {
            ui.set_archive_name(with_extension(&p.display().to_string(), self.format()).into());
        }
    }

    fn set_password(&self, a: String, b: String) {
        let ui = self.ui();
        if a != b {
            ui.set_password_error(ui.invoke_passwords_differ());
            return;
        }
        ui.set_password_error("".into());
        ui.set_password_open(false);
        if a.is_empty() {
            *self.password.borrow_mut() = None;
            ui.set_password_state(ui.invoke_no_password());
        } else {
            *self.password.borrow_mut() = Some(a);
            ui.set_password_state(ui.invoke_password_set());
        }
    }

    /// The dialog's choices, or `None` with the reason shown.
    fn defaults(&self) -> Option<AddDefaults> {
        let ui = self.ui();
        let volume_size = match ui.get_volume_choice() as usize {
            7 => match parse_size(&ui.get_volume_custom()) {
                Some(v) => v,
                None => {
                    ui.set_password_state(ui.invoke_bad_volume());
                    return None;
                }
            },
            i => VOLUMES.get(i).copied().unwrap_or(0),
        };
        Some(AddDefaults {
            format: self.format(),
            level: LEVELS[(ui.get_method() as usize).min(LEVELS.len() - 1)],
            volume_size,
            solid_block: SOLID[(ui.get_solid_choice() as usize).min(SOLID.len() - 1)],
            threads: ui.get_threads().max(0) as u32,
            test_after: ui.get_test_after(),
            delete_sources: ui.get_delete_after(),
            encrypt_names: ui.get_encrypt_names(),
        })
    }

    fn save_defaults(&self) {
        let Some(d) = self.defaults() else { return };
        let mut s = self.settings.borrow_mut();
        s.add = d;
        if let Some(f) = settings::file() {
            s.save(&f);
        }
        let ui = self.ui();
        ui.set_password_state(ui.invoke_defaults_saved());
    }

    fn jobs(&self, d: &AddDefaults, from_dialog: bool) -> Vec<Job> {
        let ui = self.ui();
        let split = d.volume_size > 0;
        let named = PathBuf::from(ui.get_archive_name().trim());
        if ui.get_each_separately() {
            // Next to the name given, one archive per item, named after it.
            let dir = named
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(Path::to_path_buf);
            return self
                .sources
                .iter()
                .map(|s| {
                    let mut dest = create::default_destination(std::slice::from_ref(s), d.format);
                    if let (Some(dir), Some(name)) = (&dir, dest.file_name()) {
                        dest = dir.join(name);
                    }
                    Job {
                        sources: vec![s.clone()],
                        dest: free_name(dest, split),
                    }
                })
                .collect();
        }
        let dest = if from_dialog && !named.as_os_str().is_empty() {
            // A bare name goes next to the files, as in WinRAR.
            if named.is_absolute() {
                named
            } else {
                self.sources[0]
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(named)
            }
        } else {
            free_name(create::default_destination(&self.sources, d.format), split)
        };
        vec![Job {
            sources: self.sources.clone(),
            dest,
        }]
    }

    fn start(self: &Rc<Self>, from_dialog: bool) {
        let ui = self.ui();
        let Some(d) = (if from_dialog {
            self.defaults()
        } else {
            Some(self.settings.borrow().add.clone()).map(|mut d| {
                d.format = self.format();
                d
            })
        }) else {
            return;
        };
        let jobs = self.jobs(&d, from_dialog);
        let options = Options {
            format: d.format,
            level: d.level,
            password: self.password.borrow().clone(),
            encrypt_names: d.encrypt_names,
            solid_block: match d.solid_block {
                0 => None,
                s => Some(s),
            },
            volume_size: (d.volume_size > 0).then_some(d.volume_size),
            threads: d.threads,
            comment: Some(ui.get_comment().to_string()).filter(|c| !c.trim().is_empty()),
            test_after: d.test_after,
            delete_sources: d.delete_sources,
        };
        if let Some(dir) = jobs[0].dest.parent() {
            *self.output_dir.borrow_mut() = Some(dir.to_path_buf());
        }
        {
            let mut s = self.settings.borrow_mut();
            for j in &jobs {
                settings::remember(&mut s.archive_history, &j.dest);
            }
            if let Some(f) = settings::file() {
                s.save(&f);
            }
        }

        let progress = Arc::new(Progress::default());
        *self.progress.borrow_mut() = Some(progress.clone());
        ui.set_screen(AddScreen::Working);
        let names: Vec<String> = jobs
            .iter()
            .map(|j| {
                j.dest
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        ui.set_heading(ui.invoke_creating(names[0].clone().into()));

        // Which job runs, written by the worker, read by the timer.
        let index = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let started = Instant::now();
        let weak = self.ui.clone();
        let p = progress.clone();
        let idx = index.clone();
        let count = jobs.len();
        self.timer
            .start(TimerMode::Repeated, Duration::from_millis(150), move || {
                let Some(ui) = weak.upgrade() else { return };
                let done = p.done.load(Ordering::Relaxed);
                let total = p.total.load(Ordering::Relaxed);
                let phase = p.phase.load(Ordering::Relaxed);
                let i = idx.load(Ordering::Relaxed).min(count - 1);
                let name: SharedString = names[i].clone().into();
                let mut heading = match phase {
                    create::PHASE_TEST => ui.invoke_testing(name),
                    create::PHASE_DELETE => ui.invoke_deleting(),
                    create::PHASE_SCAN => ui.invoke_scanning(),
                    _ => ui.invoke_creating(name),
                };
                if count > 1 {
                    heading = format!(
                        "{}  ({})",
                        heading,
                        ui.invoke_archive_n((i + 1).to_string().into(), count.to_string().into())
                    )
                    .into();
                }
                ui.set_heading(heading);
                if let Ok(c) = p.current.lock() {
                    ui.set_current(c.as_str().into());
                }
                if phase == create::PHASE_DELETE {
                    ui.set_amounts("".into());
                    ui.set_rate("".into());
                } else {
                    ui.set_amounts(ui.invoke_amounts_of(human(done).into(), human(total).into()));
                    let elapsed = started.elapsed();
                    let speed = done as f64 / elapsed.as_secs_f64().max(0.001);
                    let left = if speed > 0.0 && total > done {
                        Duration::from_secs_f64((total - done) as f64 / speed)
                    } else {
                        Duration::ZERO
                    };
                    ui.set_rate(ui.invoke_rate_of(
                        human(speed as u64).into(),
                        clock(elapsed).into(),
                        clock(left).into(),
                    ));
                }
                ui.set_progress(if total > 0 {
                    done as f32 / total as f32
                } else {
                    0.0
                });
            });

        let weak = self.ui.clone();
        std::thread::spawn(move || {
            let mut reports = Vec::new();
            let mut failure = None;
            for (i, job) in jobs.iter().enumerate() {
                index.store(i, Ordering::Relaxed);
                match create::create(&job.sources, &job.dest, &options, &progress) {
                    Ok(r) => reports.push(r),
                    Err(e) => {
                        failure = Some(e);
                        break;
                    }
                }
            }
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else {
                    // The window was closed to cancel; nothing left to show.
                    let _ = slint::quit_event_loop();
                    return;
                };
                finish(&ui, reports, failure, from_dialog);
            });
        });
    }

    fn cancel(&self) {
        let ui = self.ui();
        if ui.get_screen() == AddScreen::Working {
            if let Some(p) = &*self.progress.borrow() {
                p.cancel.store(true, Ordering::Relaxed);
                p.pause.store(false, Ordering::Relaxed);
            }
            return;
        }
        let _ = ui.hide();
    }
}

fn finish(
    ui: &AddWindow,
    reports: Vec<create::Report>,
    failure: Option<anyhow::Error>,
    from_dialog: bool,
) {
    let files: usize = reports.iter().map(|r| r.files).sum();
    let size: u64 = reports.iter().map(|r| r.unpacked).sum();
    let packed: u64 = reports.iter().map(|r| r.packed).sum();
    let percent = (packed * 100).checked_div(size).unwrap_or(100);
    let summary = ui.invoke_finished(
        files.to_string().into(),
        human(size).into(),
        human(packed).into(),
        percent.to_string().into(),
    );
    ui.set_progress(1.0);
    match failure {
        None => {
            // Like WinRAR, a successful run just goes away. From the dialog
            // too: the person already saw the settings.
            let _ = from_dialog;
            ui.set_screen(AddScreen::Done);
            ui.set_result_text(summary);
            ui.set_result_bad(false);
            let weak = ui.as_weak();
            Timer::single_shot(Duration::from_millis(900), move || {
                if let Some(ui) = weak.upgrade() {
                    let _ = ui.hide();
                }
            });
        }
        Some(e) if create::is_cancelled(&e) => {
            ui.set_screen(AddScreen::Failed);
            ui.set_result_text(ui.invoke_cancelled_text());
            ui.set_result_bad(false);
        }
        Some(e) => {
            ui.set_screen(AddScreen::Failed);
            let done = if reports.is_empty() {
                String::new()
            } else {
                format!("\n{summary}")
            };
            ui.set_result_text(
                format!("{}{done}", crate::work::sentence(&format!("{e:#}"))).into(),
            );
            ui.set_result_bad(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse_in_english_and_french() {
        assert_eq!(parse_size("700 MB"), Some(700_000_000));
        assert_eq!(parse_size("2,5 Go"), Some(2_500_000_000));
        assert_eq!(parse_size("4GB"), Some(4_000_000_000));
        assert_eq!(parse_size("100"), Some(100_000_000));
        assert_eq!(parse_size("1 KB"), None);
        assert_eq!(parse_size("lots"), None);
    }

    #[test]
    fn extensions_follow_the_format() {
        assert_eq!(
            with_extension(r"C:\a\Game.zip", Format::SevenZ),
            r"C:\a\Game.7z"
        );
        assert_eq!(
            with_extension(r"C:\a\Game.v1", Format::Zip),
            r"C:\a\Game.v1.zip"
        );
    }

    #[test]
    fn quick_names_never_collide() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("Game.zip");
        assert_eq!(free_name(dest.clone(), false), dest);
        std::fs::write(&dest, b"").unwrap();
        assert_eq!(
            free_name(dest.clone(), false),
            dir.path().join("Game (2).zip")
        );
    }
}
