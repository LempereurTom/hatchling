//! The "HatchZip" entry of the Windows 11 context menu, the one shown
//! without "Show more options". Windows 11 only puts IExplorerCommand COM
//! servers there, declared by a package; `packaging/windows/sparse` holds
//! that package, which points at this DLL next to `hatchling.exe`.
//!
//! Each item only builds a command line and starts the GUI: nothing here
//! touches a file, so a bug can't take Explorer down with it beyond a
//! missing menu entry.

#![cfg(windows)]
#![allow(non_snake_case)]

use std::ffi::{OsStr, OsString, c_void};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_NOTIMPL, E_POINTER, HINSTANCE,
    HMODULE, S_FALSE, S_OK,
};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::Com::{CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::Shell::{
    ECF_DEFAULT, ECF_HASSUBCOMMANDS, ECF_ISSEPARATOR, ECS_ENABLED, ECS_HIDDEN,
    IEnumExplorerCommand, IEnumExplorerCommand_Impl, IExplorerCommand, IExplorerCommand_Impl,
    IShellItemArray, SHStrDupW, SIGDN_FILESYSPATH,
};
use windows::core::{
    BOOL, GUID, HRESULT, IUnknown, Interface, PCWSTR, PWSTR, Ref, Result, implement,
};

/// Must match `packaging/windows/sparse/AppxManifest.xml`.
pub const CLSID_ROOT: GUID = GUID::from_u128(0x19c27050_9226_4571_893a_ca15dbd956e8);

static MODULE: AtomicIsize = AtomicIsize::new(0);
static OBJECTS: AtomicUsize = AtomicUsize::new(0);

#[unsafe(no_mangle)]
extern "system" fn DllMain(module: HINSTANCE, reason: u32, _: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(module.0 as isize, Ordering::Relaxed);
    }
    BOOL(1)
}

/// The folder this DLL was loaded from, which also holds the GUI.
fn install_dir() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let mut buf = [0u16; 32768];
        let module = HMODULE(MODULE.load(Ordering::Relaxed) as *mut c_void);
        let n = unsafe { GetModuleFileNameW(Some(module), &mut buf) } as usize;
        if n == 0 {
            return None;
        }
        PathBuf::from(OsString::from_wide(&buf[..n]))
            .parent()
            .map(Path::to_path_buf)
    })
    .as_deref()
}

fn gui() -> Option<PathBuf> {
    install_dir().map(|d| d.join("hatchling.exe"))
}

fn french() -> bool {
    // The primary language is the low 10 bits; 0x0C is French.
    (unsafe { GetUserDefaultUILanguage() } & 0x3ff) == 0x0c
}

/// Whether extracting removes the archive, from the app's saved settings
/// (on unless turned off). The menu says so in the entries, since people
/// used to other extractors expect the archive to stay.
fn removes_archive() -> bool {
    let Some(appdata) = std::env::var_os("APPDATA") else {
        return true;
    };
    let file = PathBuf::from(appdata)
        .join("Hatchling")
        .join("settings.json");
    match std::fs::read_to_string(file) {
        Ok(text) => !text
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .contains("\"keep_archive\":true"),
        Err(_) => true,
    }
}

const ARCHIVE_EXTENSIONS: &[&str] = &[
    "zip", "7z", "rar", "tar", "tgz", "gz", "txz", "xz", "001", "r00",
];

/// By name only. Explorer asks for every item and every menu line while the
/// menu opens, so touching the disk here would make a large selection, or
/// one on an offline network drive, freeze the menu. A folder named like an
/// archive only gets entries that then report an error.
fn is_archive(p: &Path) -> bool {
    p.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|e| ARCHIVE_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

/// Runs `f`, turning a panic into an error. This DLL lives inside Explorer,
/// where a panic crossing back into Windows would end the Explorer process:
/// a bug here must only cost a menu entry.
fn guard<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| Err(E_FAIL.into()))
}

fn paths(items: Ref<IShellItemArray>) -> Vec<PathBuf> {
    let Some(items) = items.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    unsafe {
        let Ok(count) = items.GetCount() else {
            return out;
        };
        for i in 0..count {
            let Ok(item) = items.GetItemAt(i) else {
                continue;
            };
            let Ok(name) = item.GetDisplayName(SIGDN_FILESYSPATH) else {
                continue;
            };
            let s = name.to_string().ok();
            CoTaskMemFree(Some(name.0 as *const c_void));
            if let Some(s) = s {
                out.push(PathBuf::from(s));
            }
        }
    }
    out
}

/// The archive name WinRAR would offer: the item's own name for one item,
/// the folder they share for several.
fn archive_stem(paths: &[PathBuf]) -> String {
    let first = &paths[0];
    if paths.len() == 1 {
        let name = if first.is_dir() {
            first.file_name()
        } else {
            first.file_stem()
        };
        return name.map_or_else(|| "archive".into(), |n| n.to_string_lossy().into_owned());
    }
    first
        .parent()
        .and_then(Path::file_name)
        .map_or_else(|| "archive".into(), |n| n.to_string_lossy().into_owned())
}

/// The folder an archive goes to with "Extract to", as hatchzip-gui names it.
fn folder_stem(archive: &Path) -> String {
    let name = archive
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    for suffix in [
        ".tar.gz",
        ".tar.xz",
        ".7z.001",
        ".zip.001",
        ".part1.rar",
        ".part01.rar",
        ".part001.rar",
    ] {
        if lower.ends_with(suffix) {
            return name[..name.len() - suffix.len()].to_string();
        }
    }
    archive
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(name)
}

fn quote(arg: &OsStr) -> Vec<u16> {
    // Paths can't contain quotes on Windows, so quoting is only about
    // spaces and a trailing backslash, which would escape the closing quote.
    let mut out: Vec<u16> = vec![b'"' as u16];
    let wide: Vec<u16> = arg.encode_wide().collect();
    let trailing = wide
        .iter()
        .rev()
        .take_while(|c| **c == b'\\' as u16)
        .count();
    out.extend(&wide);
    out.extend(std::iter::repeat_n(b'\\' as u16, trailing));
    out.push(b'"' as u16);
    out
}

/// Starts the GUI. A selection too long for one command line goes through
/// a list file, which the GUI reads and deletes.
fn launch(flags: &[&str], paths: &[PathBuf]) -> Result<()> {
    let exe = gui().ok_or_else(|| windows::core::Error::from(E_FAIL))?;
    let build = |paths: &[PathBuf]| {
        let mut cmd = quote(exe.as_os_str());
        for f in flags {
            cmd.push(b' ' as u16);
            cmd.extend(OsStr::new(f).encode_wide());
        }
        for p in paths {
            cmd.push(b' ' as u16);
            cmd.extend(quote(p.as_os_str()));
        }
        cmd
    };
    let mut cmd = build(paths);
    if cmd.len() > 30000 {
        let list = std::env::temp_dir().join(format!(
            "hatchzip-{}-{}.hzlist",
            std::process::id(),
            OBJECTS.load(Ordering::Relaxed)
        ));
        let text: String = paths.iter().map(|p| format!("{}\n", p.display())).collect();
        std::fs::write(&list, text).map_err(|_| windows::core::Error::from(E_FAIL))?;
        let mut arg = OsString::from("@");
        arg.push(&list);
        cmd = build(&[PathBuf::from(arg)]);
    }
    cmd.push(0);
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    let dir: Vec<u16> = paths
        .first()
        .and_then(|p| p.parent())
        .map(|d| d.as_os_str().encode_wide().chain([0]).collect())
        .unwrap_or_else(|| vec![0]);
    unsafe {
        CreateProcessW(
            PCWSTR(
                exe.as_os_str()
                    .encode_wide()
                    .chain([0])
                    .collect::<Vec<_>>()
                    .as_ptr(),
            ),
            Some(PWSTR(cmd.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_UNICODE_ENVIRONMENT,
            None,
            if dir.len() > 1 {
                PCWSTR(dir.as_ptr())
            } else {
                PCWSTR::null()
            },
            &startup,
            &mut info,
        )?;
        let _ = windows::Win32::Foundation::CloseHandle(info.hProcess);
        let _ = windows::Win32::Foundation::CloseHandle(info.hThread);
    }
    Ok(())
}

fn dup(s: &str) -> Result<PWSTR> {
    let wide: Vec<u16> = s.encode_utf16().chain([0]).collect();
    unsafe { SHStrDupW(PCWSTR(wide.as_ptr())) }
}

fn icon() -> Result<PWSTR> {
    let exe = gui().ok_or_else(|| windows::core::Error::from(E_NOTIMPL))?;
    dup(&format!("{},0", exe.display()))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Item {
    AddDialog,
    AddZip,
    Add7z,
    AddEachZip,
    Separator,
    Open,
    ExtractFiles,
    ExtractHere,
    ExtractHereKeep,
    ExtractTo,
    Test,
}

const ITEMS: &[Item] = &[
    Item::ExtractFiles,
    Item::ExtractHere,
    Item::ExtractHereKeep,
    Item::ExtractTo,
    Item::Test,
    Item::Open,
    Item::Separator,
    Item::AddDialog,
    Item::AddZip,
    Item::Add7z,
    Item::AddEachZip,
];

impl Item {
    fn archive_only(self) -> bool {
        matches!(
            self,
            Item::Open
                | Item::ExtractFiles
                | Item::ExtractHere
                | Item::ExtractHereKeep
                | Item::ExtractTo
                | Item::Test
                | Item::Separator
        )
    }

    fn visible(self, paths: &[PathBuf]) -> bool {
        if paths.is_empty() {
            return false;
        }
        let archives = paths.iter().all(|p| is_archive(p));
        match self {
            _ if self.archive_only() && !archives => false,
            Item::Open => paths.len() == 1,
            Item::AddEachZip => paths.len() > 1,
            _ => true,
        }
    }

    fn title(self, paths: &[PathBuf]) -> String {
        let fr = french();
        let stem = if paths.is_empty() {
            String::new()
        } else {
            archive_stem(paths)
        };
        let several = paths.len() > 1;
        let removes = matches!(self, Item::ExtractHere | Item::ExtractTo) && removes_archive();
        let (suffix_fr, suffix_en) = if removes {
            (" (supprime l'archive)", " (removes the archive)")
        } else {
            ("", "")
        };
        match (self, fr) {
            (Item::AddDialog, true) => "Ajouter à l'archive…".into(),
            (Item::AddDialog, false) => "Add to archive…".into(),
            (Item::AddZip, true) => format!("Ajouter à \"{stem}.zip\""),
            (Item::AddZip, false) => format!("Add to \"{stem}.zip\""),
            (Item::Add7z, true) => format!("Ajouter à \"{stem}.7z\""),
            (Item::Add7z, false) => format!("Add to \"{stem}.7z\""),
            (Item::AddEachZip, true) => "Ajouter chaque élément à son propre .zip".into(),
            (Item::AddEachZip, false) => "Add each item to its own .zip".into(),
            (Item::Separator, _) => String::new(),
            (Item::Open, true) => "Ouvrir avec Hatchling".into(),
            (Item::Open, false) => "Open with Hatchling".into(),
            (Item::ExtractFiles, true) => "Extraire les fichiers…".into(),
            (Item::ExtractFiles, false) => "Extract files…".into(),
            (Item::ExtractHere, true) if removes => "Extraire ici (supprime l'archive)".into(),
            (Item::ExtractHere, false) if removes => "Extract here (removes the archive)".into(),
            (Item::ExtractHere, true) => "Extraire ici".into(),
            (Item::ExtractHere, false) => "Extract here".into(),
            (Item::ExtractHereKeep, true) => "Extraire ici (garder l'archive)".into(),
            (Item::ExtractHereKeep, false) => "Extract here (keep archive)".into(),
            (Item::ExtractTo, true) if several => {
                format!("Extraire chaque archive dans son dossier{suffix_fr}")
            }
            (Item::ExtractTo, false) if several => {
                format!("Extract each archive to its own folder{suffix_en}")
            }
            (Item::ExtractTo, true) => match paths.first() {
                Some(p) => format!("Extraire vers \"{}\\\"{suffix_fr}", folder_stem(p)),
                None => "Extraire vers un nouveau dossier".into(),
            },
            (Item::ExtractTo, false) => match paths.first() {
                Some(p) => format!("Extract to \"{}\\\"{suffix_en}", folder_stem(p)),
                None => "Extract to a new folder".into(),
            },
            (Item::Test, true) => "Tester l'archive".into(),
            (Item::Test, false) => "Test archive".into(),
        }
    }

    fn flags(self) -> &'static [&'static str] {
        match self {
            Item::AddDialog => &["--add"],
            Item::AddZip => &["--add-zip"],
            Item::Add7z => &["--add-7z"],
            Item::AddEachZip => &["--add-each-zip"],
            Item::Separator | Item::Open => &[],
            Item::ExtractFiles => &["--extract-files"],
            Item::ExtractHere => &["--extract-here"],
            Item::ExtractHereKeep => &["--keep-archive", "--extract-here"],
            Item::ExtractTo => &["--extract-to"],
            Item::Test => &["--test"],
        }
    }
}

#[implement(IExplorerCommand)]
struct Root;

impl IExplorerCommand_Impl for Root_Impl {
    fn GetTitle(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        dup("Hatchling")
    }
    fn GetIcon(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        icon()
    }
    fn GetToolTip(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }
    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(CLSID_ROOT)
    }
    fn GetState(&self, items: Ref<IShellItemArray>, _: BOOL) -> Result<u32> {
        guard(|| {
            Ok(if paths(items).is_empty() {
                ECS_HIDDEN.0 as u32
            } else {
                ECS_ENABLED.0 as u32
            })
        })
    }
    fn Invoke(&self, _: Ref<IShellItemArray>, _: Ref<IBindCtx>) -> Result<()> {
        Ok(())
    }
    fn GetFlags(&self) -> Result<u32> {
        Ok(ECF_HASSUBCOMMANDS.0 as u32)
    }
    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        Ok(Enum::new(0).into())
    }
}

#[implement(IExplorerCommand)]
struct Sub(Item);

impl IExplorerCommand_Impl for Sub_Impl {
    fn GetTitle(&self, items: Ref<IShellItemArray>) -> Result<PWSTR> {
        guard(|| dup(&self.0.title(&paths(items))))
    }
    fn GetIcon(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        if self.0 == Item::Separator {
            return Err(E_NOTIMPL.into());
        }
        icon()
    }
    fn GetToolTip(&self, _: Ref<IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }
    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(GUID::from_u128(
            0x3a7a4c7f_64c3_4e4c_b756_a3f8d116de00 + self.0 as u128,
        ))
    }
    fn GetState(&self, items: Ref<IShellItemArray>, _: BOOL) -> Result<u32> {
        guard(|| {
            Ok(if self.0.visible(&paths(items)) {
                ECS_ENABLED.0 as u32
            } else {
                ECS_HIDDEN.0 as u32
            })
        })
    }
    fn Invoke(&self, items: Ref<IShellItemArray>, _: Ref<IBindCtx>) -> Result<()> {
        guard(|| {
            let paths = paths(items);
            if paths.is_empty() {
                return Ok(());
            }
            launch(self.0.flags(), &paths)
        })
    }
    fn GetFlags(&self) -> Result<u32> {
        Ok(if self.0 == Item::Separator {
            ECF_ISSEPARATOR.0 as u32
        } else {
            ECF_DEFAULT.0 as u32
        })
    }
    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        Err(E_NOTIMPL.into())
    }
}

#[implement(IEnumExplorerCommand)]
struct Enum {
    next: std::cell::Cell<usize>,
}

impl Enum {
    fn new(at: usize) -> Self {
        Self {
            next: std::cell::Cell::new(at),
        }
    }
}

impl IEnumExplorerCommand_Impl for Enum_Impl {
    fn Next(&self, celt: u32, out: *mut Option<IExplorerCommand>, fetched: *mut u32) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        let mut n = 0;
        while (n as u32) < celt && self.next.get() < ITEMS.len() {
            let cmd: IExplorerCommand = Sub(ITEMS[self.next.get()]).into();
            unsafe { out.add(n).write(Some(cmd)) };
            self.next.set(self.next.get() + 1);
            n += 1;
        }
        if !fetched.is_null() {
            unsafe { *fetched = n as u32 };
        }
        if n as u32 == celt { S_OK } else { S_FALSE }
    }
    fn Skip(&self, celt: u32) -> Result<()> {
        self.next
            .set((self.next.get() + celt as usize).min(ITEMS.len()));
        Ok(())
    }
    fn Reset(&self) -> Result<()> {
        self.next.set(0);
        Ok(())
    }
    fn Clone(&self) -> Result<IEnumExplorerCommand> {
        Ok(Enum::new(self.next.get()).into())
    }
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        riid: *const GUID,
        object: *mut *mut c_void,
    ) -> Result<()> {
        if object.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *object = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        OBJECTS.fetch_add(1, Ordering::Relaxed);
        let root: IExplorerCommand = Root.into();
        unsafe { root.query(riid, object).ok() }
    }
    fn LockServer(&self, _: BOOL) -> Result<()> {
        Ok(())
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllGetClassObject(
    clsid: *const GUID,
    riid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if clsid.is_null() || riid.is_null() || out.is_null() {
        return E_POINTER;
    }
    unsafe { *out = std::ptr::null_mut() };
    if unsafe { *clsid } != CLSID_ROOT {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = Factory.into();
    unsafe { factory.query(riid, out) }
}

#[unsafe(no_mangle)]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    // Explorer keeps the DLL for its lifetime anyway; staying loaded is
    // always safe, unloading with a live object is not.
    S_FALSE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_winrar() {
        assert_eq!(
            archive_stem(&[PathBuf::from(r"C:\x\Game.v1.iso")]),
            "Game.v1"
        );
        assert_eq!(
            archive_stem(&[
                PathBuf::from(r"C:\x\Saves\a.txt"),
                PathBuf::from(r"C:\x\Saves\b.txt")
            ]),
            "Saves"
        );
        assert_eq!(folder_stem(Path::new(r"C:\x\Game.7z.001")), "Game");
        assert_eq!(folder_stem(Path::new(r"C:\x\Game.part1.rar")), "Game");
        assert_eq!(folder_stem(Path::new(r"C:\x\src.tar.gz")), "src");
        assert_eq!(folder_stem(Path::new(r"C:\x\Game.zip")), "Game");
    }

    #[test]
    fn trailing_backslash_does_not_escape_the_quote() {
        let q = String::from_utf16(&quote(OsStr::new(r"C:\a b\"))).unwrap();
        assert_eq!(q, r#""C:\a b\\""#);
    }
}
