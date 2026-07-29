use std::{
    mem::size_of,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

use anyhow::{Context, Result, bail};
use windows::{
    Win32::{
        Foundation::{HWND, POINT},
        UI::{
            Shell::{
                NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
                NIM_SETVERSION, NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
                Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, IDI_ERROR,
                IDI_INFORMATION, IDI_WARNING, LoadIconW, MF_DISABLED, MF_ENABLED, MF_GRAYED,
                MF_SEPARATOR, MF_STRING, SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON,
                TrackPopupMenu, WM_CONTEXTMENU,
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
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
static PENDING_COMMAND: AtomicU8 = AtomicU8::new(0);
static CURRENT_HEALTH: AtomicU8 = AtomicU8::new(TrayHealth::Recovering as u8);
static ACTION_RUNNING: AtomicBool = AtomicBool::new(false);

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
        _ => None,
    }
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
    // SAFETY: menu ownership remains in this function and is released before
    // returning.
    let menu = unsafe { CreatePopupMenu() }.context("could not create the tray menu")?;
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
            _ => {}
        }
        Ok(())
    })();

    // SAFETY: this function owns the menu and destroys it exactly once.
    if let Err(error) = unsafe { DestroyMenu(menu) } {
        log::warn!(
            target: "tray",
            "component=tray event_code=tray_menu_cleanup_failed result_code=failure \
             config_revision=0 error={error}"
        );
    }
    result
}

fn health_from_atomic() -> TrayHealth {
    match CURRENT_HEALTH.load(Ordering::Acquire) {
        1 => TrayHealth::Healthy,
        3 => TrayHealth::Faulted,
        _ => TrayHealth::Recovering,
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
    use super::{TrayHealth, health_from_atomic, write_wide_buffer};

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
}
