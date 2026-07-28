use anyhow::{Result, bail};
use gradial_core::GeometryMode;

pub(crate) const HELP: &str = "\
Gradial interaction prototype

Usage:
  gradial [--geometry=auto|fixed-origin|edge-assist]

Geometry modes:
  auto          Use edge assist only when the complete ring would clip.
  fixed-origin  Keep the ring and gesture origin at M4 button-down.
  edge-assist   Always inset the ring and use raw deltas as a virtual pointer.
";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct StartupOptions {
    pub(crate) geometry_mode: GeometryMode,
}

impl StartupOptions {
    pub(crate) fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut geometry_mode = GeometryMode::Auto;
        let mut arguments = arguments.into_iter();

        while let Some(argument) = arguments.next() {
            let value = if let Some(value) = argument.strip_prefix("--geometry=") {
                value.to_owned()
            } else if argument == "--geometry" {
                arguments.next().ok_or_else(|| {
                    anyhow::anyhow!("--geometry requires auto, fixed-origin, or edge-assist")
                })?
            } else {
                bail!("unknown Gradial argument: {argument}");
            };

            geometry_mode = parse_geometry_mode(&value)?;
        }

        Ok(Self { geometry_mode })
    }
}

fn parse_geometry_mode(value: &str) -> Result<GeometryMode> {
    match value {
        "auto" => Ok(GeometryMode::Auto),
        "fixed-origin" => Ok(GeometryMode::FixedOrigin),
        "edge-assist" => Ok(GeometryMode::EdgeAssist),
        _ => bail!("invalid geometry mode {value:?}; expected auto, fixed-origin, or edge-assist"),
    }
}

#[cfg(test)]
mod tests {
    use gradial_core::GeometryMode;

    use super::StartupOptions;

    #[test]
    fn geometry_defaults_to_automatic() {
        assert_eq!(
            StartupOptions::parse(Vec::new()).expect("default options").geometry_mode,
            GeometryMode::Auto
        );
    }

    #[test]
    fn geometry_accepts_both_argument_forms() {
        assert_eq!(
            StartupOptions::parse(["--geometry=edge-assist".to_owned()])
                .expect("equals form")
                .geometry_mode,
            GeometryMode::EdgeAssist
        );
        assert_eq!(
            StartupOptions::parse(["--geometry".to_owned(), "fixed-origin".to_owned(),])
                .expect("separate form")
                .geometry_mode,
            GeometryMode::FixedOrigin
        );
    }

    #[test]
    fn invalid_geometry_is_rejected() {
        let error =
            StartupOptions::parse(["--geometry=warp".to_owned()]).expect_err("unsupported mode");
        assert!(error.to_string().contains("invalid geometry mode"));
    }
}
