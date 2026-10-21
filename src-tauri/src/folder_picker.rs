//! Explorer-style directory picker. COM objects stay on a dedicated STA thread.

use windows::{
    core::{w, HSTRING, HRESULT},
    Win32::{
        Foundation::{ERROR_CANCELLED, HWND},
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
        },
        UI::Shell::{
            FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName,
            FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
            SIGDN_FILESYSPATH,
        },
    },
};

use crate::util::normalize_windows_path;

struct ComApartment;

impl Drop for ComApartment {
    fn drop(&mut self) {
        // Balanced with the successful initialization on this same thread.
        unsafe { CoUninitialize() };
    }
}

pub fn pick_folder(owner: isize, initial_dir: String) -> Result<Option<String>, String> {
    std::thread::Builder::new()
        .name("folder-picker".into())
        .spawn(move || {
            // A fresh thread avoids the async runtime's COM apartment state.
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
                .ok()
                .map_err(|e| format!("无法初始化文件夹选择窗口：{e}"))?;
            let _apartment = ComApartment;
            show_folder_picker(owner, &initial_dir)
                .map_err(|e| format!("无法选择保存目录：{e}"))
        })
        .map_err(|e| format!("无法启动文件夹选择窗口：{e}"))?
        .join()
        .map_err(|_| "文件夹选择窗口异常终止".to_string())?
}

fn show_folder_picker(owner: isize, initial_dir: &str) -> windows::core::Result<Option<String>> {
    // All COM calls and interfaces are confined to this initialized STA thread.
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(
            dialog.GetOptions()?
                | FOS_PICKFOLDERS
                | FOS_FORCEFILESYSTEM
                | FOS_PATHMUSTEXIST
                | FOS_NOCHANGEDIR,
        )?;
        dialog.SetTitle(w!("选择 Markdown 保存目录"))?;
        dialog.SetOkButtonLabel(w!("保存到此文件夹"))?;
        if !initial_dir.is_empty() {
            // A stale or unavailable previous directory must not block browsing.
            if let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(
                &HSTRING::from(initial_dir),
                None,
            ) {
                let _ = dialog.SetFolder(&folder);
            }
        }
        if let Err(error) = dialog.Show(Some(HWND(owner as *mut _))) {
            return if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let item = dialog.GetResult()?;
        let raw_path = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = raw_path.to_string();
        CoTaskMemFree(Some(raw_path.0.cast()));
        Ok(Some(normalize_windows_path(&path?)))
    }
}
