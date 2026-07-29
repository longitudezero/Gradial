use std::{
    collections::VecDeque,
    mem::size_of,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use anyhow::{Context, Result, bail};
use gradial_core::{ActionErrorCode, ActionResult};
use windows::{
    Win32::{
        Foundation::{HWND, POINT},
        UI::{
            Shell::{
                NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_ERROR, NIIF_NOSOUND,
                NIIF_RESPECT_QUIET_TIME, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
                NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, HMENU, IDI_ERROR,
                IDI_INFORMATION, IDI_WARNING, LoadIconW, MF_DISABLED, MF_ENABLED, MF_GRAYED,
                MF_POPUP, MF_SEPARATOR, MF_STRING, SetForegroundWindow, TPM_RETURNCMD,
                TPM_RIGHTBUTTON, TrackPopupMenu, WM_CONTEXTMENU,
            },
        },
    },
    core::PCWSTR,
};

pub(crate) const CALLBACK_MESSAGE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 0x246;
const ICON_ID: u32 = 1;
const RETRY_COMMAND_ID: usize = 1;
const CANCEL_ACTION_COMMAND_ID: usize = 2;
const EXIT_COMMAND_ID: usize = 3;
const CLEAR_ACTIVITY_COMMAND_ID: usize = 4;
const ACTIVITY_CAPACITY: usize = 20;
const NOTIFICATION_CAPACITY: usize = 20;
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
static PENDING_COMMAND: AtomicU8 = AtomicU8::new(0);
static CURRENT_HEALTH: AtomicU8 = AtomicU8::new(TrayHealth::Recovering as u8);
static ACTION_RUNNING: AtomicBool = AtomicBool::new(false);
static ACTION_ACTIVITY: Mutex<ActivityStore> = Mutex::new(ActivityStore::new());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivityRecord {
    Cancelled,
    Failure(ActionErrorCode),
}

impl ActivityRecord {
    fn menu_text(self) -> String {
        match self {
            Self::Cancelled => "Cancelled — The action was stopped.".to_owned(),
            Self::Failure(code) => format!("Failed — {}", failure_message(code)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FailureNotification {
    code: ActionErrorCode,
    config_revision: u64,
}

impl FailureNotification {
    pub(crate) const fn error_code(self) -> ActionErrorCode {
        self.code
    }
}

#[derive(Debug)]
struct ActivityStore {
    records: VecDeque<ActivityRecord>,
    notifications: VecDeque<FailureNotification>,
}

impl ActivityStore {
    const fn new() -> Self {
        Self { records: VecDeque::new(), notifications: VecDeque::new() }
    }

    fn record(&mut self, result: &ActionResult, config_revision: u64) -> Option<ActivityRecord> {
        let record = match result {
            ActionResult::Success => return None,
            ActionResult::Cancelled => ActivityRecord::Cancelled,
            ActionResult::Failure { code, .. } => ActivityRecord::Failure(*code),
        };
        self.records.push_front(record);
        self.records.truncate(ACTIVITY_CAPACITY);

        if let ActivityRecord::Failure(code) = record {
            self.notifications.push_back(FailureNotification { code, config_revision });
            if self.notifications.len() > NOTIFICATION_CAPACITY {
                self.notifications.pop_front();
            }
        }
        Some(record)
    }

    fn clear(&mut self) {
        self.records.clear();
        self.notifications.clear();
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum TrayHealth {
    Healthy = 1,
    #[default]
    Recovering = 2,
    Faulted = 3,
}

impl TrayHealth {
    const fn status_text(self) -> &'static str {
        match self {
            Self::Healthy => "Status: Healthy",
            Self::Recovering => "Status: Recovering input",
            Self::Faulted => "Status: Input faulted",
        }
    }

    const fn tooltip(self) -> &'static str {
        match self {
            Self::Healthy => "Gradial - Healthy",
            Self::Recovering => "Gradial - Recovering input",
            Self::Faulted => "Gradial - Input faulted; open for recovery",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrayCommand {
    RetryInput,
    CancelCurrentAction,
    ClearActivity,
    Exit,
}

#[derive(Debug)]
pub(crate) struct TrayIcon {
    window_address: usize,
    health: TrayHealth,
}

impl TrayIcon {
    pub(crate) fn install(window_address: usize) -> Result<Self> {
        let mut tray = Self { window_address, health: TrayHealth::Recovering };
        tray.add().context("could not add Gradial's notification-area icon")?;
        log::info!(
            target: "tray",
            "component=tray event_code=tray_icon_ready result_code=ok config_revision=0 \
             health=Recovering"
        );
        Ok(tray)
    }

    pub(crate) const fn health(&self) -> TrayHealth {
        self.health
    }

    pub(crate) fn set_health(&mut self, health: TrayHealth) -> Result<()> {
        let data = self.icon_data(health)?;
        // SAFETY: `data` is fully initialized and identifies this live tray
        // icon for the duration of the call.
        if !unsafe { Shell_NotifyIconW(NIM_MODIFY, &raw const data) }.as_bool() {
            bail!("Windows could not update Gradial's notification-area icon");
        }
        self.health = health;
        CURRENT_HEALTH.store(health as u8, Ordering::Release);
        log::info!(
            target: "tray",
            "component=tray event_code=tray_health_changed result_code=ok config_revision=0 \
             health={health:?}"
        );
        Ok(())
    }

    pub(crate) fn readd(&mut self) -> Result<()> {
        self.add().context("could not restore Gradial's notification-area icon")?;
        log::info!(
            target: "tray",
            "component=tray event_code=tray_icon_restored result_code=ok config_revision=0 \
             health={:?}",
            self.health
        );
        Ok(())
    }

    pub(crate) fn show_failure_notification(
        &self,
        notification: FailureNotification,
    ) -> Result<()> {
        let data = failure_notification_data(self.window_address, notification)?;
        // SAFETY: `data` is fully initialized and identifies this live tray
        // icon. NIF_INFO asks the shell to display a notification without
        // activating or focusing Gradial's controller window.
        if !unsafe { Shell_NotifyIconW(NIM_MODIFY, &raw const data) }.as_bool() {
            bail!("Windows could not show Gradial's failure notification");
        }
        log::info!(
            target: "tray",
            "component=tray event_code=action_failure_notification_shown result_code=ok \
             error_code={} config_revision={}",
            notification.code.as_str(),
            notification.config_revision
        );
        Ok(())
    }

    fn add(&mut self) -> Result<()> {
        CURRENT_HEALTH.store(self.health as u8, Ordering::Release);
        let mut data = self.icon_data(self.health)?;
        // SAFETY: `data` is fully initialized and references a live controller
        // window. The system icon handle is shared and remains valid.
        if !unsafe { Shell_NotifyIconW(NIM_ADD, &raw const data) }.as_bool() {
            bail!("Shell_NotifyIconW(NIM_ADD) failed");
        }
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        // SAFETY: this updates the callback contract for the icon just added.
        if !unsafe { Shell_NotifyIconW(NIM_SETVERSION, &raw const data) }.as_bool() {
            // SAFETY: best-effort cleanup of the icon added above.
            let _deleted = unsafe { Shell_NotifyIconW(NIM_DELETE, &raw const data) };
            bail!("Shell_NotifyIconW(NIM_SETVERSION) failed");
        }
        Ok(())
    }

    fn icon_data(&self, health: TrayHealth) -> Result<NOTIFYICONDATAW> {
        let icon_name = match health {
            TrayHealth::Healthy => IDI_INFORMATION,
            TrayHealth::Recovering => IDI_WARNING,
            TrayHealth::Faulted => IDI_ERROR,
        };
        // SAFETY: these are shared system icon identifiers.
        let icon = unsafe { LoadIconW(None, icon_name) }.context("could not load a system icon")?;
        let mut data = NOTIFYICONDATAW {
            cbSize: u32::try_from(size_of::<NOTIFYICONDATAW>())
                .context("notification icon data size does not fit u32")?,
            hWnd: hwnd(self.window_address),
            uID: ICON_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: CALLBACK_MESSAGE,
            hIcon: icon,
            ..Default::default()
        };
        write_wide_buffer(&mut data.szTip, health.tooltip());
        Ok(data)
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        let data = NOTIFYICONDATAW {
            cbSize: u32::try_from(size_of::<NOTIFYICONDATAW>()).unwrap_or(u32::MAX),
            hWnd: hwnd(self.window_address),
            uID: ICON_ID,
            ..Default::default()
        };
        // SAFETY: this is a best-effort removal of the exact icon identity
        // owned by this object.
        let _deleted = unsafe { Shell_NotifyIconW(NIM_DELETE, &raw const data) };
    }
}

pub(crate) fn handle_callback(window: HWND, lparam: isize) -> bool {
    let encoded = u32::try_from(lparam).unwrap_or_default();
    let event = encoded & 0xFFFF;
    if !matches!(event, WM_CONTEXTMENU | NIN_SELECT | NIN_KEYSELECT) {
        return false;
    }

    if let Err(error) = show_menu(window) {
        log::error!(
            target: "tray",
            "component=tray event_code=tray_menu_failed result_code=failure config_revision=0 \
             error={error}"
        );
    }
    true
}

pub(crate) fn try_command() -> Option<TrayCommand> {
    match PENDING_COMMAND.swap(0, Ordering::AcqRel) {
        1 => Some(TrayCommand::RetryInput),
        2 => Some(TrayCommand::CancelCurrentAction),
        3 => Some(TrayCommand::Exit),
        4 => Some(TrayCommand::ClearActivity),
        _ => None,
    }
}

pub(crate) fn record_action_result(result: &ActionResult, config_revision: u64) {
    let mut state = activity_state();
    let Some(record) = state.record(result, config_revision) else {
        return;
    };
    let activity_count = state.records.len();
    drop(state);

    match record {
        ActivityRecord::Cancelled => {
            log::info!(
                target: "tray",
                "component=tray event_code=action_activity_recorded result_code=cancelled \
                 config_revision={config_revision} activity_count={activity_count} \
                 notification_queued=false"
            );
        }
        ActivityRecord::Failure(code) => {
            log::info!(
                target: "tray",
                "component=tray event_code=action_activity_recorded result_code=failure \
                 error_code={} config_revision={config_revision} \
                 activity_count={activity_count} notification_queued=true",
                code.as_str()
            );
        }
    }
}

pub(crate) fn try_failure_notification() -> Option<FailureNotification> {
    activity_state().notifications.pop_front()
}

pub(crate) fn clear_activity(config_revision: u64) {
    let mut state = activity_state();
    let removed_count = state.records.len();
    state.clear();
    drop(state);
    log::info!(
        target: "tray",
        "component=tray event_code=action_activity_cleared result_code=ok \
         config_revision={config_revision} removed_count={removed_count}"
    );
}

pub(crate) fn set_action_running(running: bool) {
    ACTION_RUNNING.store(running, Ordering::Release);
}

#[cfg(debug_assertions)]
pub(crate) fn request_debug_retry() {
    PENDING_COMMAND.store(1, Ordering::Release);
}

fn show_menu(window: HWND) -> Result<()> {
    let health = health_from_atomic();
    let activity = activity_state().records.iter().copied().collect::<Vec<_>>();
    // SAFETY: menu ownership remains in this function and is released before
    // returning.
    let menu = unsafe { CreatePopupMenu() }.context("could not create the tray menu")?;
    let activity_menu = match unsafe { CreatePopupMenu() } {
        Ok(activity_menu) => activity_menu,
        Err(error) => {
            // SAFETY: the parent menu has no attached submenus and is still
            // owned by this function.
            let _destroyed = unsafe { DestroyMenu(menu) };
            return Err(error).context("could not create the recent-activity menu");
        }
    };
    let mut activity_menu_attached = false;
    let result = (|| {
        let status = wide(health.status_text());
        let retry = wide("Retry input");
        let cancel_action = wide("Cancel current action");
        let exit = wide("Exit Gradial");
        // SAFETY: the menu is live and all strings remain null-terminated for
        // the duration of each call.
        unsafe {
            AppendMenuW(menu, MF_STRING | MF_DISABLED, 0, PCWSTR(status.as_ptr()))
                .context("could not add tray status")?;
            AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null())
                .context("could not add tray separator")?;
            let retry_flags = if health == TrayHealth::Recovering {
                MF_STRING | MF_GRAYED
            } else {
                MF_STRING | MF_ENABLED
            };
            AppendMenuW(menu, retry_flags, RETRY_COMMAND_ID, PCWSTR(retry.as_ptr()))
                .context("could not add Retry Input")?;
            let cancel_flags = if ACTION_RUNNING.load(Ordering::Acquire) {
                MF_STRING | MF_ENABLED
            } else {
                MF_STRING | MF_GRAYED
            };
            AppendMenuW(
                menu,
                cancel_flags,
                CANCEL_ACTION_COMMAND_ID,
                PCWSTR(cancel_action.as_ptr()),
            )
            .context("could not add Cancel current action")?;
        }
        append_activity_submenu(menu, activity_menu, &activity)?;
        activity_menu_attached = true;
        // SAFETY: the parent menu is live and the string remains
        // null-terminated for each call.
        unsafe {
            AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null())
                .context("could not add tray separator")?;
            AppendMenuW(menu, MF_STRING, EXIT_COMMAND_ID, PCWSTR(exit.as_ptr()))
                .context("could not add Exit")?;
        }

        let mut point = POINT::default();
        // SAFETY: the output points to a live local. Foreground ownership is
        // required for a notification-area menu to dismiss correctly.
        unsafe {
            GetCursorPos(&raw mut point).context("could not locate the tray menu")?;
            let _foreground = SetForegroundWindow(window);
        }
        // SAFETY: `menu` and `window` remain live for the synchronous call.
        let selected = unsafe {
            TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                None,
                window,
                None,
            )
        }
        .0;
        match usize::try_from(selected).unwrap_or_default() {
            RETRY_COMMAND_ID => PENDING_COMMAND.store(1, Ordering::Release),
            CANCEL_ACTION_COMMAND_ID => PENDING_COMMAND.store(2, Ordering::Release),
            EXIT_COMMAND_ID => PENDING_COMMAND.store(3, Ordering::Release),
            CLEAR_ACTIVITY_COMMAND_ID => PENDING_COMMAND.store(4, Ordering::Release),
            _ => {}
        }
        Ok(())
    })();

    if !activity_menu_attached {
        // SAFETY: the submenu was not transferred to its parent, so this
        // function still owns it.
        let _destroyed = unsafe { DestroyMenu(activity_menu) };
    }
    // SAFETY: this function owns the menu and destroys it exactly once.
    // Attached submenus are destroyed recursively.
    if let Err(error) = unsafe { DestroyMenu(menu) } {
        log::warn!(
            target: "tray",
            "component=tray event_code=tray_menu_cleanup_failed result_code=failure \
             config_revision=0 error={error}"
        );
    }
    result
}

fn append_activity_submenu(
    menu: HMENU,
    activity_menu: HMENU,
    activity: &[ActivityRecord],
) -> Result<()> {
    let activity_heading = wide(&format!("Recent activity ({})", activity.len()));
    let no_activity = wide("No failures or cancellations");
    let clear_activity = wide("Clear activity");
    // SAFETY: both menus are live and owned by the caller. Windows copies
    // every null-terminated label during its corresponding call.
    unsafe {
        if activity.is_empty() {
            AppendMenuW(activity_menu, MF_STRING | MF_DISABLED, 0, PCWSTR(no_activity.as_ptr()))
                .context("could not add the empty activity state")?;
        } else {
            for record in activity {
                let text = wide(&record.menu_text());
                AppendMenuW(activity_menu, MF_STRING | MF_DISABLED, 0, PCWSTR(text.as_ptr()))
                    .context("could not add a recent activity item")?;
            }
        }
        AppendMenuW(activity_menu, MF_SEPARATOR, 0, PCWSTR::null())
            .context("could not add the activity separator")?;
        let clear_flags =
            if activity.is_empty() { MF_STRING | MF_GRAYED } else { MF_STRING | MF_ENABLED };
        AppendMenuW(
            activity_menu,
            clear_flags,
            CLEAR_ACTIVITY_COMMAND_ID,
            PCWSTR(clear_activity.as_ptr()),
        )
        .context("could not add Clear activity")?;
        AppendMenuW(
            menu,
            MF_STRING | MF_POPUP,
            activity_menu.0.addr(),
            PCWSTR(activity_heading.as_ptr()),
        )
        .context("could not add the recent-activity submenu")?;
    }
    Ok(())
}

fn health_from_atomic() -> TrayHealth {
    match CURRENT_HEALTH.load(Ordering::Acquire) {
        1 => TrayHealth::Healthy,
        3 => TrayHealth::Faulted,
        _ => TrayHealth::Recovering,
    }
}

fn activity_state() -> MutexGuard<'static, ActivityStore> {
    ACTION_ACTIVITY.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn failure_notification_data(
    window_address: usize,
    notification: FailureNotification,
) -> Result<NOTIFYICONDATAW> {
    let mut data = NOTIFYICONDATAW {
        cbSize: u32::try_from(size_of::<NOTIFYICONDATAW>())
            .context("notification icon data size does not fit u32")?,
        hWnd: hwnd(window_address),
        uID: ICON_ID,
        uFlags: NIF_INFO,
        dwInfoFlags: NIIF_ERROR | NIIF_NOSOUND | NIIF_RESPECT_QUIET_TIME,
        ..Default::default()
    };
    write_wide_buffer(&mut data.szInfoTitle, "Gradial action failed");
    write_wide_buffer(&mut data.szInfo, failure_message(notification.code));
    Ok(data)
}

const fn failure_message(code: ActionErrorCode) -> &'static str {
    match code {
        ActionErrorCode::ActionBusy => "Another action is already running.",
        ActionErrorCode::ActionTimeout => "The action took too long and was stopped.",
        ActionErrorCode::ElevatedTarget => {
            "Gradial cannot control an administrator-elevated application."
        }
        ActionErrorCode::ExecutorUnavailable => "The action system is unavailable. Try again.",
        ActionErrorCode::InputInjectionFailed => {
            "Gradial could not send input to the target application."
        }
        ActionErrorCode::PathNotFound => "The configured file or folder could not be found.",
        ActionErrorCode::ProcessLaunchFailed => {
            "Gradial could not start the configured application."
        }
        ActionErrorCode::StaleTarget => "The original target window is no longer available.",
        ActionErrorCode::StepLimitExceeded => "The action contains too many steps.",
        ActionErrorCode::StepTimeout => "An action step took too long and was stopped.",
        ActionErrorCode::UnsupportedAction => "This action is not supported.",
        ActionErrorCode::WindowCommandFailed => "Gradial could not change the target window.",
    }
}

fn write_wide_buffer<const N: usize>(buffer: &mut [u16; N], value: &str) {
    buffer.fill(0);
    for (destination, source) in
        buffer.iter_mut().take(N.saturating_sub(1)).zip(value.encode_utf16())
    {
        *destination = source;
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn hwnd(address: usize) -> HWND {
    HWND(std::ptr::with_exposed_provenance_mut(address))
}

#[cfg(test)]
mod tests {
    use gradial_core::{ActionErrorCode, ActionResult};
    use windows::Win32::UI::Shell::{NIF_INFO, NIIF_ERROR, NIIF_NOSOUND, NIIF_RESPECT_QUIET_TIME};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreatePopupMenu, DestroyMenu, GetMenuItemCount, GetMenuState, GetMenuStringW, GetSubMenu,
        MF_BYCOMMAND, MF_BYPOSITION, MF_GRAYED,
    };

    use super::{
        ACTIVITY_CAPACITY, ActivityRecord, ActivityStore, CLEAR_ACTIVITY_COMMAND_ID,
        FailureNotification, TrayHealth, append_activity_submenu, failure_notification_data,
        health_from_atomic, write_wide_buffer,
    };

    #[test]
    fn tooltip_buffer_is_terminated_and_truncated() {
        let mut buffer = [99_u16; 5];
        write_wide_buffer(&mut buffer, "abcdef");
        assert_eq!(buffer, ['a' as u16, 'b' as u16, 'c' as u16, 'd' as u16, 0]);
    }

    #[test]
    fn unknown_atomic_health_is_recovering() {
        super::CURRENT_HEALTH.store(255, std::sync::atomic::Ordering::Release);
        assert_eq!(health_from_atomic(), TrayHealth::Recovering);
    }

    #[test]
    fn activity_retains_only_the_latest_twenty_failures_and_cancellations() {
        let mut store = ActivityStore::new();
        for index in 0..(ACTIVITY_CAPACITY + 3) {
            let result = if index % 2 == 0 {
                ActionResult::Cancelled
            } else {
                ActionResult::Failure {
                    code: ActionErrorCode::ActionBusy,
                    safe_message: "must not be retained".to_owned(),
                }
            };
            store.record(&result, 7);
        }

        assert_eq!(store.records.len(), ACTIVITY_CAPACITY);
        assert_eq!(store.records.front(), Some(&ActivityRecord::Cancelled));
        assert_eq!(
            store.records.back(),
            Some(&ActivityRecord::Failure(ActionErrorCode::ActionBusy))
        );
    }

    #[test]
    fn success_is_silent_and_failure_messages_cannot_contain_action_payloads() {
        let mut store = ActivityStore::new();
        assert_eq!(store.record(&ActionResult::Success, 8), None);
        assert!(store.notifications.is_empty());

        let private_payload = r"C:\private\secret.exe --token=hunter2";
        store.record(
            &ActionResult::Failure {
                code: ActionErrorCode::PathNotFound,
                safe_message: private_payload.to_owned(),
            },
            8,
        );

        let record = store.records.front().expect("failure should be retained");
        assert!(!record.menu_text().contains(private_payload));
        assert!(!super::failure_message(ActionErrorCode::PathNotFound).contains(private_payload));
        assert_eq!(
            store.notifications.front(),
            Some(&FailureNotification { code: ActionErrorCode::PathNotFound, config_revision: 8 })
        );
    }

    #[test]
    fn cancellations_are_retained_without_queuing_notifications() {
        let mut store = ActivityStore::new();
        store.record(&ActionResult::Cancelled, 9);

        assert_eq!(store.records.front(), Some(&ActivityRecord::Cancelled));
        assert!(store.notifications.is_empty());
    }

    #[test]
    fn clearing_activity_also_discards_pending_notifications() {
        let mut store = ActivityStore::new();
        store.record(
            &ActionResult::Failure {
                code: ActionErrorCode::ActionTimeout,
                safe_message: String::new(),
            },
            10,
        );
        store.clear();

        assert!(store.records.is_empty());
        assert!(store.notifications.is_empty());
    }

    #[test]
    fn native_activity_submenu_contains_records_and_an_enabled_clear_command() {
        // SAFETY: the test owns both menus and transfers the child to the
        // parent before destroying the parent recursively.
        let menu = unsafe { CreatePopupMenu() }.expect("parent menu should be created");
        let activity_menu = unsafe { CreatePopupMenu() }.expect("activity menu should be created");
        let activity =
            [ActivityRecord::Failure(ActionErrorCode::PathNotFound), ActivityRecord::Cancelled];
        append_activity_submenu(menu, activity_menu, &activity)
            .expect("activity submenu should be appended");

        // SAFETY: both menu handles remain live until the final DestroyMenu.
        unsafe {
            assert_eq!(GetMenuItemCount(Some(menu)), 1);
            assert_eq!(GetSubMenu(menu, 0), activity_menu);
            assert_eq!(GetMenuItemCount(Some(activity_menu)), 4);
            let mut heading = [0_u16; 64];
            let heading_length = GetMenuStringW(menu, 0, Some(&mut heading), MF_BYPOSITION);
            assert_eq!(
                String::from_utf16(&heading[..usize::try_from(heading_length).unwrap_or_default()])
                    .expect("menu heading should be UTF-16"),
                "Recent activity (2)"
            );
            let clear_command_id =
                u32::try_from(CLEAR_ACTIVITY_COMMAND_ID).expect("command ID should fit u32");
            let clear_state = GetMenuState(activity_menu, clear_command_id, MF_BYCOMMAND);
            assert_eq!(clear_state & MF_GRAYED.0, 0);
            DestroyMenu(menu).expect("parent menu should be destroyed");
        }
    }

    #[test]
    fn failure_notification_uses_only_nonactivating_shell_notification_fields() {
        let data = failure_notification_data(
            42,
            FailureNotification { code: ActionErrorCode::PathNotFound, config_revision: 11 },
        )
        .expect("notification data should be valid");

        assert_eq!(data.uFlags, NIF_INFO);
        assert_eq!(data.dwInfoFlags, NIIF_ERROR | NIIF_NOSOUND | NIIF_RESPECT_QUIET_TIME);
        assert_eq!(from_wide_buffer(&data.szInfoTitle), "Gradial action failed");
        assert_eq!(
            from_wide_buffer(&data.szInfo),
            "The configured file or folder could not be found."
        );
    }

    fn from_wide_buffer(buffer: &[u16]) -> String {
        let length = buffer.iter().position(|value| *value == 0).unwrap_or(buffer.len());
        String::from_utf16(&buffer[..length]).expect("test strings should be valid UTF-16")
    }
}
