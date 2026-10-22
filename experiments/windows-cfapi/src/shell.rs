//! Shell registration for explicitly requested generated roots only.
//! No account, provider protocol, existing root adoption or content access.
use sha2::{Digest, Sha256};
use std::{
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
};
use windows::{
    Security::Cryptography::CryptographicBuffer,
    Storage::{Provider::*, StorageFolder},
    Win32::{
        Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::{
            Threading::{GetCurrentProcess, OpenProcessToken},
            WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
        },
    },
    core::{HSTRING, Result},
};
const CONTEXT: &[u8] = b"twodrive-generated-shell-v1";

struct Apartment(PhantomData<Rc<()>>);
impl Apartment {
    fn new() -> Result<Self> {
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)?;
        }
        Ok(Self(PhantomData))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() }
    }
}

fn current_sid() -> Result<String> {
    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
    }
    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
    let token = Token(token);
    let mut needed = 0;
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut needed) };
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )?;
    }
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = windows::core::PWSTR::null();
    unsafe {
        ConvertSidToStringSidW(user.User.Sid, &mut text)?;
    }
    let result = unsafe { text.to_string() };
    unsafe {
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
    }
    result.map_err(|_| {
        windows::core::Error::from_hresult(windows::core::HRESULT(0x80070057u32 as i32))
    })
}

fn registration_id(sid: &str, path: &Path) -> HSTRING {
    // Match the documented provider!SID!account shape. The account component
    // identifies this generated root, never a cloud account or private label.
    let path = folder_path(path);
    let mut hash = Sha256::new();
    for unit in path.iter() {
        hash.update(unit.to_le_bytes());
    }
    let digest = hash.finalize();
    HSTRING::from(format!("TwoDriveGenerated!{sid}!{:x}", digest))
}

fn folder_path(path: &Path) -> HSTRING {
    let value = HSTRING::from(path);
    // RootGuard produces a Win32 verbatim path; the WinRT folder broker
    // requires the equivalent DOS path. Keep the UTF-16 spelling intact.
    let prefix = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    if value.starts_with(&prefix) {
        HSTRING::from_wide(&value[4..])
    } else {
        value
    }
}

pub(crate) struct Registration {
    id: HSTRING,
    root: PathBuf,
    // WinRT initialization must be released on its creating thread.
    _apartment: Apartment,
}
impl Registration {
    pub(crate) fn create(root: &Path) -> std::result::Result<Self, String> {
        Self::create_inner(root).map_err(|e| {
            format!(
                "register generated Shell root: HRESULT 0x{:08X}",
                e.code().0 as u32
            )
        })
    }
    fn create_inner(root: &Path) -> Result<Self> {
        let apartment = Apartment::new()?;
        let id = registration_id(&current_sid()?, root);
        for existing in StorageProviderSyncRootManager::GetCurrentSyncRoots()? {
            if existing.Id()? == id {
                return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                    0x800700B7u32 as i32,
                )));
            }
        }
        let info = StorageProviderSyncRootInfo::new()?;
        info.SetId(&id)?;
        info.SetPath(&StorageFolder::GetFolderFromPathAsync(&folder_path(root))?.join()?)?;
        info.SetDisplayNameResource(&HSTRING::from("TwoDrive generated verification"))?;
        info.SetIconResource(&HSTRING::from("%SystemRoot%\\system32\\shell32.dll,0"))?;
        info.SetVersion(&HSTRING::from(env!("CARGO_PKG_VERSION")))?;
        info.SetHydrationPolicy(StorageProviderHydrationPolicy::Full)?;
        info.SetHydrationPolicyModifier(StorageProviderHydrationPolicyModifier::None)?;
        info.SetPopulationPolicy(StorageProviderPopulationPolicy::AlwaysFull)?;
        info.SetInSyncPolicy(StorageProviderInSyncPolicy(
            windows_sys::Win32::Storage::CloudFilters::CF_INSYNC_POLICY_TRACK_ALL,
        ))?;
        info.SetHardlinkPolicy(StorageProviderHardlinkPolicy::None)?;
        info.SetShowSiblingsAsGroup(false)?;
        info.SetContext(&CryptographicBuffer::CreateFromByteArray(CONTEXT)?)?;
        let registration = Self {
            id,
            root: root.to_owned(),
            _apartment: apartment,
        };
        if let Err(error) = StorageProviderSyncRootManager::Register(&info) {
            // A failed OS call can have partially published registration. Only
            // remove an entry whose identity, exact root and marker are ours.
            if let Ok(existing) =
                StorageProviderSyncRootManager::GetSyncRootInformationForId(&registration.id)
                && registration.owns(&existing).unwrap_or(false)
                && let Err(cleanup) = StorageProviderSyncRootManager::Unregister(&registration.id)
            {
                eprintln!(
                    "generated Shell registration rollback incomplete: HRESULT 0x{:08X}",
                    cleanup.code().0 as u32
                );
            }
            return Err(error);
        }
        Ok(registration)
    }
    fn owns(&self, info: &StorageProviderSyncRootInfo) -> Result<bool> {
        Ok(info.Id()? == self.id
            && info.Path()?.Path()? == folder_path(&self.root)
            && CryptographicBuffer::Compare(
                &info.Context()?,
                &CryptographicBuffer::CreateFromByteArray(CONTEXT)?,
            )?)
    }
    pub(crate) fn verify(&self) -> std::result::Result<(), String> {
        let check = || -> Result<bool> {
            let info = StorageProviderSyncRootManager::GetSyncRootInformationForId(&self.id)?;
            let folder = StorageProviderSyncRootManager::GetSyncRootInformationForFolder(
                &StorageFolder::GetFolderFromPathAsync(&folder_path(self.root.as_path()))?
                    .join()?,
            )?;
            Ok(self.owns(&info)? && self.owns(&folder)?)
        };
        match check() {
            Ok(true) => Ok(()),
            Ok(false) => Err("generated Shell registration identity mismatch".into()),
            Err(e) => Err(format!(
                "query generated Shell registration: HRESULT 0x{:08X}",
                e.code().0 as u32
            )),
        }
    }
    pub(crate) fn unregister(&self) -> std::result::Result<(), String> {
        self.verify()?;
        StorageProviderSyncRootManager::Unregister(&self.id).map_err(|e| {
            format!(
                "unregister generated Shell root: HRESULT 0x{:08X}",
                e.code().0 as u32
            )
        })
    }
}

pub(crate) fn verify_absent(root: &Path) -> std::result::Result<(), String> {
    let query = || -> Result<bool> {
        let _apartment = Apartment::new()?;
        let id = registration_id(&current_sid()?, root);
        for existing in StorageProviderSyncRootManager::GetCurrentSyncRoots()? {
            if existing.Id()? == id {
                return Ok(false);
            }
        }
        Ok(true)
    };
    match query() {
        Ok(true) => Ok(()),
        Ok(false) => Err("generated Shell root remains registered".into()),
        Err(e) => Err(format!(
            "query Shell cleanup: HRESULT 0x{:08X}",
            e.code().0 as u32
        )),
    }
}

/// Read the Shell property store without requesting file content.
pub(crate) fn placeholder_status(path: &Path) -> std::result::Result<u32, String> {
    let query = || -> Result<u32> {
        let _apartment = Apartment::new()?;
        let item: windows::Win32::UI::Shell::IShellItem2 = unsafe {
            windows::Win32::UI::Shell::SHCreateItemFromParsingName(&folder_path(path), None)?
        };
        unsafe {
            item.GetUInt32(&windows::Win32::Storage::EnhancedStorage::PKEY_FilePlaceholderStatus)
        }
    };
    query().map_err(|e| {
        format!(
            "query Shell placeholder status: HRESULT 0x{:08X}",
            e.code().0 as u32
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn winrt_folder_path_preserves_unicode_and_removes_local_verbatim_prefix() {
        assert_eq!(
            folder_path(Path::new(r"\\?\C:\generated-中文")),
            HSTRING::from(r"C:\generated-中文")
        );
        assert_eq!(
            folder_path(Path::new(r"C:\generated-中文")),
            HSTRING::from(r"C:\generated-中文")
        );
    }
    #[test]
    fn generated_registration_identity_is_stable_and_root_specific() {
        let a = registration_id("S-1-5-21-1", Path::new(r"C:\generated-a"));
        assert_eq!(
            a,
            registration_id("S-1-5-21-1", Path::new(r"C:\generated-a"))
        );
        assert_eq!(
            a,
            registration_id("S-1-5-21-1", Path::new(r"\\?\C:\generated-a"))
        );
        assert_ne!(
            a,
            registration_id("S-1-5-21-1", Path::new(r"C:\generated-b"))
        );
        assert_ne!(
            a,
            registration_id("S-1-5-21-2", Path::new(r"C:\generated-a"))
        );
        assert!(!a.to_string().contains("generated-a"));
    }
}
