use std::{
    ffi::{OsStr, OsString},
    fmt,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::Path,
};

use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
            REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW,
            RegQueryValueExW, RegSetValueExW,
        },
    },
    core::{PCWSTR, w},
};

const VALUE_NAME: &str = "Gradial";
const MAX_REGISTRY_VALUE_BYTES: u32 = 32 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartAtLoginStatus {
    Disabled,
    Enabled,
    NeedsRepair,
}

impl StartAtLoginStatus {
    pub(crate) const fn log_name(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Enabled => "enabled",
            Self::NeedsRepair => "needs_repair",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartAtLoginAction {
    Enable,
    Disable,
    Repair,
}

impl StartAtLoginAction {
    pub(crate) const fn log_name(self) -> &'static str {
        match self {
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Repair => "repair",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartAtLoginError {
    ExecutableUnavailable,
    RegistryRead(u32),
    RegistryWrite(u32),
    RegistryDelete(u32),
    Verification,
}

impl StartAtLoginError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::ExecutableUnavailable => "executable_unavailable",
            Self::RegistryRead(_) => "registry_read_failed",
            Self::RegistryWrite(_) => "registry_write_failed",
            Self::RegistryDelete(_) => "registry_delete_failed",
            Self::Verification => "registry_verification_failed",
        }
    }

    pub(crate) const fn native_error_code(self) -> u32 {
        match self {
            Self::RegistryRead(code) | Self::RegistryWrite(code) | Self::RegistryDelete(code) => {
                code
            }
            Self::ExecutableUnavailable | Self::Verification => 0,
        }
    }
}

impl fmt::Display for StartAtLoginError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for StartAtLoginError {}

pub(crate) struct StartAtLoginService {
    value_name: OsString,
    expected_command: Result<OsString, StartAtLoginError>,
}

impl fmt::Debug for StartAtLoginService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StartAtLoginService")
            .field("value_name", &"<owned>")
            .field("expected_command", &self.expected_command.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl StartAtLoginService {
    pub(crate) fn for_current_executable() -> Self {
        let expected_command = std::env::current_exe()
            .map_err(|_| StartAtLoginError::ExecutableUnavailable)
            .and_then(|path| command_for_executable(&path));
        Self { value_name: registration_value_name(), expected_command }
    }

    pub(crate) fn inspect(&self) -> Result<StartAtLoginStatus, StartAtLoginError> {
        let expected = self.expected_command.as_deref().map_err(|error| *error)?;
        inspect_with_backend(&WindowsRegistryBackend, &self.value_name, expected)
    }

    pub(crate) fn apply(
        &self,
        action: StartAtLoginAction,
    ) -> Result<StartAtLoginStatus, StartAtLoginError> {
        let expected = self.expected_command.as_deref().map_err(|error| *error)?;
        apply_with_backend(&WindowsRegistryBackend, &self.value_name, expected, action)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StoredValue {
    String(OsString),
    Other,
}

trait RegistryBackend {
    fn read(&self, value_name: &OsStr) -> Result<Option<StoredValue>, StartAtLoginError>;
    fn write(&self, value_name: &OsStr, value: &OsStr) -> Result<(), StartAtLoginError>;
    fn delete(&self, value_name: &OsStr) -> Result<(), StartAtLoginError>;
}

fn inspect_with_backend(
    backend: &impl RegistryBackend,
    value_name: &OsStr,
    expected: &OsStr,
) -> Result<StartAtLoginStatus, StartAtLoginError> {
    Ok(match backend.read(value_name)? {
        None => StartAtLoginStatus::Disabled,
        Some(StoredValue::String(value)) if value == expected => StartAtLoginStatus::Enabled,
        Some(StoredValue::String(_) | StoredValue::Other) => StartAtLoginStatus::NeedsRepair,
    })
}

fn apply_with_backend(
    backend: &impl RegistryBackend,
    value_name: &OsStr,
    expected: &OsStr,
    action: StartAtLoginAction,
) -> Result<StartAtLoginStatus, StartAtLoginError> {
    let wanted = match action {
        StartAtLoginAction::Enable | StartAtLoginAction::Repair => {
            backend.write(value_name, expected)?;
            StartAtLoginStatus::Enabled
        }
        StartAtLoginAction::Disable => {
            backend.delete(value_name)?;
            StartAtLoginStatus::Disabled
        }
    };
    let actual = inspect_with_backend(backend, value_name, expected)?;
    if actual == wanted { Ok(actual) } else { Err(StartAtLoginError::Verification) }
}

fn command_for_executable(path: &Path) -> Result<OsString, StartAtLoginError> {
    if !path.is_absolute() || path.as_os_str().encode_wide().any(|unit| matches!(unit, 0 | 0x22)) {
        return Err(StartAtLoginError::ExecutableUnavailable);
    }
    let mut command = OsString::from("\"");
    command.push(path.as_os_str());
    command.push("\"");
    if command.encode_wide().count() >= 32_767 {
        return Err(StartAtLoginError::ExecutableUnavailable);
    }
    Ok(command)
}

fn registration_value_name() -> OsString {
    #[cfg(debug_assertions)]
    if let Some(value_name) = std::env::var_os("GRADIAL_STARTUP_VALUE_NAME")
        && valid_test_value_name(&value_name)
    {
        return value_name;
    }
    OsString::from(VALUE_NAME)
}

#[cfg(debug_assertions)]
fn valid_test_value_name(value: &OsStr) -> bool {
    let units = value.encode_wide().collect::<Vec<_>>();
    !units.is_empty()
        && units.len() <= 128
        && !units.iter().any(|unit| matches!(*unit, 0 | 0x5c))
        && value.to_string_lossy().starts_with("GradialSmoke-")
}

#[derive(Clone, Copy, Debug)]
struct WindowsRegistryBackend;

impl RegistryBackend for WindowsRegistryBackend {
    fn read(&self, value_name: &OsStr) -> Result<Option<StoredValue>, StartAtLoginError> {
        let Some(key) = open_run_key(KEY_QUERY_VALUE).map_err(StartAtLoginError::RegistryRead)?
        else {
            return Ok(None);
        };
        let value_name = wide(value_name);
        let mut value_type = REG_VALUE_TYPE::default();
        let mut byte_count = 0_u32;
        // SAFETY: `key` and the null-terminated value name remain valid for
        // this synchronous size query. No output buffer is supplied.
        let result = unsafe {
            RegQueryValueExW(
                key.handle,
                PCWSTR(value_name.as_ptr()),
                None,
                Some(&raw mut value_type),
                None,
                Some(&raw mut byte_count),
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if result != ERROR_SUCCESS {
            return Err(StartAtLoginError::RegistryRead(result.0));
        }
        if value_type != REG_SZ
            || !(2..=MAX_REGISTRY_VALUE_BYTES).contains(&byte_count)
            || !byte_count.is_multiple_of(2)
        {
            return Ok(Some(StoredValue::Other));
        }

        let mut bytes = vec![0_u8; byte_count as usize];
        let mut bytes_written = byte_count;
        // SAFETY: the output slice is live and exactly `byte_count` bytes,
        // while the key and value-name buffer remain valid for the call.
        let result = unsafe {
            RegQueryValueExW(
                key.handle,
                PCWSTR(value_name.as_ptr()),
                None,
                Some(&raw mut value_type),
                Some(bytes.as_mut_ptr()),
                Some(&raw mut bytes_written),
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if result != ERROR_SUCCESS {
            return Err(StartAtLoginError::RegistryRead(result.0));
        }
        if value_type != REG_SZ
            || bytes_written < 2
            || bytes_written > byte_count
            || !bytes_written.is_multiple_of(2)
        {
            return Ok(Some(StoredValue::Other));
        }
        bytes.truncate(bytes_written as usize);
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        let Some((&0, command)) = units.split_last() else {
            return Ok(Some(StoredValue::Other));
        };
        if command.contains(&0) {
            return Ok(Some(StoredValue::Other));
        }
        Ok(Some(StoredValue::String(OsString::from_wide(command))))
    }

    fn write(&self, value_name: &OsStr, value: &OsStr) -> Result<(), StartAtLoginError> {
        let key = create_run_key()?;
        let value_name = wide(value_name);
        let mut bytes = Vec::new();
        for unit in value.encode_wide().chain(std::iter::once(0)) {
            bytes.extend_from_slice(&unit.to_ne_bytes());
        }
        // SAFETY: the key and null-terminated name are valid, and `bytes`
        // contains the complete UTF-16 REG_SZ including its terminator.
        let result = unsafe {
            RegSetValueExW(
                key.handle,
                PCWSTR(value_name.as_ptr()),
                None,
                REG_SZ,
                Some(bytes.as_slice()),
            )
        };
        if result == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(StartAtLoginError::RegistryWrite(result.0))
        }
    }

    fn delete(&self, value_name: &OsStr) -> Result<(), StartAtLoginError> {
        let Some(key) = open_run_key(KEY_SET_VALUE).map_err(StartAtLoginError::RegistryDelete)?
        else {
            return Ok(());
        };
        let value_name = wide(value_name);
        // SAFETY: the key and null-terminated value name remain valid for the
        // duration of this synchronous deletion.
        let result = unsafe { RegDeleteValueW(key.handle, PCWSTR(value_name.as_ptr())) };
        if matches!(result, ERROR_SUCCESS | ERROR_FILE_NOT_FOUND) {
            Ok(())
        } else {
            Err(StartAtLoginError::RegistryDelete(result.0))
        }
    }
}

fn open_run_key(
    access: windows::Win32::System::Registry::REG_SAM_FLAGS,
) -> Result<Option<RegistryKey>, u32> {
    let mut handle = HKEY::default();
    // SAFETY: the subkey string is static and null-terminated, and `handle`
    // points to valid output storage.
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            access,
            &raw mut handle,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        Ok(None)
    } else if result == ERROR_SUCCESS {
        Ok(Some(RegistryKey { handle }))
    } else {
        Err(result.0)
    }
}

fn create_run_key() -> Result<RegistryKey, StartAtLoginError> {
    let mut handle = HKEY::default();
    // SAFETY: both strings are static and null-terminated, the security
    // pointer is absent, and `handle` points to valid output storage.
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            w!(""),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            None,
            &raw mut handle,
            None,
        )
    };
    if result == ERROR_SUCCESS {
        Ok(RegistryKey { handle })
    } else {
        Err(StartAtLoginError::RegistryWrite(result.0))
    }
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[derive(Debug)]
struct RegistryKey {
    handle: HKEY,
}

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: this guard owns one successful registry-key open/create.
        let _ = unsafe { RegCloseKey(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        ffi::{OsStr, OsString},
        path::Path,
    };

    use super::{
        RegistryBackend, StartAtLoginAction, StartAtLoginError, StartAtLoginService,
        StartAtLoginStatus, StoredValue, apply_with_backend, command_for_executable,
        inspect_with_backend,
    };

    #[derive(Debug, Default)]
    struct FakeRegistry {
        value: RefCell<Option<StoredValue>>,
        writes: RefCell<Vec<(OsString, OsString)>>,
        deletes: RefCell<Vec<OsString>>,
        read_error: RefCell<Option<StartAtLoginError>>,
        write_error: RefCell<Option<StartAtLoginError>>,
        delete_error: RefCell<Option<StartAtLoginError>>,
        ignore_write: bool,
        ignore_delete: bool,
    }

    impl RegistryBackend for FakeRegistry {
        fn read(&self, _value_name: &OsStr) -> Result<Option<StoredValue>, StartAtLoginError> {
            if let Some(error) = *self.read_error.borrow() {
                return Err(error);
            }
            Ok(self.value.borrow().clone())
        }

        fn write(&self, value_name: &OsStr, value: &OsStr) -> Result<(), StartAtLoginError> {
            if let Some(error) = *self.write_error.borrow() {
                return Err(error);
            }
            self.writes.borrow_mut().push((value_name.to_owned(), value.to_owned()));
            if !self.ignore_write {
                *self.value.borrow_mut() = Some(StoredValue::String(value.to_owned()));
            }
            Ok(())
        }

        fn delete(&self, value_name: &OsStr) -> Result<(), StartAtLoginError> {
            if let Some(error) = *self.delete_error.borrow() {
                return Err(error);
            }
            self.deletes.borrow_mut().push(value_name.to_owned());
            if !self.ignore_delete {
                *self.value.borrow_mut() = None;
            }
            Ok(())
        }
    }

    const VALUE: &str = "Gradial";
    const EXPECTED: &str = r#""C:\Program Files\Gradial\gradial.exe""#;

    #[test]
    fn missing_exact_and_mismatched_values_have_distinct_statuses() {
        let registry = FakeRegistry::default();
        assert_eq!(
            inspect_with_backend(&registry, OsStr::new(VALUE), OsStr::new(EXPECTED)),
            Ok(StartAtLoginStatus::Disabled)
        );

        *registry.value.borrow_mut() = Some(StoredValue::String(OsString::from(EXPECTED)));
        assert_eq!(
            inspect_with_backend(&registry, OsStr::new(VALUE), OsStr::new(EXPECTED)),
            Ok(StartAtLoginStatus::Enabled)
        );

        for stale in [
            StoredValue::String(OsString::from(r#""C:\Old\gradial.exe""#)),
            StoredValue::String(OsString::from(format!("{EXPECTED} --extra"))),
            StoredValue::Other,
        ] {
            *registry.value.borrow_mut() = Some(stale);
            assert_eq!(
                inspect_with_backend(&registry, OsStr::new(VALUE), OsStr::new(EXPECTED)),
                Ok(StartAtLoginStatus::NeedsRepair)
            );
        }
    }

    #[test]
    fn enable_repair_and_disable_touch_only_the_owned_value() {
        let registry = FakeRegistry::default();
        assert_eq!(
            apply_with_backend(
                &registry,
                OsStr::new(VALUE),
                OsStr::new(EXPECTED),
                StartAtLoginAction::Enable,
            ),
            Ok(StartAtLoginStatus::Enabled)
        );
        *registry.value.borrow_mut() =
            Some(StoredValue::String(OsString::from(r#""C:\Stale\gradial.exe""#)));
        assert_eq!(
            apply_with_backend(
                &registry,
                OsStr::new(VALUE),
                OsStr::new(EXPECTED),
                StartAtLoginAction::Repair,
            ),
            Ok(StartAtLoginStatus::Enabled)
        );
        assert_eq!(
            apply_with_backend(
                &registry,
                OsStr::new(VALUE),
                OsStr::new(EXPECTED),
                StartAtLoginAction::Disable,
            ),
            Ok(StartAtLoginStatus::Disabled)
        );
        assert_eq!(
            registry.writes.borrow().as_slice(),
            [
                (OsString::from(VALUE), OsString::from(EXPECTED)),
                (OsString::from(VALUE), OsString::from(EXPECTED)),
            ]
        );
        assert_eq!(registry.deletes.borrow().as_slice(), [OsString::from(VALUE)]);
    }

    #[test]
    fn backend_failures_are_stable_and_verification_is_required() {
        let registry = FakeRegistry::default();
        *registry.write_error.borrow_mut() = Some(StartAtLoginError::RegistryWrite(5));
        assert_eq!(
            apply_with_backend(
                &registry,
                OsStr::new(VALUE),
                OsStr::new(EXPECTED),
                StartAtLoginAction::Enable,
            ),
            Err(StartAtLoginError::RegistryWrite(5))
        );

        *registry.write_error.borrow_mut() = None;
        *registry.read_error.borrow_mut() = Some(StartAtLoginError::RegistryRead(6));
        assert_eq!(
            inspect_with_backend(&registry, OsStr::new(VALUE), OsStr::new(EXPECTED)),
            Err(StartAtLoginError::RegistryRead(6))
        );

        let ignored_write = FakeRegistry { ignore_write: true, ..FakeRegistry::default() };
        assert_eq!(
            apply_with_backend(
                &ignored_write,
                OsStr::new(VALUE),
                OsStr::new(EXPECTED),
                StartAtLoginAction::Enable,
            ),
            Err(StartAtLoginError::Verification)
        );

        let ignored_delete = FakeRegistry {
            value: RefCell::new(Some(StoredValue::String(OsString::from(EXPECTED)))),
            ignore_delete: true,
            ..FakeRegistry::default()
        };
        assert_eq!(
            apply_with_backend(
                &ignored_delete,
                OsStr::new(VALUE),
                OsStr::new(EXPECTED),
                StartAtLoginAction::Disable,
            ),
            Err(StartAtLoginError::Verification)
        );
    }

    #[test]
    fn executable_command_is_absolute_quoted_and_argument_free() {
        assert_eq!(
            command_for_executable(Path::new(r"C:\Program Files\Gradial\gradial.exe")),
            Ok(OsString::from(EXPECTED))
        );
        assert_eq!(
            command_for_executable(Path::new(r"gradial.exe")),
            Err(StartAtLoginError::ExecutableUnavailable)
        );
        assert_eq!(
            command_for_executable(Path::new("C:\\Invalid\"Name\\gradial.exe")),
            Err(StartAtLoginError::ExecutableUnavailable)
        );

        let debug = format!("{:?}", StartAtLoginService::for_current_executable());
        assert!(!debug.to_ascii_lowercase().contains("gradial.exe"));
        assert!(debug.contains("<redacted>"));
    }
}
