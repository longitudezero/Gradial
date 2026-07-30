use std::{io::Write, path::Path};

use anyhow::Result;
use flexi_logger::{
    Cleanup, Criterion, DeferredNow, Duplicate, FileSpec, Logger, LoggerHandle, Naming, WriteMode,
};
use log::Record;

mod timing;

pub(crate) use timing::TimingMarkers;

const MAX_LOG_BYTES: u64 = 1_000_000;
const MAX_LOG_FILES: usize = 5;
const MAX_ARCHIVED_LOG_FILES: usize = MAX_LOG_FILES - 1;

pub(crate) fn initialize(log_directory: &Path) -> Result<LoggerHandle> {
    let handle = Logger::try_with_str(
        "warn,startup=info,config=info,profiles=info,input=info,interaction=info,\
             actions=info,renderer=info,focus=info,diagnostics=info,tray=info,settings=info,\
             onboarding=info",
    )?
    .use_utc()
    .log_to_file(
        FileSpec::default()
            .directory(log_directory)
            .basename("gradial")
            .suffix("log")
            .suppress_timestamp(),
    )
    .format_for_files(format_record)
    .duplicate_to_stderr(Duplicate::Warn)
    .rotate(
        Criterion::Size(MAX_LOG_BYTES),
        Naming::Numbers,
        Cleanup::KeepLogFiles(MAX_ARCHIVED_LOG_FILES),
    )
    .write_mode(WriteMode::Async)
    .start()?;

    Ok(handle)
}

fn format_record(
    writer: &mut dyn Write,
    now: &mut DeferredNow,
    record: &Record<'_>,
) -> std::io::Result<()> {
    write!(
        writer,
        "{} level={} target={} {}",
        now.format_rfc3339(),
        record.level(),
        record.target(),
        record.args()
    )
}
