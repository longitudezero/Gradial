use std::path::PathBuf;

use windows::{
    Win32::{
        Foundation::{ERROR_CANCELLED, RPC_E_CHANGED_MODE},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
        },
        UI::Shell::{
            Common::COMDLG_FILTERSPEC, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR,
            FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FOS_STRICTFILETYPES,
            FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog,
            SIGDN_FILESYSPATH,
        },
    },
    core::{HRESULT, PCWSTR, w},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PathDialogKind {
    File,
    Folder,
    Executable,
    Icon,
    ConfigurationImport,
    ConfigurationExport,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PathDialogOutcome {
    Selected(PathBuf),
    Cancelled,
    Failed,
}

pub(crate) fn choose_path(kind: PathDialogKind) -> PathDialogOutcome {
    let Ok(dialog_thread) = std::thread::Builder::new()
        .name("gradial-file-dialog".to_owned())
        .spawn(move || choose_path_on_sta_thread(kind))
    else {
        return PathDialogOutcome::Failed;
    };
    dialog_thread.join().unwrap_or(PathDialogOutcome::Failed)
}

fn choose_path_on_sta_thread(kind: PathDialogKind) -> PathDialogOutcome {
    let Ok(_apartment) = ComApartment::initialize() else {
        return PathDialogOutcome::Failed;
    };
    let result = show_dialog(kind);
    match result {
        Ok(path) => PathDialogOutcome::Selected(path),
        Err(error) if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => {
            PathDialogOutcome::Cancelled
        }
        Err(_) => PathDialogOutcome::Failed,
    }
}

fn show_dialog(kind: PathDialogKind) -> windows::core::Result<PathBuf> {
    if kind == PathDialogKind::ConfigurationExport {
        return show_save_dialog();
    }
    show_open_dialog(kind)
}

fn show_open_dialog(kind: PathDialogKind) -> windows::core::Result<PathBuf> {
    // SAFETY: COM is initialized for this dedicated STA thread. The returned
    // interface and all derived shell items remain on this thread.
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)? };
    let file_dialog: &IFileDialog = &dialog;
    let mut options = FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR | FOS_PATHMUSTEXIST;
    if kind == PathDialogKind::Folder {
        options |= FOS_PICKFOLDERS;
    } else {
        options |= FOS_FILEMUSTEXIST;
    }
    if matches!(
        kind,
        PathDialogKind::Executable | PathDialogKind::Icon | PathDialogKind::ConfigurationImport
    ) {
        options |= FOS_STRICTFILETYPES;
    }

    // SAFETY: the dialog interface is valid on this STA thread, and all
    // strings and filter specifications live through their synchronous calls.
    unsafe {
        file_dialog.SetOptions(options)?;
        file_dialog.SetTitle(dialog_title(kind))?;
        if kind == PathDialogKind::Executable {
            file_dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
                pszName: w!("Applications"),
                pszSpec: w!("*.exe"),
            }])?;
            file_dialog.SetFileTypeIndex(1)?;
        } else if kind == PathDialogKind::Icon {
            file_dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
                pszName: w!("PNG or SVG icons"),
                pszSpec: w!("*.png;*.svg"),
            }])?;
            file_dialog.SetFileTypeIndex(1)?;
        } else if kind == PathDialogKind::ConfigurationImport {
            set_configuration_filter(file_dialog)?;
        }
        dialog.Show(None)?;
    }

    // SAFETY: GetResult returns an item owned by the dialog result. The
    // display-name buffer is allocated with the COM task allocator and freed
    // exactly once after conversion.
    let display_name = unsafe { file_dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)? };
    let path_result = unsafe { display_name.to_string() }.map(PathBuf::from);
    unsafe {
        CoTaskMemFree(Some(display_name.0.cast()));
    }
    Ok(path_result?)
}

fn show_save_dialog() -> windows::core::Result<PathBuf> {
    // SAFETY: COM is initialized for this dedicated STA thread. The returned
    // interface and all derived shell items remain on this thread.
    let dialog: IFileSaveDialog =
        unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)? };
    let file_dialog: &IFileDialog = &dialog;
    let options = FOS_FORCEFILESYSTEM
        | FOS_NOCHANGEDIR
        | FOS_PATHMUSTEXIST
        | FOS_OVERWRITEPROMPT
        | FOS_STRICTFILETYPES;

    // SAFETY: the dialog interface is valid on this STA thread, and all
    // strings and filter specifications live through their synchronous calls.
    unsafe {
        file_dialog.SetOptions(options)?;
        file_dialog.SetTitle(dialog_title(PathDialogKind::ConfigurationExport))?;
        set_configuration_filter(file_dialog)?;
        file_dialog.SetDefaultExtension(w!("json"))?;
        file_dialog.SetFileName(w!("gradial-settings.json"))?;
        dialog.Show(None)?;
    }

    // SAFETY: GetResult returns an item owned by the dialog result. The
    // display-name buffer is allocated with the COM task allocator and freed
    // exactly once after conversion.
    let display_name = unsafe { file_dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)? };
    let path_result = unsafe { display_name.to_string() }.map(PathBuf::from);
    unsafe {
        CoTaskMemFree(Some(display_name.0.cast()));
    }
    Ok(path_result?)
}

unsafe fn set_configuration_filter(file_dialog: &IFileDialog) -> windows::core::Result<()> {
    // SAFETY: the caller guarantees the COM interface is valid on its owning
    // STA thread, and these static strings outlive the synchronous calls.
    unsafe {
        file_dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
            pszName: w!("Gradial settings"),
            pszSpec: w!("*.json"),
        }])?;
        file_dialog.SetFileTypeIndex(1)
    }
}

const fn dialog_title(kind: PathDialogKind) -> PCWSTR {
    match kind {
        PathDialogKind::File => w!("Gradial: choose a file"),
        PathDialogKind::Folder => w!("Gradial: choose a folder"),
        PathDialogKind::Executable => w!("Gradial: choose an application"),
        PathDialogKind::Icon => w!("Gradial: choose an icon"),
        PathDialogKind::ConfigurationImport => w!("Gradial: import settings"),
        PathDialogKind::ConfigurationExport => w!("Gradial: export settings"),
    }
}

#[derive(Debug)]
struct ComApartment {
    uninitialize: bool,
}

impl ComApartment {
    fn initialize() -> windows::core::Result<Self> {
        // SAFETY: this is the first work on a new dedicated thread. A
        // successful initialization is balanced by Drop on the same thread.
        let result =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if result.is_ok() {
            Ok(Self { uninitialize: true })
        } else if result == RPC_E_CHANGED_MODE {
            Ok(Self { uninitialize: false })
        } else {
            Err(result.into())
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            // SAFETY: initialization succeeded on this thread and this is its
            // one matching uninitialization.
            unsafe {
                CoUninitialize();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PathDialogKind, dialog_title};

    #[test]
    fn every_dialog_kind_has_a_nonempty_title() {
        for kind in [
            PathDialogKind::File,
            PathDialogKind::Folder,
            PathDialogKind::Executable,
            PathDialogKind::Icon,
            PathDialogKind::ConfigurationImport,
            PathDialogKind::ConfigurationExport,
        ] {
            assert!(!dialog_title(kind).is_null());
        }
    }
}
