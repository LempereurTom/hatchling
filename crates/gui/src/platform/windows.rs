use std::path::PathBuf;

/// The settings switch, named after this system's file manager.
pub const SHELL_MENU_NAME: super::ShellMenuName = super::ShellMenuName::Explorer;

/// Explorer, through the per-user registry hive.
pub const SHELL_MENU: bool = true;

/// `%APPDATA%\Hatchling`, which roams with the user's profile.
pub fn config_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(appdata).join("Hatchling"))
}

/// Explorer passes the files as arguments here, so there is nothing to
/// listen for.
pub fn forward_opened_files(_on_open: impl Fn(PathBuf) + 'static) {}

/// The drive letters that exist, for the browser's drive list.
pub fn roots() -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .map(|l| PathBuf::from(format!("{}:\\", l as char)))
        .filter(|p| p.exists())
        .collect()
}

/// Moves files and folders to the Recycle Bin, so a delete from the browser
/// can be undone the way it can in Explorer.
pub fn recycle(paths: &[PathBuf]) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_SILENT, FOF_WANTNUKEWARNING,
        SHFILEOPSTRUCTW, SHFileOperationW,
    };
    if paths.is_empty() {
        return Ok(());
    }
    // SHFileOperation reads relative paths against the current folder.
    if let Some(p) = paths
        .iter()
        .find(|p| !p.is_absolute() || p.parent().is_none())
    {
        anyhow::bail!(
            "Refusing to delete {}: not a file or folder inside a drive.",
            p.display()
        );
    }
    // A list of NUL-terminated paths, ended by an empty one.
    let mut from: Vec<u16> = Vec::new();
    for p in paths {
        from.extend(p.as_os_str().encode_wide());
        from.push(0);
    }
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        // The browser already asked. Where there is no Recycle Bin (USB
        // sticks, network drives) Windows would delete for good without a
        // word; FOF_WANTNUKEWARNING makes it ask first in that case.
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_WANTNUKEWARNING) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 {
        anyhow::bail!("Windows could not move the files to the Recycle Bin (error {r:#x}).");
    }
    Ok(())
}
