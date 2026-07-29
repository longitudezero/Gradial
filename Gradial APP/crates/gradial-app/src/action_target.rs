use std::mem::size_of;

use gradial_core::InvocationContext;
use windows::Win32::{
    Foundation::{CloseHandle, FILETIME, HANDLE, HWND},
    Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL,
        TOKEN_QUERY, TokenIntegrityLevel,
    },
    System::Threading::{
        GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
        PROCESS_QUERY_LIMITED_INFORMATION,
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, IsWindow},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TargetValidationFailure {
    StaleTarget,
    ElevatedTarget,
    InspectionFailed,
}

pub(crate) fn validate(target: &InvocationContext) -> Result<(), TargetValidationFailure> {
    validate_with(&WindowsTargetEnvironment, target)
}

trait TargetEnvironment {
    fn window_exists(&self, window: usize) -> bool;
    fn window_process_id(&self, window: usize) -> u32;
    fn process_start_time(&self, process_id: u32) -> Option<u64>;
    fn foreground_window(&self) -> usize;
    fn process_integrity(&self, process_id: u32) -> Result<u32, ()>;
    fn gradial_integrity(&self) -> Result<u32, ()>;
}

fn validate_with(
    environment: &impl TargetEnvironment,
    target: &InvocationContext,
) -> Result<(), TargetValidationFailure> {
    if !target_is_current(environment, target) {
        return Err(TargetValidationFailure::StaleTarget);
    }

    let target_integrity = environment
        .process_integrity(target.foreground_pid)
        .map_err(|()| TargetValidationFailure::InspectionFailed)?;
    let gradial_integrity =
        environment.gradial_integrity().map_err(|()| TargetValidationFailure::InspectionFailed)?;
    if !target_is_current(environment, target) {
        return Err(TargetValidationFailure::StaleTarget);
    }
    if target_integrity > gradial_integrity {
        return Err(TargetValidationFailure::ElevatedTarget);
    }

    Ok(())
}

fn target_is_current(environment: &impl TargetEnvironment, target: &InvocationContext) -> bool {
    !(target.foreground_window == 0
        || target.foreground_pid == 0
        || target.process_start_time == 0
        || !environment.window_exists(target.foreground_window)
        || environment.window_process_id(target.foreground_window) != target.foreground_pid
        || environment.process_start_time(target.foreground_pid) != Some(target.process_start_time)
        || environment.foreground_window() != target.foreground_window)
}

#[derive(Clone, Copy, Debug)]
struct WindowsTargetEnvironment;

impl TargetEnvironment for WindowsTargetEnvironment {
    fn window_exists(&self, window: usize) -> bool {
        // SAFETY: the address is the HWND captured from Windows at activation.
        unsafe { IsWindow(Some(hwnd(window))).as_bool() }
    }

    fn window_process_id(&self, window: usize) -> u32 {
        let mut process_id = 0_u32;
        // SAFETY: the address is the captured HWND and the output is a live
        // local. A destroyed or replaced window produces zero or a mismatch.
        unsafe {
            GetWindowThreadProcessId(hwnd(window), Some(&raw mut process_id));
        }
        process_id
    }

    fn process_start_time(&self, process_id: u32) -> Option<u64> {
        let process = OwnedHandle::open_process(process_id).ok()?;
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: the process handle has limited query rights and all outputs
        // point to live locals.
        unsafe {
            GetProcessTimes(
                process.handle,
                &raw mut creation,
                &raw mut exit,
                &raw mut kernel,
                &raw mut user,
            )
            .ok()?;
        }
        Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
    }

    fn foreground_window(&self) -> usize {
        // SAFETY: GetForegroundWindow has no preconditions.
        unsafe { GetForegroundWindow().0.addr() }
    }

    fn process_integrity(&self, process_id: u32) -> Result<u32, ()> {
        let process = OwnedHandle::open_process(process_id).map_err(|_| ())?;
        integrity_rid(process.handle)
    }

    fn gradial_integrity(&self) -> Result<u32, ()> {
        // SAFETY: GetCurrentProcess returns a pseudo-handle that remains valid
        // for the process lifetime and must not be closed.
        integrity_rid(unsafe { GetCurrentProcess() })
    }
}

fn integrity_rid(process: HANDLE) -> Result<u32, ()> {
    let mut token = HANDLE::default();
    // SAFETY: `process` is a live process handle or the current-process
    // pseudo-handle, and the output points to a live local.
    unsafe {
        OpenProcessToken(process, TOKEN_QUERY, &raw mut token).map_err(|_| ())?;
    }
    let token = OwnedHandle { handle: token };

    let mut required_bytes = 0_u32;
    // SAFETY: a zero-length query with a null buffer obtains the required
    // length. Failure with ERROR_INSUFFICIENT_BUFFER is expected.
    let _size_query = unsafe {
        GetTokenInformation(token.handle, TokenIntegrityLevel, None, 0, &raw mut required_bytes)
    };
    if required_bytes < u32::try_from(size_of::<TOKEN_MANDATORY_LABEL>()).map_err(|_| ())? {
        return Err(());
    }

    let required_usize = usize::try_from(required_bytes).map_err(|_| ())?;
    let word_count = required_usize.div_ceil(size_of::<usize>());
    let mut storage = vec![0_usize; word_count];
    // SAFETY: the usize-backed buffer has sufficient size and alignment for
    // TOKEN_MANDATORY_LABEL and remains live through all SID reads.
    unsafe {
        GetTokenInformation(
            token.handle,
            TokenIntegrityLevel,
            Some(storage.as_mut_ptr().cast()),
            required_bytes,
            &raw mut required_bytes,
        )
        .map_err(|_| ())?;
    }

    // SAFETY: GetTokenInformation initialized the aligned buffer as a
    // TOKEN_MANDATORY_LABEL whose SID is contained in the same live buffer.
    let label = unsafe { &*storage.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    if label.Label.Sid.is_invalid() {
        return Err(());
    }
    // SAFETY: the token API returned a valid mandatory-label SID.
    let count_pointer = unsafe { GetSidSubAuthorityCount(label.Label.Sid) };
    if count_pointer.is_null() {
        return Err(());
    }
    // SAFETY: the count pointer belongs to the validated SID.
    let count = unsafe { *count_pointer };
    if count == 0 {
        return Err(());
    }
    // SAFETY: `count - 1` identifies the mandatory integrity RID, the final
    // sub-authority in this token SID.
    let rid_pointer = unsafe { GetSidSubAuthority(label.Label.Sid, u32::from(count - 1)) };
    if rid_pointer.is_null() {
        return Err(());
    }
    // SAFETY: the sub-authority pointer belongs to the live SID buffer.
    Ok(unsafe { *rid_pointer })
}

#[derive(Debug)]
struct OwnedHandle {
    handle: HANDLE,
}

impl OwnedHandle {
    fn open_process(process_id: u32) -> windows::core::Result<Self> {
        // SAFETY: the requested access is read-only and the PID came from the
        // captured target or its live HWND.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }?;
        Ok(Self { handle })
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this object uniquely owns handles returned by OpenProcess or
        // OpenProcessToken. Current-process pseudo-handles are never wrapped.
        if let Err(error) = unsafe { CloseHandle(self.handle) } {
            log::warn!(
                target: "actions",
                "component=actions event_code=target_handle_cleanup_failed \
                 result_code=failure config_revision=0 error={error}"
            );
        }
    }
}

fn hwnd(address: usize) -> HWND {
    HWND(std::ptr::with_exposed_provenance_mut(address))
}

#[cfg(test)]
mod tests {
    use gradial_core::InvocationContext;

    use super::{TargetEnvironment, TargetValidationFailure, validate_with};

    #[derive(Clone, Copy, Debug)]
    struct FakeEnvironment {
        window_exists: bool,
        window_process_id: u32,
        process_start_time: Option<u64>,
        foreground_window: usize,
        process_integrity: Result<u32, ()>,
        gradial_integrity: Result<u32, ()>,
    }

    impl Default for FakeEnvironment {
        fn default() -> Self {
            Self {
                window_exists: true,
                window_process_id: 41,
                process_start_time: Some(73),
                foreground_window: 29,
                process_integrity: Ok(0x2000),
                gradial_integrity: Ok(0x2000),
            }
        }
    }

    impl TargetEnvironment for FakeEnvironment {
        fn window_exists(&self, _window: usize) -> bool {
            self.window_exists
        }

        fn window_process_id(&self, _window: usize) -> u32 {
            self.window_process_id
        }

        fn process_start_time(&self, _process_id: u32) -> Option<u64> {
            self.process_start_time
        }

        fn foreground_window(&self) -> usize {
            self.foreground_window
        }

        fn process_integrity(&self, _process_id: u32) -> Result<u32, ()> {
            self.process_integrity
        }

        fn gradial_integrity(&self) -> Result<u32, ()> {
            self.gradial_integrity
        }
    }

    fn target() -> InvocationContext {
        InvocationContext {
            foreground_window: 29,
            foreground_pid: 41,
            process_start_time: 73,
            ..InvocationContext::default()
        }
    }

    #[test]
    fn matching_live_target_at_equal_or_lower_integrity_is_accepted() {
        assert_eq!(validate_with(&FakeEnvironment::default(), &target()), Ok(()));
        let lower_target =
            FakeEnvironment { process_integrity: Ok(0x1000), ..FakeEnvironment::default() };
        assert_eq!(validate_with(&lower_target, &target()), Ok(()));
    }

    #[test]
    fn missing_replaced_reused_or_background_windows_are_stale() {
        let scenarios = [
            FakeEnvironment { window_exists: false, ..FakeEnvironment::default() },
            FakeEnvironment { window_process_id: 42, ..FakeEnvironment::default() },
            FakeEnvironment { process_start_time: Some(74), ..FakeEnvironment::default() },
            FakeEnvironment { foreground_window: 30, ..FakeEnvironment::default() },
        ];
        for environment in scenarios {
            assert_eq!(
                validate_with(&environment, &target()),
                Err(TargetValidationFailure::StaleTarget)
            );
        }
        assert_eq!(
            validate_with(&FakeEnvironment::default(), &InvocationContext::default()),
            Err(TargetValidationFailure::StaleTarget)
        );
    }

    #[test]
    fn higher_integrity_and_unreadable_integrity_are_distinct() {
        let elevated =
            FakeEnvironment { process_integrity: Ok(0x3000), ..FakeEnvironment::default() };
        assert_eq!(
            validate_with(&elevated, &target()),
            Err(TargetValidationFailure::ElevatedTarget)
        );
        let unreadable =
            FakeEnvironment { process_integrity: Err(()), ..FakeEnvironment::default() };
        assert_eq!(
            validate_with(&unreadable, &target()),
            Err(TargetValidationFailure::InspectionFailed)
        );
    }
}
