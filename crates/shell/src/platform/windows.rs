//! Explorer's classic context menu, through per-user registry keys under
//! `HKCU\Software\Classes`. Archives get a HatchZip submenu under
//! `SystemFileAssociations\.<ext>`, without taking over their file
//! association; every other file and every folder gets one to make an
//! archive. Windows 11 shows the classic menu under "Show more options"; its
//! top-level entry comes from `hatchzip-shellext` and a package.

use std::path::Path;

use anyhow::{Context, Result};
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

use crate::{ACTIONS, ADD_ACTIONS, ALL_EXTENSIONS, Action, Dirs, EXTENSIONS};

const BASE: &str = r"Software\Classes\SystemFileAssociations";

/// Everything that is not an archive, and folders. A distinct verb name, so
/// Explorer never merges it with the archive submenu.
const ADD_KEYS: &[&str] = &[
    r"Software\Classes\*\shell\HatchZip.Add",
    r"Software\Classes\Directory\shell\HatchZip.Add",
];

fn menu_key(ext: &str) -> String {
    format!(r"{BASE}\.{ext}\shell\HatchZip")
}

pub fn current_dirs() -> Result<Dirs> {
    Ok(Dirs {
        data: std::env::temp_dir(),
    })
}

unsafe extern "system" {
    fn GetUserDefaultUILanguage() -> u16;
}

fn french() -> bool {
    // The primary language is the low 10 bits; 0x0C is French.
    (unsafe { GetUserDefaultUILanguage() } & 0x3ff) == 0x0c
}

fn label(a: &Action) -> &'static str {
    if french() { a.label_fr } else { a.label }
}

fn submenu<'a>(
    hkcu: &RegKey,
    path: &str,
    exe: &str,
    actions: impl Iterator<Item = &'a Action>,
) -> Result<RegKey> {
    // Items are numbered in their key names, so an older registration
    // whose numbers have shifted would leave duplicates behind.
    let _ = hkcu.delete_subkey_all(path);
    let (menu, _) = hkcu
        .create_subkey(path)
        .with_context(|| format!("creating HKCU\\{path}"))?;
    menu.set_value("MUIVerb", &"HatchZip")?;
    menu.set_value("Icon", &format!("\"{exe}\",0"))?;
    // An empty SubCommands value turns the entry into a submenu built
    // from its own `shell` key.
    menu.set_value("SubCommands", &"")?;
    for (i, a) in actions.enumerate() {
        // Explorer orders submenu items by key name.
        let (item, _) = menu.create_subkey(format!(r"shell\{:02}{}", i + 1, a.id))?;
        item.set_value("MUIVerb", &label(a))?;
        let (command, _) = item.create_subkey("command")?;
        command.set_value("", &format!("\"{exe}\" {} \"%1\"", a.flag))?;
    }
    Ok(menu)
}

pub fn register(_dirs: &Dirs, gui: &Path) -> Result<Vec<String>> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let exe = gui.to_string_lossy();
    let mut written = Vec::new();
    for ext in EXTENSIONS {
        submenu(
            &hkcu,
            &menu_key(ext),
            &exe,
            ACTIONS.iter().chain(ADD_ACTIONS),
        )?;
        written.push(format!("Explorer: .{ext} files"));
    }
    // Archives already have their own submenu with these entries in it.
    let not_archives = EXTENSIONS
        .iter()
        .map(|e| format!("System.FileExtension:=.{e}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    for (i, key) in ADD_KEYS.iter().enumerate() {
        let menu = submenu(&hkcu, key, &exe, ADD_ACTIONS.iter())?;
        if i == 0 {
            menu.set_value("AppliesTo", &format!("NOT ({not_archives})"))?;
        }
    }
    written.push("Explorer: every file and folder".into());
    Ok(written)
}

pub fn unregister(_dirs: &Dirs) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let keys = ALL_EXTENSIONS
        .iter()
        .map(|e| menu_key(e))
        .chain(ADD_KEYS.iter().map(|k| k.to_string()));
    for key in keys {
        match hkcu.delete_subkey_all(&key) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(e).with_context(|| format!("removing HKCU\\{key}"));
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn is_registered(_dirs: &Dirs) -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(menu_key(EXTENSIONS[0]))
        .is_ok()
}
