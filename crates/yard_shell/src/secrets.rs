use crate::Extension;
use gpui::{App, AppContext, Global, Task};
use std::{
    cell::Cell,
    collections::HashMap,
    fmt,
    str::Utf8Error,
    sync::{Arc, Mutex},
};
use tracing::Instrument as _;
use zeroize::Zeroizing;

pub const MAX_SECRET_BYTES: usize = 2560;

/// A secret whose bytes are wiped on drop.
pub struct Secret(Zeroizing<Vec<u8>>);

impl Secret {
    pub fn expose(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn expose_str(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(self.expose())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretError {
    InvalidKey,
    TooLarge,
    Unavailable,
    Denied,
    Failed,
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidKey => "invalid secret key",
            Self::TooLarge => "secret too large",
            Self::Unavailable => "secret store unavailable",
            Self::Denied => "access to the secret store denied",
            Self::Failed => "secret store failed",
        })
    }
}

impl std::error::Error for SecretError {}

type Address = (String, String);

#[derive(Default)]
struct MemoryStore {
    values: HashMap<Address, Zeroizing<Vec<u8>>>,
    next_error: Option<SecretError>,
}

impl MemoryStore {
    fn access<T>(
        store: &Mutex<Self>,
        operation: impl FnOnce(&mut HashMap<Address, Zeroizing<Vec<u8>>>) -> T,
    ) -> Result<T, StoreError> {
        let mut store = store.lock().map_err(|_| SecretError::Failed)?;
        if let Some(error) = store.next_error.take() {
            return Err(error.into());
        }
        Ok(operation(&mut store.values))
    }
}

enum Backend {
    System {
        identifier: String,
        product_name: String,
    },
    Memory(Mutex<MemoryStore>),
    InvalidSystemNames,
}

#[derive(Clone)]
pub struct SecretStore(Arc<Backend>);

impl SecretStore {
    pub fn system(identifier: &str, product_name: &str) -> Self {
        if identifier.is_empty()
            || identifier.len() > 1024
            || identifier.chars().any(|c| c == '/' || c.is_control())
            || product_name.is_empty()
            || product_name.chars().any(char::is_control)
        {
            return Self(Arc::new(Backend::InvalidSystemNames));
        }
        Self(Arc::new(Backend::System {
            identifier: identifier.to_owned(),
            product_name: product_name.to_owned(),
        }))
    }

    pub fn memory() -> Self {
        Self(Arc::new(Backend::Memory(Mutex::default())))
    }

    /// Makes the next call to the memory store fail with the given error.
    pub fn fail_next(&self, error: SecretError) {
        if let Backend::Memory(store) = self.0.as_ref() {
            store
                .lock()
                .expect("memory secret store lock poisoned")
                .next_error = Some(error);
        }
    }

    async fn set(&self, address: Address, value: Zeroizing<Vec<u8>>) -> Result<(), StoreError> {
        match self.0.as_ref() {
            Backend::System {
                identifier,
                product_name,
            } => {
                let names = platform::entry_names(identifier, product_name, &address.0, &address.1);
                platform::set(names, value).await
            }
            Backend::Memory(store) => MemoryStore::access(store, |values| {
                values.insert(address, value);
            }),
            Backend::InvalidSystemNames => Err(SecretError::Unavailable.into()),
        }
    }

    async fn get(&self, address: Address) -> Result<Option<Secret>, StoreError> {
        match self.0.as_ref() {
            Backend::System {
                identifier,
                product_name,
            } => {
                let names = platform::entry_names(identifier, product_name, &address.0, &address.1);
                platform::get(names).await
            }
            Backend::Memory(store) => {
                MemoryStore::access(store, |values| values.get(&address).cloned().map(Secret))
            }
            Backend::InvalidSystemNames => Err(SecretError::Unavailable.into()),
        }
    }

    async fn delete(&self, address: Address) -> Result<(), StoreError> {
        match self.0.as_ref() {
            Backend::System {
                identifier,
                product_name,
            } => {
                let names = platform::entry_names(identifier, product_name, &address.0, &address.1);
                platform::delete(names).await
            }
            Backend::Memory(store) => MemoryStore::access(store, |values| {
                values.remove(&address);
            }),
            Backend::InvalidSystemNames => Err(SecretError::Unavailable.into()),
        }
    }
}

struct AppSecretStore {
    store: SecretStore,
    invalid_names_reported: Cell<bool>,
}

impl Global for AppSecretStore {}

struct RequestedWithoutStore;

impl Global for RequestedWithoutStore {}

/// Installs the application's secret store once, at startup.
pub fn set_secret_store(cx: &mut App, store: SecretStore) {
    if cx.try_global::<AppSecretStore>().is_some() {
        log::error!("the application's secret store can only be set once");
        return;
    }
    cx.set_global(AppSecretStore {
        store,
        invalid_names_reported: Cell::default(),
    });
}

fn check_key(key: &str) -> Result<(), SecretError> {
    if (1..=128).contains(&key.len())
        && key
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-'))
    {
        Ok(())
    } else {
        Err(SecretError::InvalidKey)
    }
}

struct StoreError {
    kind: SecretError,
    status: Option<i32>,
}

impl From<SecretError> for StoreError {
    fn from(kind: SecretError) -> Self {
        Self { kind, status: None }
    }
}

impl StoreError {
    fn report(self) -> SecretError {
        let error = self.kind;
        match error {
            SecretError::InvalidKey | SecretError::TooLarge => {
                tracing::info!(reason = %error, "refused");
            }
            SecretError::Denied => tracing::info!(reason = %error, "denied"),
            SecretError::Unavailable | SecretError::Failed => {
                tracing::warn!("{error}");
                if let Some(status) = self.status {
                    log::warn!("{error} ({status})");
                } else {
                    log::warn!("{error}");
                }
            }
        }
        error
    }
}

impl Extension {
    pub fn set_secret(&self, key: &str, value: &[u8], cx: &App) -> Task<Result<(), SecretError>> {
        self.secret_request(
            cx,
            "set",
            key,
            Some(value),
            |store, address, value| async move {
                store.set(address, value).await?;
                tracing::debug!("stored");
                Ok(())
            },
        )
    }

    pub fn get_secret(&self, key: &str, cx: &App) -> Task<Result<Option<Secret>, SecretError>> {
        self.secret_request(cx, "get", key, None, |store, address, _| async move {
            let secret = store.get(address).await?;
            tracing::debug!(found = secret.is_some(), "read");
            Ok(secret)
        })
    }

    pub fn delete_secret(&self, key: &str, cx: &App) -> Task<Result<(), SecretError>> {
        self.secret_request(cx, "delete", key, None, |store, address, _| async move {
            store.delete(address).await?;
            tracing::debug!("deleted");
            Ok(())
        })
    }

    fn secret_request<T, F>(
        &self,
        cx: &App,
        op: &'static str,
        key: &str,
        value: Option<&[u8]>,
        operation: impl FnOnce(SecretStore, Address, Zeroizing<Vec<u8>>) -> F,
    ) -> Task<Result<T, SecretError>>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, StoreError>> + Send + 'static,
    {
        let span = tracing::info_span!("secret", extension = self.name.as_str(), op);
        let group = span.enter();

        let Some(app_store) = cx.try_global::<AppSecretStore>() else {
            tracing::warn!("{}", SecretError::Unavailable);
            if cx.has_global::<RequestedWithoutStore>() {
                return Task::ready(Err(SecretError::Unavailable));
            }
            // GPUI installs globals through a mutable App on the foreground thread.
            drop(group);
            return cx.spawn(async move |cx| {
                async {
                    cx.update(|cx| {
                        if !cx.has_global::<RequestedWithoutStore>() {
                            cx.set_global(RequestedWithoutStore);
                            log::error!(
                                "extension secrets are unavailable because the application did not set a secret store"
                            );
                        }
                    });
                    Err(SecretError::Unavailable)
                }
                .instrument(span)
                .await
            });
        };
        let store = &app_store.store;
        if matches!(store.0.as_ref(), Backend::InvalidSystemNames) {
            if !app_store.invalid_names_reported.replace(true) {
                log::error!(
                    "extension secrets are unavailable because the system store names are invalid"
                );
            }
            tracing::warn!("{}", SecretError::Unavailable);
            return Task::ready(Err(SecretError::Unavailable));
        }
        if let Err(error) = check_key(key) {
            return Task::ready(Err(StoreError::from(error).report()));
        }
        if value.is_some_and(|value| value.len() > MAX_SECRET_BYTES) {
            return Task::ready(Err(StoreError::from(SecretError::TooLarge).report()));
        }

        let address = (self.name.clone(), key.to_owned());
        let value = Zeroizing::new(value.unwrap_or_default().to_vec());
        let future = operation(store.clone(), address, value);
        drop(group);
        cx.background_spawn(
            async move { future.await.map_err(StoreError::report) }.instrument(span),
        )
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{Secret, SecretError, StoreError, Zeroizing};
    use objc2_core_foundation::{
        CFData, CFDictionary, CFRetained, CFString, CFType, kCFAllocatorNull, kCFBooleanTrue,
    };
    use objc2_security::{
        SecItemAdd, SecItemCopyMatching, SecItemDelete, SecItemUpdate, errSecAuthFailed,
        errSecDuplicateItem, errSecInteractionNotAllowed, errSecInteractionRequired,
        errSecItemNotFound, errSecNoAccessForItem, errSecNoDefaultKeychain, errSecNoSuchKeychain,
        errSecNotAvailable, errSecSuccess, errSecUserCanceled, kSecAttrAccount, kSecAttrService,
        kSecClass, kSecClassGenericPassword, kSecReturnData, kSecValueData,
    };
    use std::ptr::{self, NonNull};

    pub(super) struct EntryNames<'a> {
        pub(super) service: &'a str,
        pub(super) account: String,
    }

    pub(super) fn entry_names<'a>(
        identifier: &'a str,
        _product_name: &str,
        extension: &str,
        key: &str,
    ) -> EntryNames<'a> {
        EntryNames {
            service: identifier,
            account: format!("{extension}/{key}"),
        }
    }

    fn query(names: &EntryNames<'_>) -> CFRetained<CFDictionary<CFString, CFType>> {
        let service = CFString::from_str(names.service);
        let account = CFString::from_str(&names.account);
        // SAFETY: These are immutable Security framework constants.
        unsafe {
            CFDictionary::from_slices(
                &[kSecClass, kSecAttrService, kSecAttrAccount],
                &[kSecClassGenericPassword, &service, &account],
            )
        }
    }

    pub(super) async fn set(
        names: EntryNames<'_>,
        value: Zeroizing<Vec<u8>>,
    ) -> Result<(), StoreError> {
        // SAFETY: kCFAllocatorNull is an immutable Core Foundation constant.
        let null_allocator = unsafe { kCFAllocatorNull }.ok_or(SecretError::Failed)?;
        // SAFETY: The bytes remain valid until data and both attribute dictionaries
        // drop. kCFAllocatorNull leaves ownership of the bytes with value.
        let data = unsafe {
            CFData::with_bytes_no_copy(
                None,
                value.as_ptr(),
                value.len() as isize,
                Some(null_allocator),
            )
        }
        .ok_or(SecretError::Failed)?;
        let query = query(&names);
        // SAFETY: kSecValueData is an immutable Security framework constant.
        let update =
            unsafe { CFDictionary::<CFString, CFType>::from_slices(&[kSecValueData], &[&data]) };
        // SAFETY: The dictionaries contain attributes of the types Security expects.
        let mut status = unsafe { SecItemUpdate(query.as_opaque(), update.as_opaque()) };
        if status == errSecItemNotFound {
            let service = CFString::from_str(names.service);
            let account = CFString::from_str(&names.account);
            // SAFETY: These are immutable Security framework constants.
            let item = unsafe {
                CFDictionary::<CFString, CFType>::from_slices(
                    &[kSecClass, kSecAttrService, kSecAttrAccount, kSecValueData],
                    &[kSecClassGenericPassword, &service, &account, &data],
                )
            };
            // SAFETY: The dictionary has valid attributes and a null result asks
            // Security to return no item.
            status = unsafe { SecItemAdd(item.as_opaque(), ptr::null_mut()) };
            if status == errSecDuplicateItem {
                // A concurrent writer created the entry after our first update.
                // SAFETY: The original dictionaries are still valid.
                status = unsafe { SecItemUpdate(query.as_opaque(), update.as_opaque()) };
            }
        }
        drop(update);
        drop(query);
        drop(data);
        check_status(status)
    }

    pub(super) async fn get(names: EntryNames<'_>) -> Result<Option<Secret>, StoreError> {
        let service = CFString::from_str(names.service);
        let account = CFString::from_str(&names.account);
        // SAFETY: These are immutable Core Foundation and Security constants.
        let query = unsafe {
            let return_data = kCFBooleanTrue.ok_or(SecretError::Failed)?;
            CFDictionary::<CFString, CFType>::from_slices(
                &[kSecClass, kSecAttrService, kSecAttrAccount, kSecReturnData],
                &[kSecClassGenericPassword, &service, &account, return_data],
            )
        };
        let mut result = ptr::null();
        // SAFETY: The query has valid attributes and result is a valid out-pointer.
        let status = unsafe { SecItemCopyMatching(query.as_opaque(), &mut result) };
        if status == errSecItemNotFound {
            return Ok(None);
        }
        check_status(status)?;
        let result = NonNull::new(result.cast_mut()).ok_or(SecretError::Failed)?;
        // SAFETY: SecItemCopyMatching transfers ownership of the result.
        let data = unsafe { CFRetained::from_raw(result) }
            .downcast::<CFData>()
            .map_err(|_| SecretError::Failed)?;
        // The Security framework returns immutable CFData without a writable buffer.
        Ok(Some(Secret(Zeroizing::new(data.to_vec()))))
    }

    pub(super) async fn delete(names: EntryNames<'_>) -> Result<(), StoreError> {
        let query = query(&names);
        // SAFETY: The query has valid generic password attributes.
        let status = unsafe { SecItemDelete(query.as_opaque()) };
        if status == errSecItemNotFound {
            return Ok(());
        }
        check_status(status)
    }

    #[allow(non_upper_case_globals)] // Security framework status names.
    fn check_status(status: i32) -> Result<(), StoreError> {
        let kind = match status {
            errSecSuccess => return Ok(()),
            errSecUserCanceled
            | errSecAuthFailed
            | errSecInteractionNotAllowed
            | errSecInteractionRequired
            | errSecNoAccessForItem => SecretError::Denied,
            // Unavailable: errSecNotAvailable, errSecNoSuchKeychain, or errSecNoDefaultKeychain.
            errSecNotAvailable | errSecNoSuchKeychain | errSecNoDefaultKeychain => {
                SecretError::Unavailable
            }
            _ => SecretError::Failed,
        };
        Err(StoreError {
            kind,
            status: Some(status),
        })
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod platform {
    use super::{Secret, SecretError, StoreError, Zeroizing};
    use oo7::dbus::{Collection, Error, Service, ServiceError};
    use oo7::zbus::{self, DBusError as _};
    use std::io::ErrorKind;

    pub(super) struct EntryNames<'a> {
        pub(super) label: String,
        pub(super) attributes: [(&'static str, &'a str); 3],
    }

    pub(super) fn entry_names<'a>(
        identifier: &'a str,
        product_name: &str,
        extension: &'a str,
        key: &'a str,
    ) -> EntryNames<'a> {
        EntryNames {
            label: format!("{product_name}: {extension}/{key}"),
            attributes: [
                ("application", identifier),
                ("extension", extension),
                ("key", key),
            ],
        }
    }

    async fn unlock(collection: &Collection) -> Result<(), StoreError> {
        collection.unlock(None).await?;
        if collection.is_locked().await? {
            return Err(SecretError::Denied.into());
        }
        Ok(())
    }

    pub(super) async fn set(
        names: EntryNames<'_>,
        value: Zeroizing<Vec<u8>>,
    ) -> Result<(), StoreError> {
        // Keep the service alive until the collection and its items are dropped
        // so oo7 can close the session.
        let service = Service::new().await?;
        let collection = service.default_collection().await?;
        unlock(&collection).await?;
        collection
            .create_item(&names.label, &names.attributes, value, true, None)
            .await?;
        Ok(())
    }

    pub(super) async fn get(names: EntryNames<'_>) -> Result<Option<Secret>, StoreError> {
        let service = Service::new().await?;
        let Some(collection) = service.with_alias(Service::DEFAULT_COLLECTION).await? else {
            return Ok(None);
        };
        unlock(&collection).await?;
        for item in collection.search_items(&names.attributes).await? {
            let read = async {
                item.unlock(None).await?;
                item.secret().await
            };
            match read.await {
                Ok(secret) => {
                    // oo7::Secret also wipes its buffer on drop.
                    return Ok(Some(Secret(Zeroizing::new(secret.as_bytes().to_vec()))));
                }
                Err(Error::Deleted | Error::Service(ServiceError::NoSuchObject(_))) => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(None)
    }

    pub(super) async fn delete(names: EntryNames<'_>) -> Result<(), StoreError> {
        let service = Service::new().await?;
        let Some(collection) = service.with_alias(Service::DEFAULT_COLLECTION).await? else {
            return Ok(());
        };
        unlock(&collection).await?;
        for item in collection.search_items(&names.attributes).await? {
            let delete = async {
                item.unlock(None).await?;
                item.delete(None).await
            };
            match delete.await {
                Ok(()) | Err(Error::Deleted | Error::Service(ServiceError::NoSuchObject(_))) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    impl From<Error> for StoreError {
        fn from(error: Error) -> Self {
            match error {
                Error::Dismissed | Error::Service(ServiceError::IsLocked(_)) => SecretError::Denied,
                Error::ZBus(error) | Error::Service(ServiceError::ZBus(error)) => map_dbus(&error),
                Error::IO(error) if error.kind() != ErrorKind::TimedOut => SecretError::Unavailable,
                _ => SecretError::Failed,
            }
            .into()
        }
    }

    fn map_dbus(error: &zbus::Error) -> SecretError {
        use zbus::Error::{Address, Connection, FDO, Handshake, InputOutput, MethodError};

        match error {
            Address(_) | Connection(..) | Handshake(_) => SecretError::Unavailable,
            InputOutput(error) if error.kind() != ErrorKind::TimedOut => SecretError::Unavailable,
            MethodError(name, ..) => map_dbus_name(name.as_str()),
            FDO(error) => match error.as_ref() {
                zbus::fdo::Error::ZBus(error) => map_dbus(error),
                error => map_dbus_name(error.name().as_str()),
            },
            _ => SecretError::Failed,
        }
    }

    fn map_dbus_name(name: &str) -> SecretError {
        match name {
            "org.freedesktop.DBus.Error.AccessDenied" => SecretError::Denied,
            "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.NameHasNoOwner"
            | "org.freedesktop.DBus.Error.NoServer"
            | "org.freedesktop.DBus.Error.Disconnected"
            | "org.freedesktop.DBus.Error.BadAddress"
            | "org.freedesktop.DBus.Error.IOError" => SecretError::Unavailable,
            name if name.starts_with("org.freedesktop.DBus.Error.Spawn.") => {
                SecretError::Unavailable
            }
            _ => SecretError::Failed,
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{Secret, SecretError, StoreError, Zeroizing};
    use std::{ptr, ptr::NonNull, slice};
    use windows::{
        Win32::{
            Foundation::{
                ERROR_ACCESS_DENIED, ERROR_CANCELLED, ERROR_NO_LOGON_SERVERS,
                ERROR_NO_SUCH_LOGON_SESSION, ERROR_NOT_FOUND, ERROR_SERVICE_NOT_ACTIVE,
            },
            Security::Credentials::{
                CRED_MAX_GENERIC_TARGET_NAME_LENGTH, CRED_MAX_USERNAME_LENGTH,
                CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree,
                CredReadW, CredWriteW,
            },
        },
        core::{Error, PCWSTR, PWSTR},
    };
    use zeroize::Zeroize;

    pub(super) struct EntryNames<'a> {
        pub(super) target: String,
        pub(super) user_name: &'a str,
    }

    pub(super) fn entry_names<'a>(
        identifier: &str,
        _product_name: &str,
        extension: &'a str,
        key: &str,
    ) -> EntryNames<'a> {
        EntryNames {
            target: format!("{identifier}/{extension}/{key}"),
            user_name: extension,
        }
    }

    fn wide_string(value: &str, max_len: u32) -> Result<Vec<u16>, StoreError> {
        let mut units: Vec<_> = value.encode_utf16().collect();
        if units.len() > max_len as usize {
            return Err(SecretError::Failed.into());
        }
        units.push(0);
        Ok(units)
    }

    struct OwnedCredential(NonNull<CREDENTIALW>);

    impl Drop for OwnedCredential {
        fn drop(&mut self) {
            // SAFETY: The credential and any non-null blob belong to this guard
            // and remain valid until CredFree releases the allocation.
            unsafe {
                let credential = self.0.as_ref();
                if credential.CredentialBlobSize != 0 && !credential.CredentialBlob.is_null() {
                    slice::from_raw_parts_mut(
                        credential.CredentialBlob,
                        credential.CredentialBlobSize as usize,
                    )
                    .zeroize();
                }
                CredFree(self.0.as_ptr().cast());
            }
        }
    }

    pub(super) async fn set(
        names: EntryNames<'_>,
        mut value: Zeroizing<Vec<u8>>,
    ) -> Result<(), StoreError> {
        let mut target = wide_string(&names.target, CRED_MAX_GENERIC_TARGET_NAME_LENGTH)?;
        let mut user_name = wide_string(names.user_name, CRED_MAX_USERNAME_LENGTH)?;
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR::from_raw(target.as_mut_ptr()),
            CredentialBlobSize: value.len() as u32,
            CredentialBlob: if value.is_empty() {
                ptr::null_mut()
            } else {
                value.as_mut_ptr()
            },
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR::from_raw(user_name.as_mut_ptr()),
            ..CREDENTIALW::default()
        };
        // SAFETY: The buffers and strings are valid for the duration of the call.
        unsafe { CredWriteW(&credential, 0) }.map_err(map_error)
    }

    pub(super) async fn get(names: EntryNames<'_>) -> Result<Option<Secret>, StoreError> {
        let target = wide_string(&names.target, CRED_MAX_GENERIC_TARGET_NAME_LENGTH)?;
        let mut credential: *mut CREDENTIALW = ptr::null_mut();
        // SAFETY: The target is NUL-terminated and credential is a valid out-pointer.
        match unsafe {
            CredReadW(
                PCWSTR::from_raw(target.as_ptr()),
                CRED_TYPE_GENERIC,
                None,
                &mut credential,
            )
        } {
            Err(error) if missing(&error) => return Ok(None),
            Err(error) => return Err(map_error(error)),
            Ok(()) => {}
        }
        let credential = OwnedCredential(NonNull::new(credential).ok_or(SecretError::Failed)?);
        // SAFETY: The successful CredReadW call returned this credential.
        let data = unsafe { credential.0.as_ref() };
        let blob_size = data.CredentialBlobSize as usize;
        let blob = data.CredentialBlob;
        let bytes = if blob_size == 0 {
            &[]
        } else if blob.is_null() {
            return Err(SecretError::Failed.into());
        } else {
            // SAFETY: CredReadW returned a blob of this size, owned by the guard.
            unsafe { slice::from_raw_parts(blob, blob_size) }
        };
        Ok(Some(Secret(Zeroizing::new(bytes.to_vec()))))
    }

    pub(super) async fn delete(names: EntryNames<'_>) -> Result<(), StoreError> {
        let target = wide_string(&names.target, CRED_MAX_GENERIC_TARGET_NAME_LENGTH)?;
        // SAFETY: The target is NUL-terminated and valid for the call.
        match unsafe { CredDeleteW(PCWSTR::from_raw(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
            Ok(()) => Ok(()),
            Err(error) if missing(&error) => Ok(()),
            Err(error) => Err(map_error(error)),
        }
    }

    fn missing(error: &Error) -> bool {
        error.code() == ERROR_NOT_FOUND.to_hresult()
    }

    fn map_error(error: Error) -> StoreError {
        let status = error.code();
        // Denied: ERROR_ACCESS_DENIED or ERROR_CANCELLED.
        let kind = if [ERROR_ACCESS_DENIED, ERROR_CANCELLED]
            .iter()
            .any(|code| status == code.to_hresult())
        {
            SecretError::Denied
        // Unavailable: ERROR_NO_SUCH_LOGON_SESSION, ERROR_NO_LOGON_SERVERS,
        // or ERROR_SERVICE_NOT_ACTIVE.
        } else if [
            ERROR_NO_SUCH_LOGON_SESSION,
            ERROR_NO_LOGON_SERVERS,
            ERROR_SERVICE_NOT_ACTIVE,
        ]
        .iter()
        .any(|code| status == code.to_hresult())
        {
            SecretError::Unavailable
        } else {
            SecretError::Failed
        };
        StoreError {
            kind,
            status: Some(status.0),
        }
    }
}

#[cfg(test)]
use platform::entry_names;
#[cfg(test)]
mod tests;
