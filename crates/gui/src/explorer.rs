//! The main window: a file browser laid out like WinRAR's, with the toolbar
//! acting on the selection. Folders list from disk; double-clicking an
//! archive lists what is inside, read-only. Adding, extracting and testing
//! start the dedicated windows as their own processes, the same ones the
//! right-click menu starts, so a long run never blocks browsing.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;
use hatchzip_core::archive::Kind;
use hatchzip_core::archive::storage::Reads;
use hatchzip_core::human;
use hatchzip_core::session::{self, State};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::settings::{self, Settings};
use crate::{ExplorerWindow, FileRow, platform};

const ARCHIVE_EXTENSIONS: &[&str] = &[
    "zip", "7z", "rar", "tar", "tgz", "gz", "txz", "xz", "001", "r00",
];

fn is_archive_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ARCHIVE_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

#[derive(Clone, Debug)]
struct Entry {
    name: String,
    /// On disk, or inside the archive with `/` separators.
    path: PathBuf,
    dir: bool,
    archive: bool,
    up: bool,
    size: Option<u64>,
    modified: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq)]
enum Place {
    Roots,
    Folder(PathBuf),
    Archive(String),
}

/// An archive listed in the window.
struct Opened {
    path: PathBuf,
    parts: Vec<PathBuf>,
    format: &'static str,
    password: Option<String>,
    /// Folder inside the archive, "" for the top, to what it holds.
    tree: HashMap<String, Vec<Entry>>,
    files: usize,
    dirs: usize,
    size: u64,
    packed: u64,
    encrypted: bool,
}

fn build_tree(
    items: Vec<(String, u64, bool, Option<SystemTime>)>,
) -> (HashMap<String, Vec<Entry>>, usize, usize) {
    let mut tree: HashMap<String, Vec<Entry>> = HashMap::new();
    let mut seen_dirs: std::collections::HashSet<String> = Default::default();
    let mut files = 0;
    tree.entry(String::new()).or_default();
    let mut add_dir = |tree: &mut HashMap<String, Vec<Entry>>, path: &str, modified| {
        if path.is_empty() || !seen_dirs.insert(path.to_string()) {
            return;
        }
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        tree.entry(parent.to_string()).or_default().push(Entry {
            name: name.to_string(),
            path: PathBuf::from(path),
            dir: true,
            archive: false,
            up: false,
            size: None,
            modified,
        });
        tree.entry(path.to_string()).or_default();
    };
    for (raw, size, dir, modified) in items {
        let path: String = raw
            .replace('\\', "/")
            .split('/')
            .filter(|p| !p.is_empty() && *p != ".")
            .collect::<Vec<_>>()
            .join("/");
        if path.is_empty() {
            continue;
        }
        // Every folder on the way, even ones the archive doesn't list.
        let mut at = 0;
        while let Some(i) = path[at..].find('/') {
            add_dir(&mut tree, &path[..at + i], None);
            at += i + 1;
        }
        if dir {
            add_dir(&mut tree, &path, modified);
            continue;
        }
        files += 1;
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", &path));
        tree.entry(parent.to_string()).or_default().push(Entry {
            name: name.to_string(),
            path: PathBuf::from(&path),
            dir: false,
            archive: is_archive_name(name),
            up: false,
            size: Some(size),
            modified,
        });
    }
    let dirs = seen_dirs.len();
    (tree, files, dirs)
}

fn load_archive(archive: &Path) -> Result<Opened> {
    let (primary, paths) = session::locate(archive)?;
    let name = primary
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let reads = Arc::new(Reads::default());
    let (state, password) =
        session::open_encrypted(&primary, &paths, None, false, None, &reads, |wrong| {
            crate::password::ask(&name, wrong)
        })?;
    let job = match state {
        State::Fresh(job, _) | State::Resume(job, _) => job,
        State::Orphan(_) => anyhow::bail!(
            "Every file of this archive has already been extracted; only its leftover files remain."
        ),
    };
    let items = job
        .units
        .iter()
        .rev()
        .flat_map(|u| &u.items)
        .map(|i| {
            (
                i.path.to_string_lossy().into_owned(),
                i.size,
                matches!(i.kind, Kind::Dir),
                i.mtime,
            )
        })
        .collect();
    let (tree, files, dirs) = build_tree(items);
    let parts: Vec<PathBuf> = job.storage.parts().iter().map(|p| p.path.clone()).collect();
    let packed = job.storage.parts().iter().map(|p| p.len).sum();
    Ok(Opened {
        path: primary,
        parts,
        format: job.format_name,
        encrypted: job.encrypted(),
        password,
        tree,
        files,
        dirs,
        size: job.remaining_size(),
        packed,
    })
}

fn list_folder(dir: &Path) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let Ok(e) = e else { continue };
        let Ok(meta) = e.metadata() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        let dir = meta.is_dir();
        out.push(Entry {
            archive: !dir && is_archive_name(&name),
            name,
            path: e.path(),
            dir,
            up: false,
            size: (!dir).then_some(meta.len()),
            modified: meta.modified().ok(),
        });
    }
    Ok(out)
}

fn date(t: Option<SystemTime>) -> String {
    let Some(t) = t else { return String::new() };
    let utc = time::OffsetDateTime::from(t);
    let d = time::UtcOffset::current_local_offset()
        .map(|o| utc.to_offset(o))
        .unwrap_or(utc);
    format!(
        "{:02}/{:02}/{} {:02}:{:02}",
        d.day(),
        d.month() as u8,
        d.year(),
        d.hour(),
        d.minute()
    )
}

/// Starts another hatchzip-gui. A selection too long for one command line
/// goes through a list file, as the Windows 11 menu does.
fn spawn_self(flags: &[&str], paths: &[PathBuf]) -> Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(flags);
    let length: usize = paths.iter().map(|p| p.as_os_str().len() + 3).sum();
    if length > 30000 {
        let list = std::env::temp_dir().join(format!(
            "hatchzip-{}-{}.hzlist",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        let text: String = paths.iter().map(|p| format!("{}\n", p.display())).collect();
        std::fs::write(&list, text)?;
        let mut arg = OsString::from("@");
        arg.push(&list);
        cmd.arg(arg);
    } else {
        cmd.args(paths);
    }
    cmd.spawn()?;
    Ok(())
}

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

fn post(f: impl FnOnce(&Rc<App>) + Send + 'static) {
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = APP.with(|a| a.borrow().clone()) {
            f(&app);
        }
    });
}

enum Confirm {
    Delete(Vec<PathBuf>),
}

struct App {
    ui: slint::Weak<ExplorerWindow>,
    place: RefCell<Place>,
    /// Everything in the current place, sorted; the model shows those
    /// matching the filter.
    entries: RefCell<Vec<Entry>>,
    shown: RefCell<Vec<usize>>,
    model: Rc<VecModel<FileRow>>,
    anchor: Cell<i32>,
    cursor: Cell<i32>,
    sort: Cell<(i32, bool)>,
    opened: RefCell<Option<Opened>>,
    settings: RefCell<Settings>,
    confirm: RefCell<Option<Confirm>>,
    roots: RefCell<Vec<PathBuf>>,
    about: RefCell<Option<crate::AboutWindow>>,
    /// The open folder or archive is being read on a worker.
    generation: Cell<u64>,
}

pub fn run(path: Option<PathBuf>) -> Result<()> {
    let ui = ExplorerWindow::new()?;
    let model = Rc::new(VecModel::<FileRow>::default());
    ui.set_rows(ModelRc::from(model.clone()));
    let settings = Settings::saved();
    let app = Rc::new(App {
        ui: ui.as_weak(),
        place: RefCell::new(Place::Roots),
        entries: RefCell::new(Vec::new()),
        shown: RefCell::new(Vec::new()),
        model,
        anchor: Cell::new(-1),
        cursor: Cell::new(-1),
        sort: Cell::new((0, false)),
        opened: RefCell::new(None),
        settings: RefCell::new(settings),
        confirm: RefCell::new(None),
        roots: RefCell::new(platform::roots()),
        about: RefCell::new(None),
        generation: Cell::new(0),
    });
    APP.with(|a| *a.borrow_mut() = Some(app.clone()));
    crate::password::host_in!(ui);
    wire(&ui, &app);
    let drives: Vec<SharedString> = app
        .roots
        .borrow()
        .iter()
        .map(|p| p.display().to_string().into())
        .collect();
    ui.set_drives(ModelRc::new(VecModel::from(drives)));
    ui.set_shell_menu_on(
        hatchzip_shell::Dirs::current().is_ok_and(|d| hatchzip_shell::is_registered(&d)),
    );
    app.refresh_favorites();

    let start = path
        .map(|p| std::path::absolute(&p).unwrap_or(p))
        .or_else(|| {
            app.settings
                .borrow()
                .last_folder
                .clone()
                .filter(|p| p.is_dir())
        })
        .or_else(|| std::env::current_dir().ok());
    match start {
        Some(p) if p.is_file() => {
            if let Some(parent) = p.parent() {
                app.go(Place::Folder(parent.to_path_buf()), None);
            }
            app.open_archive(p);
        }
        Some(p) if p.is_dir() => app.go(Place::Folder(p), None),
        _ => app.go(Place::Roots, None),
    }
    ui.run()?;
    // Remember where the person was.
    let place = app.place.borrow().clone();
    let folder = match place {
        Place::Folder(p) => Some(p),
        Place::Archive(_) => app
            .opened
            .borrow()
            .as_ref()
            .and_then(|o| o.path.parent().map(Path::to_path_buf)),
        Place::Roots => None,
    };
    if let Some(f) = folder {
        let mut s = app.settings.borrow_mut();
        s.last_folder = Some(f);
        if let Some(file) = settings::file() {
            let _ = s.save_to(&file);
        }
    }
    APP.with(|a| a.borrow_mut().take());
    Ok(())
}

fn wire(ui: &ExplorerWindow, app: &Rc<App>) {
    macro_rules! on {
        ($setter:ident, |$a:ident $(, $arg:ident)*| $body:expr) => {{
            let $a = app.clone();
            ui.$setter(move |$($arg),*| $body);
        }};
    }
    on!(on_navigate, |a, t| a.navigate(t.as_str()));
    on!(on_go_up, |a| a.go_up());
    on!(on_refresh, |a| a.refresh());
    on!(on_pick_drive, |a, i| {
        let root = a.roots.borrow().get(i as usize).cloned();
        if let Some(r) = root {
            a.go(Place::Folder(r), None);
        }
    });
    on!(on_row_pressed, |a, i, ctrl, shift| a.press(i, ctrl, shift));
    on!(on_row_activated, |a, i| a.activate(i));
    on!(on_move_cursor, |a, d, shift| a.move_cursor(d, shift));
    on!(on_select_all, |a| a.select_all());
    on!(on_sort_by, |a, c| {
        let (col, desc) = a.sort.get();
        a.sort.set((c, if col == c { !desc } else { false }));
        a.resort();
    });
    on!(on_add, |a| a.add());
    on!(on_extract_to, |a| a.extract(&["--extract-files"], true));
    on!(on_extract_here, |a| a.extract(&["--extract-here"], false));
    on!(on_extract_each, |a| a.extract(&["--extract-to"], false));
    on!(on_test, |a| a.extract(&["--test"], false));
    on!(on_view, |a| a.view());
    on!(on_delete, |a| a.delete());
    on!(on_info, |a| a.info());
    on!(on_info_close, |a| a.ui().set_info_shown(false));
    on!(on_confirm_yes, |a| a.confirmed(true));
    on!(on_confirm_no, |a| a.confirmed(false));
    on!(on_open_archive, |a| a.pick_archive());
    on!(on_change_folder, |a| a.pick_folder());
    on!(on_add_favorite, |a| a.favorite(true));
    on!(on_remove_favorite, |a| a.favorite(false));
    on!(on_open_favorite, |a, i| {
        let fav = a.settings.borrow().favorites.get(i as usize).cloned();
        if let Some(f) = fav {
            a.go(Place::Folder(f), None);
        }
    });
    on!(on_shell_menu_toggled, |a| a.shell_menu());
    on!(on_filter_edited, |a, _t| a.apply_filter());
    on!(on_about, |a| a.show_about());
    on!(on_quit, |a| {
        let _ = a.ui().hide();
    });
}

impl App {
    fn ui(&self) -> ExplorerWindow {
        self.ui
            .upgrade()
            .expect("the window outlives its callbacks")
    }

    fn notify(&self, text: impl Into<SharedString>) {
        let ui = self.ui();
        ui.set_info_text(text.into());
        ui.set_info_shown(true);
    }

    fn kind_text(&self, e: &Entry) -> SharedString {
        let ui = self.ui();
        if e.up {
            return ui.invoke_up_kind();
        }
        if e.dir {
            return if matches!(*self.place.borrow(), Place::Roots) {
                ui.invoke_drive_kind()
            } else {
                ui.invoke_folder_kind()
            };
        }
        let ext = Path::new(&e.name)
            .extension()
            .map(|x| x.to_string_lossy().to_uppercase())
            .unwrap_or_default();
        if ext.is_empty() {
            ui.invoke_plain_file()
        } else if e.archive {
            ui.invoke_archive_kind(ext.into())
        } else {
            ui.invoke_file_kind(ext.into())
        }
    }

    fn row(&self, e: &Entry, selected: bool) -> FileRow {
        FileRow {
            name: e.name.as_str().into(),
            size: e.size.map(human).unwrap_or_default().into(),
            kind: self.kind_text(e),
            modified: date(e.modified).into(),
            dir: e.dir,
            archive: e.archive,
            up: e.up,
            selected,
        }
    }

    /// Shows `place`; `select` names the entry to put the cursor on.
    fn go(self: &Rc<Self>, place: Place, select: Option<String>) {
        let ui = self.ui();
        ui.set_filter("".into());
        let mut list = match &place {
            Place::Roots => self
                .roots
                .borrow()
                .iter()
                .map(|r| Entry {
                    name: r.display().to_string(),
                    path: r.clone(),
                    dir: true,
                    archive: false,
                    up: false,
                    size: None,
                    modified: None,
                })
                .collect(),
            Place::Folder(dir) => match list_folder(dir) {
                Ok(l) => l,
                Err(e) => {
                    self.notify(format!("{}: {e}", dir.display()));
                    return;
                }
            },
            Place::Archive(inner) => self
                .opened
                .borrow()
                .as_ref()
                .and_then(|o| o.tree.get(inner).cloned())
                .unwrap_or_default(),
        };
        if !matches!(place, Place::Roots) {
            list.insert(
                0,
                Entry {
                    name: "..".into(),
                    path: PathBuf::new(),
                    dir: false,
                    archive: false,
                    up: true,
                    size: None,
                    modified: None,
                },
            );
        }
        let location = match &place {
            Place::Roots => String::new(),
            Place::Folder(d) => d.display().to_string(),
            Place::Archive(inner) => {
                let o = self.opened.borrow();
                let base = o
                    .as_ref()
                    .map(|o| o.path.display().to_string())
                    .unwrap_or_default();
                if inner.is_empty() {
                    base
                } else {
                    format!("{base}\\{}", inner.replace('/', "\\"))
                }
            }
        };
        let in_archive = matches!(place, Place::Archive(_));
        ui.set_in_archive(in_archive);
        if in_archive && let Some(o) = &*self.opened.borrow() {
            ui.set_archive_title(
                o.path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
                    .into(),
            );
        }
        ui.set_location(location.into());
        *self.place.borrow_mut() = place;
        *self.entries.borrow_mut() = list;
        self.sort_entries();
        self.apply_filter();
        let cursor = select
            .and_then(|name| {
                let entries = self.entries.borrow();
                self.shown
                    .borrow()
                    .iter()
                    .position(|i| entries[*i].name == name)
            })
            .map_or(0, |i| i as i32);
        if !self.shown.borrow().is_empty() {
            self.press(cursor, false, false);
            ui.invoke_show_row(cursor);
        }
    }

    fn sort_entries(&self) {
        let (col, desc) = self.sort.get();
        let mut entries = self.entries.borrow_mut();
        entries.sort_by(|a, b| {
            use std::cmp::Ordering;
            // ".." first, then folders, then files, whatever the column.
            let rank = |e: &Entry| {
                if e.up {
                    0
                } else if e.dir {
                    1
                } else {
                    2
                }
            };
            let r = rank(a).cmp(&rank(b));
            if r != Ordering::Equal {
                return r;
            }
            let by_name = || a.name.to_lowercase().cmp(&b.name.to_lowercase());
            let o = match col {
                1 => a.size.cmp(&b.size).then_with(by_name),
                2 => {
                    let ext = |e: &Entry| {
                        Path::new(&e.name)
                            .extension()
                            .map(|x| x.to_string_lossy().to_lowercase())
                    };
                    ext(a).cmp(&ext(b)).then_with(by_name)
                }
                3 => a.modified.cmp(&b.modified).then_with(by_name),
                _ => by_name(),
            };
            if desc { o.reverse() } else { o }
        });
    }

    fn resort(self: &Rc<Self>) {
        let ui = self.ui();
        let (col, desc) = self.sort.get();
        ui.set_sort_column(col);
        ui.set_sort_descending(desc);
        let selected = self.selected_names();
        self.sort_entries();
        self.apply_filter();
        let entries = self.entries.borrow();
        for (row, i) in self.shown.borrow().iter().enumerate() {
            if selected.contains(&entries[*i].name) {
                let mut r = self.model.row_data(row).unwrap_or_default();
                r.selected = true;
                self.model.set_row_data(row, r);
            }
        }
        drop(entries);
        self.update_status();
    }

    fn apply_filter(&self) {
        let filter = self.ui().get_filter().to_lowercase();
        let entries = self.entries.borrow();
        let shown: Vec<usize> = (0..entries.len())
            .filter(|i| {
                filter.is_empty()
                    || entries[*i].up
                    || entries[*i].name.to_lowercase().contains(&filter)
            })
            .collect();
        let rows: Vec<FileRow> = shown
            .iter()
            .map(|i| self.row(&entries[*i], false))
            .collect();
        drop(entries);
        self.model.set_vec(rows);
        *self.shown.borrow_mut() = shown;
        self.cursor.set(-1);
        self.anchor.set(-1);
        self.ui().set_cursor(-1);
        self.update_status();
    }

    fn selected(&self) -> Vec<Entry> {
        let entries = self.entries.borrow();
        let shown = self.shown.borrow();
        (0..self.model.row_count())
            .filter(|r| self.model.row_data(*r).is_some_and(|row| row.selected))
            .filter_map(|r| shown.get(r).map(|i| entries[*i].clone()))
            .filter(|e| !e.up)
            .collect()
    }

    fn selected_names(&self) -> Vec<String> {
        self.selected().into_iter().map(|e| e.name).collect()
    }

    fn set_selection(&self, f: impl Fn(usize, bool) -> bool) {
        for r in 0..self.model.row_count() {
            let Some(mut row) = self.model.row_data(r) else {
                continue;
            };
            let now = f(r, row.selected);
            if row.selected != now {
                row.selected = now;
                self.model.set_row_data(r, row);
            }
        }
        self.update_status();
    }

    fn press(&self, i: i32, ctrl: bool, shift: bool) {
        if i < 0 || i as usize >= self.model.row_count() {
            return;
        }
        let anchor = if self.anchor.get() < 0 {
            i
        } else {
            self.anchor.get()
        };
        if shift {
            let (lo, hi) = (anchor.min(i) as usize, anchor.max(i) as usize);
            self.set_selection(|r, was| (ctrl && was) || (lo..=hi).contains(&r));
        } else if ctrl {
            self.set_selection(|r, was| if r == i as usize { !was } else { was });
            self.anchor.set(i);
        } else {
            self.set_selection(|r, _| r == i as usize);
            self.anchor.set(i);
        }
        self.cursor.set(i);
        self.ui().set_cursor(i);
    }

    fn move_cursor(&self, delta: i32, shift: bool) {
        let n = self.model.row_count() as i32;
        if n == 0 {
            return;
        }
        let at = (self.cursor.get().max(0) as i64 + delta as i64).clamp(0, n as i64 - 1) as i32;
        self.press(at, false, shift);
        self.ui().invoke_show_row(at);
    }

    fn select_all(&self) {
        let shown = self.shown.borrow().clone();
        let entries = self.entries.borrow();
        let ups: Vec<bool> = shown.iter().map(|i| entries[*i].up).collect();
        drop(entries);
        self.set_selection(|r, _| !ups.get(r).copied().unwrap_or(true));
    }

    fn update_status(&self) {
        let ui = self.ui();
        let entries = self.entries.borrow();
        let all: Vec<&Entry> = self
            .shown
            .borrow()
            .iter()
            .map(|i| &entries[*i])
            .filter(|e| !e.up)
            .collect();
        let dirs = all.iter().filter(|e| e.dir).count();
        let files: Vec<&&Entry> = all.iter().filter(|e| !e.dir).collect();
        let size: u64 = files.iter().filter_map(|e| e.size).sum();
        ui.set_status_right(ui.invoke_totals(
            dirs.to_string().into(),
            human(size).into(),
            files.len().to_string().into(),
        ));
        drop(entries);
        let sel = self.selected();
        let sel_files: Vec<&Entry> = sel.iter().filter(|e| !e.dir).collect();
        ui.set_status_left(if sel.is_empty() {
            "".into()
        } else {
            ui.invoke_selected_text(
                human(sel_files.iter().filter_map(|e| e.size).sum()).into(),
                sel_files.len().to_string().into(),
            )
        });
        let place = self.place.borrow().clone();
        let in_archive = matches!(place, Place::Archive(_));
        let in_folder = matches!(place, Place::Folder(_));
        let archives = !sel.is_empty() && sel.iter().all(|e| e.archive);
        ui.set_can_add(in_folder && !sel.is_empty());
        ui.set_can_extract(in_archive || (in_folder && archives));
        ui.set_can_test(in_archive || (in_folder && archives));
        ui.set_can_view(sel.len() == 1 && !sel[0].dir);
        ui.set_can_delete(in_folder && !sel.is_empty());
        ui.set_can_info(in_archive || !sel.is_empty());
    }

    fn activate(self: &Rc<Self>, i: i32) {
        let entry = {
            let entries = self.entries.borrow();
            self.shown
                .borrow()
                .get(i as usize)
                .map(|i| entries[*i].clone())
        };
        let Some(e) = entry else { return };
        if e.up {
            return self.go_up();
        }
        let place = self.place.borrow().clone();
        match place {
            Place::Roots => self.go(Place::Folder(e.path), None),
            Place::Folder(_) if e.dir => self.go(Place::Folder(e.path), None),
            Place::Folder(_) if e.archive => self.open_archive(e.path),
            Place::Folder(_) => {
                if let Err(err) = open::that_detached(&e.path) {
                    self.notify(format!("{err}"));
                }
            }
            Place::Archive(_) if e.dir => {
                self.go(Place::Archive(e.path.to_string_lossy().into_owned()), None)
            }
            Place::Archive(_) => self.view(),
        }
    }

    fn go_up(self: &Rc<Self>) {
        let place = self.place.borrow().clone();
        match place {
            Place::Roots => {}
            Place::Folder(dir) => match dir.parent() {
                Some(parent) => {
                    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
                    self.go(Place::Folder(parent.to_path_buf()), name);
                }
                None => self.go(Place::Roots, Some(dir.display().to_string())),
            },
            Place::Archive(inner) if inner.is_empty() => {
                let path = self.opened.borrow().as_ref().map(|o| o.path.clone());
                if let Some(p) = path
                    && let Some(parent) = p.parent()
                {
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                    self.go(Place::Folder(parent.to_path_buf()), name);
                }
            }
            Place::Archive(inner) => {
                let (parent, name) = inner.rsplit_once('/').unwrap_or(("", &inner));
                self.go(Place::Archive(parent.to_string()), Some(name.to_string()));
            }
        }
    }

    fn navigate(self: &Rc<Self>, text: &str) {
        let p = PathBuf::from(text.trim().trim_matches('"'));
        if p.is_dir() {
            self.go(Place::Folder(std::path::absolute(&p).unwrap_or(p)), None);
        } else if p.is_file() {
            self.open_archive(p);
        } else if text.trim().is_empty() {
            self.go(Place::Roots, None);
        } else {
            self.notify(format!("{}: not found", p.display()));
        }
    }

    fn refresh(self: &Rc<Self>) {
        let place = self.place.borrow().clone();
        let cursor_name = self.selected_names().into_iter().next();
        self.go(place, cursor_name);
    }

    fn open_archive(self: &Rc<Self>, path: PathBuf) {
        let ui = self.ui();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        ui.set_busy(true);
        ui.set_busy_text(ui.invoke_opening(name.into()));
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        std::thread::spawn(move || {
            let result = load_archive(&path);
            post(move |app| {
                if app.generation.get() != generation {
                    return;
                }
                app.ui().set_busy(false);
                match result {
                    Ok(opened) => {
                        *app.opened.borrow_mut() = Some(opened);
                        app.go(Place::Archive(String::new()), None);
                    }
                    Err(e) => app.notify(crate::work::sentence(&format!("{e:#}"))),
                }
            });
        });
    }

    /// The archives the command applies to: the open one, or those selected.
    fn target_archives(&self) -> Vec<PathBuf> {
        if matches!(*self.place.borrow(), Place::Archive(_)) {
            return self
                .opened
                .borrow()
                .as_ref()
                .map(|o| vec![o.path.clone()])
                .unwrap_or_default();
        }
        self.selected()
            .into_iter()
            .filter(|e| e.archive)
            .map(|e| e.path)
            .collect()
    }

    fn add(&self) {
        let sel: Vec<PathBuf> = self.selected().into_iter().map(|e| e.path).collect();
        if sel.is_empty() || !matches!(*self.place.borrow(), Place::Folder(_)) {
            let ui = self.ui();
            return self.notify(ui.invoke_select_first());
        }
        if let Err(e) = spawn_self(&["--add"], &sel) {
            self.notify(format!("{e:#}"));
        }
    }

    /// `dialog`: with one archive, open the full extraction window, the one
    /// with the space plan and the options, as WinRAR's "Extract To" does.
    fn extract(&self, flags: &[&str], dialog: bool) {
        let archives = self.target_archives();
        if archives.is_empty() {
            let ui = self.ui();
            return self.notify(ui.invoke_select_archive());
        }
        let result = if dialog && archives.len() == 1 {
            spawn_self(&["--extract-dialog"], &archives)
        } else {
            spawn_self(flags, &archives)
        };
        if let Err(e) = result {
            self.notify(format!("{e:#}"));
        }
    }

    fn view(&self) {
        let sel = self.selected();
        let Some(e) = sel.first().filter(|e| !e.dir).cloned() else {
            let ui = self.ui();
            return self.notify(ui.invoke_view_unsupported());
        };
        if !matches!(*self.place.borrow(), Place::Archive(_)) {
            if let Err(err) = open::that_detached(&e.path) {
                self.notify(format!("{err}"));
            }
            return;
        }
        let Some((parts, format, password)) = self
            .opened
            .borrow()
            .as_ref()
            .map(|o| (o.parts.clone(), o.format, o.password.clone()))
        else {
            return;
        };
        let inner = e.path.to_string_lossy().replace('\\', "/");
        // A folder of its own per view, so two files of the same name from
        // different folders never overwrite each other.
        let dir = std::env::temp_dir().join("Hatchling View").join(format!(
            "{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis())
        ));
        let ui = self.ui();
        ui.set_busy(true);
        ui.set_busy_text(ui.invoke_opening(e.name.clone().into()));
        std::thread::spawn(move || {
            let result = hatchzip_core::peek::extract_entry(
                &parts,
                format,
                &inner,
                password.as_deref(),
                &dir,
            )
            .and_then(|f| open::that_detached(&f).map_err(Into::into));
            post(move |app| {
                app.ui().set_busy(false);
                if let Err(e) = result {
                    app.notify(crate::work::sentence(&format!("{e:#}")));
                }
            });
        });
    }

    fn delete(&self) {
        if !matches!(*self.place.borrow(), Place::Folder(_)) {
            return;
        }
        let paths: Vec<PathBuf> = self.selected().into_iter().map(|e| e.path).collect();
        if paths.is_empty() {
            return;
        }
        let ui = self.ui();
        ui.set_confirm_text(ui.invoke_confirm_delete(paths.len().to_string().into()));
        ui.set_confirm_shown(true);
        *self.confirm.borrow_mut() = Some(Confirm::Delete(paths));
    }

    fn confirmed(self: &Rc<Self>, yes: bool) {
        self.ui().set_confirm_shown(false);
        let Some(c) = self.confirm.borrow_mut().take() else {
            return;
        };
        if !yes {
            return;
        }
        match c {
            Confirm::Delete(paths) => {
                if let Err(e) = platform::recycle(&paths) {
                    self.notify(format!("{e:#}"));
                }
                self.refresh();
            }
        }
    }

    fn info(&self) {
        let ui = self.ui();
        if matches!(*self.place.borrow(), Place::Archive(_)) {
            if let Some(o) = &*self.opened.borrow() {
                let ratio = (o.packed * 100).checked_div(o.size).unwrap_or(100);
                let yes_no = if o.encrypted {
                    ui.invoke_yes_text()
                } else {
                    ui.invoke_no_text()
                };
                self.notify(ui.invoke_info_archive(
                    o.path.display().to_string().into(),
                    o.format.to_uppercase().into(),
                    o.files.to_string().into(),
                    o.dirs.to_string().into(),
                    human(o.size).into(),
                    human(o.packed).into(),
                    ratio.to_string().into(),
                    yes_no,
                ));
            }
            return;
        }
        let sel = self.selected();
        if sel.is_empty() {
            return;
        }
        ui.set_busy(true);
        let n = sel.len();
        std::thread::spawn(move || {
            // Folders are counted all the way down, as Explorer's Properties do.
            let (mut files, mut dirs, mut size) = (0usize, 0usize, 0u64);
            let mut stack: Vec<PathBuf> = Vec::new();
            for e in &sel {
                if e.dir {
                    stack.push(e.path.clone());
                } else {
                    files += 1;
                    size += e.size.unwrap_or(0);
                }
            }
            while let Some(d) = stack.pop() {
                dirs += 1;
                let Ok(rd) = std::fs::read_dir(&d) else {
                    continue;
                };
                for c in rd.flatten() {
                    let Ok(m) = std::fs::symlink_metadata(c.path()) else {
                        continue;
                    };
                    if m.is_dir() {
                        stack.push(c.path());
                    } else {
                        files += 1;
                        size += m.len();
                    }
                }
            }
            post(move |app| {
                let ui = app.ui();
                ui.set_busy(false);
                app.notify(ui.invoke_info_items(
                    n.to_string().into(),
                    human(size).into(),
                    files.to_string().into(),
                    dirs.to_string().into(),
                ));
            });
        });
    }

    fn pick_archive(self: &Rc<Self>) {
        let ui = self.ui();
        let mut d = rfd::FileDialog::new()
            .set_title(ui.invoke_open_archive_title().as_str())
            .add_filter(ui.invoke_archives_filter().as_str(), ARCHIVE_EXTENSIONS)
            .add_filter(ui.invoke_all_files_filter().as_str(), &["*"]);
        if let Place::Folder(dir) = &*self.place.borrow() {
            d = d.set_directory(dir);
        }
        if let Some(p) = d.pick_file() {
            if let Some(parent) = p.parent() {
                self.go(
                    Place::Folder(parent.to_path_buf()),
                    p.file_name().map(|n| n.to_string_lossy().into_owned()),
                );
            }
            self.open_archive(p);
        }
    }

    fn pick_folder(self: &Rc<Self>) {
        let ui = self.ui();
        let mut d = rfd::FileDialog::new().set_title(ui.invoke_change_folder_title().as_str());
        if let Place::Folder(dir) = &*self.place.borrow() {
            d = d.set_directory(dir);
        }
        if let Some(p) = d.pick_folder() {
            self.go(Place::Folder(p), None);
        }
    }

    fn refresh_favorites(&self) {
        let favs: Vec<SharedString> = self
            .settings
            .borrow()
            .favorites
            .iter()
            .map(|p| p.display().to_string().into())
            .collect();
        self.ui().set_favorites(ModelRc::new(VecModel::from(favs)));
    }

    fn favorite(&self, add: bool) {
        let Place::Folder(dir) = self.place.borrow().clone() else {
            return;
        };
        {
            let mut s = self.settings.borrow_mut();
            s.favorites.retain(|f| f != &dir);
            if add {
                s.favorites.push(dir);
            }
            if let Some(f) = settings::file() {
                s.save(&f);
            }
        }
        self.refresh_favorites();
    }

    fn shell_menu(&self) {
        let on = self.ui().get_shell_menu_on();
        std::thread::spawn(move || {
            let result = crate::shell::set_menu(on);
            post(move |app| {
                let ui = app.ui();
                match result {
                    Ok(()) => app.notify(if on {
                        ui.invoke_menu_added()
                    } else {
                        ui.invoke_menu_removed()
                    }),
                    Err(e) => {
                        ui.set_shell_menu_on(!on);
                        app.notify(format!("{e:#}"));
                    }
                }
            });
        });
    }

    fn show_about(&self) {
        let mut about = self.about.borrow_mut();
        if about.is_none() {
            let Ok(w) = crate::AboutWindow::new() else {
                return;
            };
            w.set_version(env!("CARGO_PKG_VERSION").into());
            w.on_open_link(|| {
                let _ = open::that_detached("https://github.com/LempereurTom/hatchling");
            });
            let weak = w.as_weak();
            w.on_dismiss(move || {
                if let Some(w) = weak.upgrade() {
                    let _ = w.hide();
                }
            });
            *about = Some(w);
        }
        if let Some(w) = about.as_ref() {
            let _ = w.show();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_has_every_folder_once() {
        let (tree, files, dirs) = build_tree(vec![
            ("Game/data/a.bin".into(), 3, false, None),
            ("Game/data/".into(), 0, true, None),
            ("Game\\readme.txt".into(), 1, false, None),
            ("top.zip".into(), 9, false, None),
        ]);
        assert_eq!(files, 3);
        assert_eq!(dirs, 2);
        let names = |k: &str| {
            let mut v: Vec<String> = tree[k].iter().map(|e| e.name.clone()).collect();
            v.sort();
            v
        };
        assert_eq!(names(""), ["Game", "top.zip"]);
        assert_eq!(names("Game"), ["data", "readme.txt"]);
        assert_eq!(names("Game/data"), ["a.bin"]);
        assert!(
            tree[""]
                .iter()
                .find(|e| e.name == "top.zip")
                .unwrap()
                .archive
        );
    }
}
