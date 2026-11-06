//! Native Windows shell associations, icons, and file activation.
#![allow(unsafe_code)]

use std::{
    ffi::OsString,
    io,
    mem::size_of,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{RPC_E_CHANGED_MODE, SIZE},
        Graphics::Gdi::{
            CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
            BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
        },
        System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
        UI::{
            Shell::{
                AssocQueryStringW, IShellItemImageFactory, SHCreateItemFromParsingName,
                ShellExecuteExW, ASSOCF_NOTRUNCATE, ASSOCSTR, ASSOCSTR_EXECUTABLE,
                ASSOCSTR_FRIENDLYAPPNAME, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
                SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileAssociation {
    pub executable: Option<PathBuf>,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn open_path(path: impl AsRef<Path>) -> io::Result<()> {
    let path = shell_absolute_path(path.as_ref())?;
    let directory = path
        .parent()
        .map(|path| wide(path.as_os_str()))
        .transpose()?;
    let path = wide(path.as_os_str())?;
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpFile: PCWSTR(path.as_ptr()),
        lpDirectory: directory
            .as_ref()
            .map_or(PCWSTR::null(), |path| PCWSTR(path.as_ptr())),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut info) }.map_err(io::Error::other)
}

pub fn file_association(path: impl AsRef<Path>) -> io::Result<FileAssociation> {
    let extension = path
        .as_ref()
        .extension()
        .filter(|extension| !extension.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "File has no extension"))?;
    let mut association = OsString::from(".");
    association.push(extension);
    let association = wide(&association)?;
    let executable = association_string(&association, ASSOCSTR_EXECUTABLE);
    let display_name = association_string(&association, ASSOCSTR_FRIENDLYAPPNAME);
    let (executable, display_name) = match (executable, display_name) {
        (Err(error), Err(_)) => return Err(error),
        (executable, display_name) => (
            executable.unwrap_or_default(),
            display_name.unwrap_or_default(),
        ),
    };
    let association = FileAssociation {
        executable: executable
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
        display_name: display_name.filter(|value| !value.is_empty()),
    };
    if association.executable.is_none() && association.display_name.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No application is associated with this file type",
        ));
    }
    Ok(association)
}

pub fn file_icon(path: impl AsRef<Path>, size: u32) -> io::Result<ShellIcon> {
    let requested = i32::try_from(size)
        .ok()
        .filter(|size| (1..=1024).contains(size))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Icon size must be between 1 and 1024 pixels",
            )
        })?;
    let path = shell_absolute_path(path.as_ref())?;
    let path = wide(path.as_os_str())?;
    let _apartment = ComApartment::new()?;
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None) }
            .map_err(io::Error::other)?;
    let bitmap = unsafe {
        factory.GetImage(
            SIZE {
                cx: requested,
                cy: requested,
            },
            SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
        )
    }
    .map(Bitmap)
    .map_err(io::Error::other)?;
    bitmap.rgba()
}

fn association_string(association: &[u16], kind: ASSOCSTR) -> io::Result<Option<String>> {
    let mut length = 0u32;
    let result = unsafe {
        AssocQueryStringW(
            ASSOCF_NOTRUNCATE,
            kind,
            PCWSTR(association.as_ptr()),
            PCWSTR::null(),
            PWSTR::null(),
            &mut length,
        )
    };
    if length == 0 {
        return result.ok().map(|()| None).map_err(io::Error::other);
    }
    if length > 32_768 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows returned an oversized association string",
        ));
    }
    let mut buffer = vec![0u16; length as usize];
    unsafe {
        AssocQueryStringW(
            ASSOCF_NOTRUNCATE,
            kind,
            PCWSTR(association.as_ptr()),
            PCWSTR::null(),
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    }
    .ok()
    .map_err(io::Error::other)?;
    let length = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());
    String::from_utf16(&buffer[..length])
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn shell_absolute_path(path: &Path) -> io::Result<PathBuf> {
    let path = std::path::absolute(path)?;
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    const VERBATIM: &[u16] = &[92, 92, 63, 92];
    if !wide.starts_with(VERBATIM) || wide.len() == VERBATIM.len() {
        return Ok(path);
    }
    let rest = &wide[VERBATIM.len()..];
    let normalized = if rest.starts_with(&[85, 78, 67, 92]) {
        let mut unc = vec![92, 92];
        unc.extend_from_slice(&rest[4..]);
        unc
    } else {
        rest.to_vec()
    };
    Ok(PathBuf::from(OsString::from_wide(&normalized)))
}

fn wide(value: &std::ffi::OsStr) -> io::Result<Vec<u16>> {
    let mut value: Vec<u16> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows paths cannot contain NUL",
        ));
    }
    value.push(0);
    Ok(value)
}

struct ComApartment(bool);
impl ComApartment {
    fn new() -> io::Result<Self> {
        match unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) } {
            Ok(()) => Ok(Self(true)),
            Err(error) if error.code() == RPC_E_CHANGED_MODE => Ok(Self(false)),
            Err(error) => Err(io::Error::other(error)),
        }
    }
}
impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

struct Bitmap(HBITMAP);
impl Bitmap {
    fn rgba(&self) -> io::Result<ShellIcon> {
        let mut native = BITMAP::default();
        if unsafe {
            GetObjectW(
                self.0,
                size_of::<BITMAP>() as i32,
                Some((&mut native as *mut BITMAP).cast()),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if native.bmWidth <= 0
            || native.bmHeight <= 0
            || native.bmWidth > 4096
            || native.bmHeight > 4096
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned an invalid icon bitmap",
            ));
        }
        let width = native.bmWidth as u32;
        let height = native.bmHeight as u32;
        let length = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
        let mut bgra = vec![0u8; length];
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: native.bmWidth,
                biHeight: -native.bmHeight,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = DeviceContext::new(unsafe { CreateCompatibleDC(HDC::default()) })?;
        let copied = unsafe {
            GetDIBits(
                dc.0,
                self.0,
                0,
                height,
                Some(bgra.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        if copied != height as i32 {
            return Err(io::Error::last_os_error());
        }
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        if bgra.chunks_exact(4).all(|pixel| pixel[3] == 0) {
            for pixel in bgra.chunks_exact_mut(4) {
                pixel[3] = 255;
            }
        }
        Ok(ShellIcon {
            width,
            height,
            rgba: bgra,
        })
    }
}
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0);
        }
    }
}

struct DeviceContext(HDC);
impl DeviceContext {
    fn new(dc: HDC) -> io::Result<Self> {
        if dc.0 == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(dc))
        }
    }
}
impl Drop for DeviceContext {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_the_current_executables_native_icon() {
        let executable = std::env::current_exe().unwrap();
        let icon = file_icon(executable, 48).unwrap();
        assert!(icon.width > 0 && icon.height > 0);
        assert_eq!(
            icon.rgba.len(),
            icon.width as usize * icon.height as usize * 4
        );
        assert!(icon.rgba.chunks_exact(4).any(|pixel| pixel[3] != 0));
    }

    #[test]
    fn resolves_an_executable_file_association() {
        let association = file_association(std::env::current_exe().unwrap()).unwrap();
        assert!(association.executable.is_some() || association.display_name.is_some());
    }

    #[test]
    fn opens_a_real_shortcut_and_resolves_its_icon() {
        use windows::{
            core::ComInterface,
            Win32::{
                System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER},
                UI::Shell::{IShellLinkW, ShellLink},
            },
        };

        let root = std::env::temp_dir().join(format!(
            "delib-shortcut-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let shortcut = root.join("application.lnk");
        let executable = std::env::current_exe().unwrap();
        let executable = wide(executable.as_os_str()).unwrap();
        let shortcut_wide = wide(shortcut.as_os_str()).unwrap();
        let arguments = wide(std::ffi::OsStr::new(
            "--exact windows::tests::shortcut_process_marker --ignored --nocapture",
        ))
        .unwrap();
        let working_directory = wide(root.as_os_str()).unwrap();
        let _apartment = ComApartment::new().unwrap();
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(PCWSTR(executable.as_ptr())).unwrap();
            link.SetArguments(PCWSTR(arguments.as_ptr())).unwrap();
            link.SetWorkingDirectory(PCWSTR(working_directory.as_ptr()))
                .unwrap();
            link.SetIconLocation(PCWSTR(executable.as_ptr()), 0)
                .unwrap();
            let persist: IPersistFile = link.cast().unwrap();
            persist.Save(PCWSTR(shortcut_wide.as_ptr()), true).unwrap();
        }
        let icon = file_icon(&shortcut, 48).unwrap();
        assert_eq!(
            icon.rgba.len(),
            icon.width as usize * icon.height as usize * 4
        );
        assert!(icon.rgba.chunks_exact(4).any(|pixel| pixel[3] != 0));
        open_path(&shortcut).unwrap();
        let marker = root.join("delib-opened.marker");
        for _ in 0..100 {
            if marker.is_file() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(marker.is_file(), "The shortcut target did not run");
        for _ in 0..50 {
            if std::fs::remove_dir_all(&root).is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore]
    fn shortcut_process_marker() {
        std::fs::write("delib-opened.marker", std::process::id().to_string()).unwrap();
    }

    #[test]
    fn rejects_invalid_icon_sizes_and_paths() {
        assert_eq!(
            file_icon(std::env::current_exe().unwrap(), 0)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            file_association("README").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(wide(std::ffi::OsStr::new("bad\0path")).is_err());
        assert_eq!(
            shell_absolute_path(Path::new(r"\\?\C:\Windows\notepad.exe")).unwrap(),
            PathBuf::from(r"C:\Windows\notepad.exe")
        );
        assert_eq!(
            shell_absolute_path(Path::new(r"\\?\UNC\server\share\app.exe")).unwrap(),
            PathBuf::from(r"\\server\share\app.exe")
        );
    }

    #[test]
    fn opening_a_missing_shortcut_returns_an_error() {
        let path = std::env::temp_dir().join(format!(
            "delib-missing-{}-{:?}.lnk",
            std::process::id(),
            std::thread::current().id()
        ));
        assert!(!path.exists());
        assert!(open_path(path).is_err());
    }
}
