use anyhow::{Result, bail};
use uuid::Uuid;
use windows::{
    Win32::System::{
        Diagnostics::Etw::{EventRegister, EventUnregister, EventWriteString, REGHANDLE},
        Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
    },
    core::{GUID, PCWSTR},
};

/// Stable provider identity used by the development capture scripts.
pub(crate) const PROVIDER_GUID_TEXT: &str = "3c55b348-69f8-4d33-8e81-62ba761d4f52";
const PROVIDER_GUID: GUID = GUID::from_u128(0x3c55b348_69f8_4d33_8e81_62ba761d4f52);
const INFORMATION_LEVEL: u8 = 4;
const TIMING_KEYWORD: u64 = 1;

/// A lightweight ETW provider whose timestamped strings can be correlated
/// with `PresentMon` display events by process and invocation ID.
#[derive(Debug)]
pub(crate) struct TimingMarkers {
    registration: REGHANDLE,
    qpc_frequency: i64,
}

impl TimingMarkers {
    pub(crate) fn register() -> Result<Self> {
        let mut registration = REGHANDLE::default();
        // SAFETY: the provider GUID is static, no callback is installed, and
        // the output points to a live REGHANDLE.
        let status = unsafe { EventRegister(&PROVIDER_GUID, None, None, &raw mut registration) };
        if status != 0 {
            bail!("could not register Gradial's ETW timing provider: Windows error {status}");
        }
        log::info!(
            target: "diagnostics",
            "component=diagnostics event_code=timing_provider_registered result_code=ok \
             config_revision=0 provider_guid={PROVIDER_GUID_TEXT}"
        );

        Ok(Self { registration, qpc_frequency: query_qpc_frequency() })
    }

    pub(crate) fn emit(
        &self,
        marker: &str,
        invocation_id: Option<Uuid>,
        source_qpc: i64,
        fields: &str,
    ) {
        let invocation = invocation_id.map_or_else(|| "none".to_owned(), |id| id.to_string());
        let marker_qpc = query_qpc();
        let message = format!(
            "marker={marker} invocation_id={invocation} source_qpc={source_qpc} \
             marker_qpc={marker_qpc} qpc_frequency={} {fields}",
            self.qpc_frequency
        );
        let wide: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();

        // SAFETY: `wide` is null-terminated and remains live for the call.
        let status = unsafe {
            EventWriteString(
                self.registration,
                INFORMATION_LEVEL,
                TIMING_KEYWORD,
                PCWSTR::from_raw(wide.as_ptr()),
            )
        };
        if status != 0 {
            log::warn!(
                target: "diagnostics",
                "component=diagnostics event_code=timing_marker_write_failed \
                 result_code=failure config_revision=0 marker={marker} windows_error={status}"
            );
        }
    }
}

fn query_qpc() -> i64 {
    let mut timestamp = 0_i64;
    // SAFETY: the output pointer refers to a live local i64.
    if unsafe { QueryPerformanceCounter(&raw mut timestamp) }.is_ok() { timestamp } else { 0 }
}

fn query_qpc_frequency() -> i64 {
    let mut frequency = 0_i64;
    // SAFETY: the output pointer refers to a live local i64.
    if unsafe { QueryPerformanceFrequency(&raw mut frequency) }.is_ok() { frequency } else { 0 }
}

impl Drop for TimingMarkers {
    fn drop(&mut self) {
        // SAFETY: the handle was registered exactly once by this object and is
        // unregistered exactly once here after all Arc clones have gone away.
        let status = unsafe { EventUnregister(self.registration) };
        if status != 0 {
            log::warn!(
                target: "diagnostics",
                "component=diagnostics event_code=timing_provider_unregister_failed \
                 result_code=failure config_revision=0 windows_error={status}"
            );
        }
    }
}
