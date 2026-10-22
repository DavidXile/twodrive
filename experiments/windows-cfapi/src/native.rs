//! Real CldApi adapter for generated fixtures only. Registration is explicitly opt-in.
use crate::{LocalStatus, transfer_range};
use std::{
    collections::HashMap,
    ffi::c_void,
    mem::{offset_of, size_of},
    os::windows::ffi::OsStrExt,
    path::{Component, Path, PathBuf, Prefix},
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use twodrive_core::FileState;
use windows_sys::Win32::{
    Foundation::*,
    Storage::{CloudFilters::*, FileSystem::*},
};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE, FILE_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT,
            FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    },
    Win32::System::IO::IO_STATUS_BLOCK,
};

type Result<T> = std::result::Result<T, String>;
const ID: &[u8] = b"twodrive-cfapi-generated-fixture-v1";
const SIZE: usize = 65536;
const OFFLINE: i32 = 0xC000CF11u32 as i32;
const FAILED: i32 = 0xC000CF12u32 as i32;

fn hr(value: i32, action: &str) -> Result<()> {
    if value < 0 {
        Err(format!("{action}: HRESULT 0x{:08X}", value as u32))
    } else {
        Ok(())
    }
}
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

pub fn payload() -> Vec<u8> {
    (0..SIZE).map(|i| (i % 251) as u8).collect()
}

pub fn platform() -> Result<CF_PLATFORM_INFO> {
    let mut info = CF_PLATFORM_INFO::default();
    hr(
        unsafe { CfGetPlatformInfo(&mut info) },
        "query CFAPI platform",
    )?;
    Ok(info)
}

/// Engine-owned opaque object source. No paths, Provider or database API crosses
/// this boundary. Each requested buffer is at most 64 KiB.
pub trait HydrationSource: Send + Sync {
    fn size(&self, identity: &[u8]) -> Result<usize>;
    fn read_range(
        &self,
        identity: &[u8],
        offset: usize,
        length: usize,
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>>;
}

#[derive(Default)]
struct Source {
    calls: AtomicUsize,
    failures: AtomicUsize,
    offline: AtomicBool,
    stopping: AtomicBool,
    engine: Option<Arc<dyn HydrationSource>>,
    active: Mutex<HashMap<(i64, i64), Arc<AtomicBool>>>,
}

// The OS owns callback pointers until CfDisconnectSyncRoot returns. Source is
// boxed for a stable address and outlives that call, including error unwinding.
unsafe extern "system" fn fetch(
    info: *const CF_CALLBACK_INFO,
    params: *const CF_CALLBACK_PARAMETERS,
) {
    // SAFETY: CldApi supplies valid pointers for the duration of this callback.
    let (info, params) = unsafe { (&*info, &*params) };
    let source = unsafe { &*(info.CallbackContext as *const Source) };
    source.calls.fetch_add(1, Ordering::SeqCst);
    let requested = unsafe { params.Anonymous.FetchData };
    let mut status = FAILED;
    let mut offset = requested.RequiredFileOffset;
    let mut length = requested.RequiredLength;
    let cancelled = Arc::new(AtomicBool::new(false));
    let key = (info.TransferKey, info.RequestKey);
    let mut claimed = false;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
        if info.FileIdentity.is_null()
            || info.FileIdentityLength == 0
            || info.FileIdentityLength > 4096
        {
            return Err("invalid opaque identity".into());
        }
        let identity = unsafe {
            std::slice::from_raw_parts(
                info.FileIdentity.cast::<u8>(),
                info.FileIdentityLength as usize,
            )
        };
        let size = match &source.engine {
            Some(engine) => engine.size(identity)?,
            None if identity == ID => SIZE,
            None => return Err("unknown identity".into()),
        };
        let (start, end) = transfer_range(offset, length, size).ok_or("invalid range")?;
        {
            let mut active = source.active.lock().map_err(|_| "request gate poisoned")?;
            if active.len() >= 32 || active.contains_key(&key) {
                return Err("request capacity exceeded".into());
            }
            active.insert(key, cancelled.clone());
            claimed = true;
        }
        offset = start as i64;
        length = (end - start) as i64;
        for start in (start..end).step_by(65536) {
            if source.offline.load(Ordering::SeqCst)
                || source.stopping.load(Ordering::SeqCst)
                || cancelled.load(Ordering::SeqCst)
            {
                return Err("request unavailable or cancelled".into());
            }
            let count = (end - start).min(65536);
            let bytes = match &source.engine {
                Some(engine) => engine.read_range(identity, start, count, &cancelled)?,
                None => payload()[start..start + count].to_vec(),
            };
            if bytes.len() != count
                || cancelled.load(Ordering::SeqCst)
                || source.stopping.load(Ordering::SeqCst)
            {
                return Err("short or cancelled engine read".into());
            }
            execute_transfer(info, 0, start as i64, count as i64, &bytes)?;
        }
        Ok(())
    }));
    if claimed && let Ok(mut active) = source.active.lock() {
        active.remove(&key);
    }
    if matches!(outcome, Ok(Ok(()))) {
        return;
    }
    if source.offline.load(Ordering::SeqCst) {
        status = OFFLINE;
    }
    let _ = execute_transfer(info, status, offset, length, &[]);
    source.failures.fetch_add(1, Ordering::SeqCst);
}

unsafe extern "system" fn cancel(info: *const CF_CALLBACK_INFO, _: *const CF_CALLBACK_PARAMETERS) {
    let info = unsafe { &*info };
    let source = unsafe { &*(info.CallbackContext as *const Source) };
    if let Ok(active) = source.active.lock() {
        // A partial cancellation conservatively cancels this entire request.
        if let Some(flag) = active.get(&(info.TransferKey, info.RequestKey)) {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

fn execute_transfer(
    info: &CF_CALLBACK_INFO,
    status: i32,
    offset: i64,
    length: i64,
    bytes: &[u8],
) -> Result<()> {
    let operation = CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: CF_OPERATION_TYPE_TRANSFER_DATA,
        ConnectionKey: info.ConnectionKey,
        TransferKey: info.TransferKey,
        RequestKey: info.RequestKey,
        CorrelationVector: info.CorrelationVector,
        ..Default::default()
    };
    let mut transfer = CF_OPERATION_PARAMETERS {
        ParamSize: (offset_of!(CF_OPERATION_PARAMETERS, Anonymous)
            + size_of::<CF_OPERATION_PARAMETERS_0_0>()) as u32,
        ..Default::default()
    };
    transfer.Anonymous.TransferData = CF_OPERATION_PARAMETERS_0_0 {
        Flags: CF_OPERATION_TRANSFER_DATA_FLAG_NONE,
        CompletionStatus: status,
        Buffer: if bytes.is_empty() {
            ptr::null()
        } else {
            bytes.as_ptr().cast()
        },
        Offset: offset,
        Length: length,
    };
    // CfExecute consumes this buffer synchronously; it cannot outlive bytes.
    hr(
        unsafe { CfExecute(&operation, &mut transfer) },
        "transfer engine data",
    )
}

unsafe extern "system" fn deny_delete(
    info: *const CF_CALLBACK_INFO,
    _: *const CF_CALLBACK_PARAMETERS,
) {
    deny_mutation(unsafe { &*info }, CF_OPERATION_TYPE_ACK_DELETE);
}
unsafe extern "system" fn deny_rename(
    info: *const CF_CALLBACK_INFO,
    _: *const CF_CALLBACK_PARAMETERS,
) {
    deny_mutation(unsafe { &*info }, CF_OPERATION_TYPE_ACK_RENAME);
}
unsafe extern "system" fn deny_dehydrate(
    info: *const CF_CALLBACK_INFO,
    _: *const CF_CALLBACK_PARAMETERS,
) {
    deny_mutation(unsafe { &*info }, CF_OPERATION_TYPE_ACK_DEHYDRATE);
}
fn deny_mutation(info: &CF_CALLBACK_INFO, kind: CF_OPERATION_TYPE) {
    let operation = CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: kind,
        ConnectionKey: info.ConnectionKey,
        TransferKey: info.TransferKey,
        RequestKey: info.RequestKey,
        CorrelationVector: info.CorrelationVector,
        ..Default::default()
    };
    let mut parameters = CF_OPERATION_PARAMETERS::default();
    let denied = 0xC0000022u32 as i32;
    let size = match kind {
        CF_OPERATION_TYPE_ACK_DELETE => {
            parameters.Anonymous.AckDelete = CF_OPERATION_PARAMETERS_0_7 {
                Flags: 0,
                CompletionStatus: denied,
            };
            size_of::<CF_OPERATION_PARAMETERS_0_7>()
        }
        CF_OPERATION_TYPE_ACK_RENAME => {
            parameters.Anonymous.AckRename = CF_OPERATION_PARAMETERS_0_6 {
                Flags: 0,
                CompletionStatus: denied,
            };
            size_of::<CF_OPERATION_PARAMETERS_0_6>()
        }
        _ => {
            parameters.Anonymous.AckDehydrate = CF_OPERATION_PARAMETERS_0_5 {
                CompletionStatus: denied,
                ..Default::default()
            };
            size_of::<CF_OPERATION_PARAMETERS_0_5>()
        }
    };
    parameters.ParamSize = (offset_of!(CF_OPERATION_PARAMETERS, Anonymous) + size) as u32;
    let _ = unsafe { CfExecute(&operation, &mut parameters) };
}

pub struct Session {
    #[cfg(feature = "shell-integration")]
    shell: Option<crate::shell::Registration>,
    root: PathBuf,
    registered: bool,
    connection: Option<CF_CONNECTION_KEY>,
    source: Box<Source>,
    _guard: RootGuard,
}

// Handles deny DELETE sharing, preventing directory substitution while path-
// based CFAPI calls run. Check original components before canonicalization.
struct RootGuard {
    root: PathBuf,
    _handles: Vec<Handle>,
}
/// Newly created ordinary directory with its original path chain held against
/// replacement; no registration or existing-data adoption.
pub struct GeneratedDirectory(RootGuard);
impl GeneratedDirectory {
    pub fn create(path: &Path) -> Result<Self> {
        let mut guard = RootGuard::lock_parent(path)?;
        guard.require_unmanaged_parent()?;
        guard.create_leaf()?;
        Ok(Self(guard))
    }
    pub fn path(&self) -> &Path {
        &self.0.root
    }
}
impl RootGuard {
    fn lock_parent(root: &Path) -> Result<Self> {
        let disk = matches!(root.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)));
        if !disk
            || !root.is_absolute()
            || root.components().any(|c| matches!(c, Component::ParentDir))
            || root.file_name().is_none()
        {
            return Err(
                "fixture root requires an absolute local disk path without parent traversal".into(),
            );
        }
        let parent = root.parent().ok_or("missing parent")?;
        let mut handles = Vec::new();
        for ancestor in parent.ancestors().collect::<Vec<_>>().into_iter().rev() {
            let handle = unsafe {
                CreateFileW(
                    wide(ancestor).as_ptr(),
                    FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(format!("lock original ancestor: {}", unsafe {
                    GetLastError()
                }));
            }
            let handle = Handle(handle, false);
            let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
            if unsafe {
                GetFileInformationByHandleEx(
                    handle.0,
                    FileAttributeTagInfo,
                    (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                    size_of_val(&tag) as u32,
                )
            } == 0
            {
                return Err("cannot inspect original ancestor".into());
            }
            if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err("fixture ancestors must not be reparse points".into());
            }
            handles.push(handle);
        }
        Ok(Self {
            root: root.to_path_buf(),
            _handles: handles,
        })
    }
    fn require_unmanaged_parent(&self) -> Result<()> {
        let parent = self.root.parent().ok_or("missing parent")?;
        let mut existing = CF_SYNC_ROOT_BASIC_INFO::default();
        let query = unsafe {
            CfGetSyncRootInfoByPath(
                wide(parent).as_ptr(),
                CF_SYNC_ROOT_INFO_BASIC,
                (&mut existing as *mut CF_SYNC_ROOT_BASIC_INFO).cast(),
                size_of_val(&existing) as u32,
                ptr::null_mut(),
            )
        };
        require_unmanaged(query)
    }
    fn create_leaf(&mut self) -> Result<()> {
        let root = &self.root;
        let handles = &mut self._handles;
        // Atomically create a new leaf relative to the locked parent and keep
        // its no-delete-sharing handle. No create/open replacement race.
        let mut name = wide(Path::new(root.file_name().unwrap()));
        let length = ((name.len() - 1) * 2)
            .try_into()
            .map_err(|_| "fixture name too long")?;
        let unicode = UNICODE_STRING {
            Length: length,
            MaximumLength: length,
            Buffer: name.as_mut_ptr(),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: handles.last().ok_or("no parent handle")?.0,
            ObjectName: &unicode,
            Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
            ..Default::default()
        };
        let mut io = IO_STATUS_BLOCK::default();
        let mut handle = ptr::null_mut();
        let status = unsafe {
            NtCreateFile(
                &mut handle,
                FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | SYNCHRONIZE,
                &attributes,
                &mut io,
                ptr::null(),
                FILE_ATTRIBUTE_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                FILE_CREATE,
                FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
                ptr::null(),
                0,
            )
        };
        if status < 0 {
            return Err(format!(
                "create new fixture directory: NTSTATUS 0x{:08X}",
                status as u32
            ));
        }
        handles.push(Handle(handle, false));
        self.root = root.canonicalize().map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn require_unmanaged(result: i32) -> Result<()> {
    // Observed and defined ERROR_CLOUD_FILE_NOT_UNDER_SYNC_ROOT (390).
    if result == (0x80070000u32 | ERROR_CLOUD_FILE_NOT_UNDER_SYNC_ROOT) as i32 {
        return Ok(());
    }
    if result >= 0 {
        return Err("fixture parent belongs to a sync root".into());
    }
    Err(format!(
        "cannot establish unmanaged parent: HRESULT 0x{:08X}",
        result as u32
    ))
}
pub fn verify_unregistered(path: &Path) -> Result<()> {
    let mut info = CF_SYNC_ROOT_BASIC_INFO::default();
    require_unmanaged(unsafe {
        CfGetSyncRootInfoByPath(
            wide(path).as_ptr(),
            CF_SYNC_ROOT_INFO_BASIC,
            (&mut info as *mut CF_SYNC_ROOT_BASIC_INFO).cast(),
            size_of_val(&info) as u32,
            ptr::null_mut(),
        )
    })
}

#[cfg(feature = "shell-integration")]
pub fn verify_shell_unregistered(path: &Path) -> Result<()> {
    crate::shell::verify_absent(path)
}

impl Session {
    /// Only a NEW empty root beneath locked non-reparse ancestors is accepted. Never
    /// updates an existing registration or adopts another client's placeholder.
    pub fn create(root: &Path) -> Result<Self> {
        Self::create_inner(root, None, cfg!(feature = "shell-integration"))
    }
    /// Same new-root guard as the probe; explicit fixture host only.
    pub fn create_with_source(root: &Path, engine: Arc<dyn HydrationSource>) -> Result<Self> {
        Self::create_inner(root, Some(engine), cfg!(feature = "shell-integration"))
    }
    fn create_inner(
        root: &Path,
        engine: Option<Arc<dyn HydrationSource>>,
        _shell: bool,
    ) -> Result<Self> {
        if !root.is_absolute() || root.file_name().is_none() {
            return Err("fixture root must be a new absolute directory".into());
        }
        let mut guard = RootGuard::lock_parent(root)?;
        guard.require_unmanaged_parent()?;
        guard.create_leaf()?;
        let mut session = Self {
            root: guard.root.clone(),
            registered: false,
            connection: None,
            source: Box::new(Source {
                engine,
                ..Default::default()
            }),
            _guard: guard,
            #[cfg(feature = "shell-integration")]
            shell: None,
        };
        #[cfg(feature = "shell-integration")]
        if _shell {
            session.shell = Some(crate::shell::Registration::create(&session.root)?);
            session.registered = true;
            session.verify_shell_registration()?;
            session.connect()?;
            return Ok(session);
        }
        let name = wide(Path::new("TwoDrive isolated CFAPI experiment"));
        let version = wide(Path::new(env!("CARGO_PKG_VERSION")));
        let registration = CF_SYNC_REGISTRATION {
            StructSize: size_of::<CF_SYNC_REGISTRATION>() as u32,
            ProviderName: name.as_ptr(),
            ProviderVersion: version.as_ptr(),
            SyncRootIdentity: ID.as_ptr().cast(),
            SyncRootIdentityLength: ID.len() as u32,
            ..Default::default()
        };
        let policies = CF_SYNC_POLICIES {
            StructSize: size_of::<CF_SYNC_POLICIES>() as u32,
            Hydration: CF_HYDRATION_POLICY {
                Primary: CF_HYDRATION_POLICY_FULL,
                Modifier: CF_HYDRATION_POLICY_MODIFIER_NONE,
            },
            Population: CF_POPULATION_POLICY {
                Primary: CF_POPULATION_POLICY_ALWAYS_FULL,
                Modifier: CF_POPULATION_POLICY_MODIFIER_NONE,
            },
            InSync: CF_INSYNC_POLICY_TRACK_ALL,
            HardLink: CF_HARDLINK_POLICY_NONE,
            PlaceholderManagement: CF_PLACEHOLDER_MANAGEMENT_POLICY_DEFAULT,
        };
        hr(
            unsafe {
                CfRegisterSyncRoot(
                    wide(&session.root).as_ptr(),
                    &registration,
                    &policies,
                    CF_REGISTER_FLAG_NONE,
                )
            },
            "register generated root",
        )?;
        session.registered = true;
        session.connect()?;
        Ok(session)
    }

    #[cfg(feature = "shell-integration")]
    pub fn verify_shell_registration(&self) -> Result<()> {
        self.shell
            .as_ref()
            .ok_or("Shell registration is absent")?
            .verify()
    }

    #[cfg(feature = "shell-integration")]
    pub fn shell_placeholder_status(&self, path: &Path) -> Result<u32> {
        if path.parent() != Some(self.root.as_path()) {
            return Err("Shell query requires a generated leaf in the owned root".into());
        }
        crate::shell::placeholder_status(path)
    }

    pub fn connect(&mut self) -> Result<()> {
        if self.connection.is_some() {
            return Err("already connected".into());
        }
        self.source.stopping.store(false, Ordering::SeqCst);
        let table = [
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_FETCH_DATA,
                Callback: Some(fetch),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_CANCEL_FETCH_DATA,
                Callback: Some(cancel),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_DELETE,
                Callback: Some(deny_delete),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_RENAME,
                Callback: Some(deny_rename),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_DEHYDRATE,
                Callback: Some(deny_dehydrate),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NONE,
                Callback: None,
            },
        ];
        let mut key = 0;
        hr(
            unsafe {
                CfConnectSyncRoot(
                    wide(&self.root).as_ptr(),
                    table.as_ptr(),
                    (&*self.source as *const Source).cast::<c_void>(),
                    CF_CONNECT_FLAG_NONE,
                    &mut key,
                )
            },
            "connect generated root",
        )?;
        self.connection = Some(key);
        Ok(())
    }
    pub fn disconnect(&mut self) -> Result<()> {
        self.source.stopping.store(true, Ordering::SeqCst);
        if let Ok(active) = self.source.active.lock() {
            for flag in active.values() {
                flag.store(true, Ordering::SeqCst);
            }
        }
        if let Some(key) = self.connection {
            hr(
                unsafe { CfDisconnectSyncRoot(key) },
                "disconnect generated root",
            )?;
            self.connection = None;
        }
        Ok(())
    }
    pub fn finish(&mut self) -> Result<()> {
        self.disconnect()?;
        if self.registered {
            #[cfg(feature = "shell-integration")]
            if let Some(shell) = &self.shell {
                shell.unregister()?;
                self.registered = false;
                return Ok(());
            }
            hr(
                unsafe { CfUnregisterSyncRoot(wide(&self.root).as_ptr()) },
                "unregister generated root",
            )?;
            self.registered = false;
        }
        Ok(())
    }
    pub fn calls(&self) -> usize {
        self.source.calls.load(Ordering::SeqCst)
    }
    pub fn failures(&self) -> usize {
        self.source.failures.load(Ordering::SeqCst)
    }
    pub fn offline(&self, value: bool) {
        self.source.offline.store(value, Ordering::SeqCst);
    }

    pub fn placeholder(&self, relative: &Path) -> Result<PathBuf> {
        self.placeholder_for(relative, ID, SIZE)
    }
    pub fn placeholder_for(
        &self,
        relative: &Path,
        identity: &[u8],
        size: usize,
    ) -> Result<PathBuf> {
        if identity.is_empty() || identity.len() > 4096 || size > i64::MAX as usize {
            return Err("invalid placeholder identity/size".into());
        }
        // The harness supplies generated leaf names only, including a long leaf.
        if relative.components().count() != 1
            || relative.file_name().is_none()
            || relative.is_absolute()
        {
            return Err("only a generated leaf name is accepted".into());
        }
        let name = wide(relative);
        let mut entry = CF_PLACEHOLDER_CREATE_INFO {
            RelativeFileName: name.as_ptr(),
            FileIdentity: identity.as_ptr().cast(),
            FileIdentityLength: identity.len() as u32,
            Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC,
            ..Default::default()
        };
        entry.FsMetadata.FileSize = size as i64;
        entry.FsMetadata.BasicInfo.FileAttributes = FILE_ATTRIBUTE_NORMAL;
        let mut done = 0;
        hr(
            unsafe {
                CfCreatePlaceholders(
                    wide(&self.root).as_ptr(),
                    &mut entry,
                    1,
                    CF_CREATE_FLAG_STOP_ON_ERROR,
                    &mut done,
                )
            },
            "create generated placeholder",
        )?;
        hr(entry.Result, "placeholder entry")?;
        if done != 1 {
            return Err("placeholder was not created".into());
        }
        Ok(self.root.join(relative))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Err(error) = self.finish() {
            // Do not claim cleanup succeeded. Leaking the context is safer than
            // dangling callback memory if the OS cannot disconnect.
            eprintln!("cleanup incomplete: {error}");
            if self.connection.is_some() {
                let _ = Box::leak(std::mem::take(&mut self.source));
            }
        }
    }
}

struct Handle(HANDLE, bool);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            if self.1 {
                CfCloseHandle(self.0);
            } else {
                CloseHandle(self.0);
            }
        }
    }
}

fn metadata_handle(path: &Path) -> Result<Handle> {
    // OPEN_REPARSE_POINT + attributes-only does not request file content.
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(format!("open attributes: {}", unsafe { GetLastError() }));
    }
    Ok(Handle(handle, false))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NativeObservation {
    pub logical_size: u64,
    pub resident: bool,
    pub partial: bool,
    pub pinned: bool,
    pub pin_known_unpinned: bool,
    pub os_in_sync: bool,
    pub modified_bytes: i64,
}
impl NativeObservation {
    pub fn clean(&self) -> bool {
        self.resident && self.pin_known_unpinned && self.os_in_sync && self.modified_bytes == 0
    }
}
fn observation(handle: HANDLE, expected_identity: &[u8], size: usize) -> Result<NativeObservation> {
    // u64 buffer provides alignment for the variable-length standard-info blob.
    let mut buffer = [0u64; 1024];
    let mut returned = 0;
    hr(
        unsafe {
            CfGetPlaceholderInfo(
                handle,
                CF_PLACEHOLDER_INFO_STANDARD,
                buffer.as_mut_ptr().cast(),
                size_of_val(&buffer) as u32,
                &mut returned,
            )
        },
        "inspect placeholder",
    )?;
    if returned < size_of::<CF_PLACEHOLDER_STANDARD_INFO>() as u32 {
        return Err("short placeholder info".into());
    }
    let info = unsafe { &*buffer.as_ptr().cast::<CF_PLACEHOLDER_STANDARD_INFO>() };
    let identity_end =
        offset_of!(CF_PLACEHOLDER_STANDARD_INFO, FileIdentity) + expected_identity.len();
    if info.FileIdentityLength as usize != expected_identity.len()
        || identity_end > returned as usize
    {
        return Err("foreign placeholder identity".into());
    }
    let identity =
        unsafe { std::slice::from_raw_parts(info.FileIdentity.as_ptr(), expected_identity.len()) };
    if identity != expected_identity {
        return Err("foreign placeholder identity".into());
    }
    let mut logical_size = 0i64;
    if unsafe { GetFileSizeEx(handle, &mut logical_size) } == 0 || logical_size < 0 {
        return Err("cannot inspect logical size".into());
    }
    Ok(NativeObservation {
        logical_size: logical_size as u64,
        resident: info.OnDiskDataSize >= size as i64,
        partial: info.OnDiskDataSize > 0 && info.OnDiskDataSize < size as i64,
        pinned: info.PinState == CF_PIN_STATE_PINNED,
        pin_known_unpinned: info.PinState == CF_PIN_STATE_UNPINNED
            || info.PinState == CF_PIN_STATE_UNSPECIFIED,
        os_in_sync: info.InSyncState == CF_IN_SYNC_STATE_IN_SYNC,
        modified_bytes: info.ModifiedDataSize,
    })
}
fn status(handle: HANDLE, engine_state: FileState) -> Result<LocalStatus> {
    let info = observation(handle, ID, SIZE)?;
    Ok(LocalStatus {
        resident: info.resident,
        partial: info.partial,
        pinned: info.pinned,
        os_in_sync: info.os_in_sync,
        modified_bytes: info.modified_bytes,
        engine_state,
        pending_operations: false,
        open_handles: false,
    })
}

pub fn inspect_item(path: &Path, identity: &[u8], size: usize) -> Result<NativeObservation> {
    observation(metadata_handle(path)?.0, identity, size)
}

/// Scope-bound OS guard. It cannot be detached into a release-ready boolean.
pub struct LockedPlaceholder<'a> {
    handle: HANDLE,
    identity: &'a [u8],
    size: usize,
}
impl LockedPlaceholder<'_> {
    pub fn inspect(&self) -> Result<NativeObservation> {
        observation(
            unsafe { CfGetWin32HandleFromProtectedHandle(self.handle) },
            self.identity,
            self.size,
        )
    }
    pub fn dehydrate(&self) -> Result<()> {
        let state = self.inspect()?;
        if !state.clean() || state.logical_size != self.size as u64 {
            return Err("native state vetoes dehydration".into());
        }
        hr(
            unsafe {
                CfDehydratePlaceholder(self.handle, 0, -1, CF_DEHYDRATE_FLAG_NONE, ptr::null_mut())
            },
            "dehydrate journal-verified placeholder",
        )
    }
}
pub fn with_exclusive_placeholder<T>(
    path: &Path,
    identity: &[u8],
    size: usize,
    action: impl FnOnce(&LockedPlaceholder<'_>) -> Result<T>,
) -> Result<T> {
    let mut raw = ptr::null_mut();
    hr(
        unsafe {
            CfOpenFileWithOplock(
                wide(path).as_ptr(),
                CF_OPEN_FILE_FLAG_EXCLUSIVE | CF_OPEN_FILE_FLAG_WRITE_ACCESS,
                &mut raw,
            )
        },
        "exclusive engine oplock",
    )?;
    let handle = Handle(raw, true);
    if !unsafe { CfReferenceProtectedHandle(handle.0) } {
        return Err("oplock already broken".into());
    }
    struct Reference(HANDLE);
    impl Drop for Reference {
        fn drop(&mut self) {
            unsafe { CfReleaseProtectedHandle(self.0) };
        }
    }
    let _reference = Reference(handle.0);
    let locked = LockedPlaceholder {
        handle: handle.0,
        identity,
        size,
    };
    locked.inspect()?;
    action(&locked)
}

pub fn inspect(path: &Path, engine_state: FileState) -> Result<LocalStatus> {
    status(metadata_handle(path)?.0, engine_state)
}

pub fn pin(path: &Path, pinned: bool) -> Result<()> {
    let mut handle = ptr::null_mut();
    hr(
        unsafe {
            CfOpenFileWithOplock(
                wide(path).as_ptr(),
                CF_OPEN_FILE_FLAG_WRITE_ACCESS,
                &mut handle,
            )
        },
        "open pin oplock",
    )?;
    let handle = Handle(handle, true);
    hr(
        unsafe {
            CfSetPinState(
                handle.0,
                if pinned {
                    CF_PIN_STATE_PINNED
                } else {
                    CF_PIN_STATE_UNSPECIFIED
                },
                CF_SET_PIN_FLAG_NONE,
                ptr::null_mut(),
            )
        },
        "set pin intent",
    )
}

/// Prototype only: engine_state is the harness's immutable fixture state. A
/// production caller needs a durable generation/journal and item-lock adapter.
pub fn release(path: &Path, engine_state: FileState) -> Result<()> {
    with_exclusive_placeholder(path, ID, SIZE, |locked| {
        if engine_state != FileState::Cached {
            return Err("fixture engine state veto".into());
        }
        locked.dehydrate()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(feature = "shell-integration")]
    #[test]
    #[ignore = "explicit new generated Shell root required; never run automatically"]
    fn shell_registration_and_metadata_states() {
        let root = PathBuf::from(
            std::env::var_os("TWODRIVE_APPROVED_SHELL_FIXTURE_ROOT")
                .expect("explicit new Shell root required"),
        );
        assert!(
            root.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("twodrive-cfapi-fixture-"))
        );
        let bare = root.with_file_name(format!(
            "{}-bare",
            root.file_name().unwrap().to_string_lossy()
        ));
        let mut control = Session::create_inner(&bare, None, false).unwrap();
        let baseline = (|| -> Result<()> {
            verify_shell_unregistered(&bare)?;
            assert!(control.verify_shell_registration().is_err());
            Ok(())
        })();
        control.finish().unwrap();
        verify_unregistered(&bare).unwrap();
        baseline.unwrap();

        let mut session = Session::create(&root).unwrap();
        let result = (|| -> Result<serde_json::Value> {
            session.verify_shell_registration()?;
            let path = session.placeholder(Path::new("generated-中文.bin"))?;
            let another = session.placeholder(Path::new("generated-clean.bin"))?;
            let online = session.shell_placeholder_status(&path)?;
            assert_eq!(session.calls(), 0, "Shell metadata hydrated content");
            assert_eq!(fs::read(&path).map_err(|e| e.to_string())?, payload());
            let local = session.shell_placeholder_status(&path)?;
            assert_ne!(
                online, local,
                "Shell must distinguish cloud-only and local residency"
            );
            assert!(inspect(&path, FileState::Cached)?.resident);
            pin(&path, true)?;
            let pinned = session.shell_placeholder_status(&path)?;
            assert_ne!(local, pinned, "Shell must distinguish pinned intent");
            assert!(release(&path, FileState::Cached).is_err());
            assert_eq!(fs::read(&another).map_err(|e| e.to_string())?, payload());
            release(&another, FileState::Cached)?;
            assert_eq!(session.shell_placeholder_status(&another)?, online);
            // CfUnregister can remove unhydrated generated placeholders. Finish
            // the fixture with complete generated bytes so cleanup retains both.
            assert_eq!(fs::read(&another).map_err(|e| e.to_string())?, payload());
            Ok(
                serde_json::json!({"baseline_shell_absent":true,"shell_registered":true,
                "metadata_without_hydration":true,"online_shell_status":online,
                "local_shell_status":local,"pinned_shell_status":pinned,
                "pinned_release_rejected":true,"clean_release_returns_online_status":true,
                "visual_icon_acceptance":"not_run_by_this_test"}),
            )
        })();
        session.finish().unwrap();
        verify_unregistered(&root).unwrap();
        verify_shell_unregistered(&root).unwrap();
        let mut report = result.unwrap();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        report["cfapi_unregistered"] = serde_json::json!(true);
        report["shell_unregistered"] = serde_json::json!(true);
        report["generated_files_retained"] = serde_json::json!(true);
        if let Some(output) = std::env::var_os("TWODRIVE_SHELL_REPORT") {
            fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
        println!("{report}");
    }
    #[test]
    fn only_known_unmanaged_hresult_allows_creation() {
        assert!(require_unmanaged(0x80070186u32 as i32).is_ok());
        for code in [
            0,
            1,
            0x80070005u32 as i32,
            0x80070057u32 as i32,
            0x80070001u32 as i32,
            -1,
        ] {
            assert!(require_unmanaged(code).is_err());
        }
    }
    #[test]
    fn new_leaf_and_ancestors_stay_locked_against_substitution() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("new-fixture");
        // File-lock behavior is independent of Cloud Filter support on a CI volume.
        let mut guard = RootGuard::lock_parent(&root).unwrap();
        guard.create_leaf().unwrap();
        let mut second = RootGuard::lock_parent(&root).unwrap();
        assert!(second.create_leaf().is_err());
        drop(second);
        assert!(fs::rename(&root, temp.path().join("replacement")).is_err());
        assert!(fs::rename(temp.path(), temp.path().with_extension("replacement")).is_err());
        drop(guard);
        fs::rename(&root, temp.path().join("replacement")).unwrap();
    }
    #[test]
    fn original_junction_is_rejected_before_normalizing_or_creating_leaf() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        let link = temp.path().join("junction");
        fs::create_dir(&real).unwrap();
        fs::write(real.join("marker"), b"generated marker").unwrap();
        let status = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:TD_GENERATED_LINK -Target $env:TD_GENERATED_TARGET | Out-Null"])
            .env("TD_GENERATED_LINK", &link).env("TD_GENERATED_TARGET", &real).status().unwrap();
        assert!(status.success());
        assert!(RootGuard::lock_parent(&link.join("new-fixture")).is_err());
        assert!(!real.join("new-fixture").exists());
        fs::remove_dir(&link).unwrap();
        assert_eq!(fs::read(real.join("marker")).unwrap(), b"generated marker");
    }
}
