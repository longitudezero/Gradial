use anyhow::{Result, bail};
use gradial_core::{GeometryMode, SliceCount};

pub(crate) const HELP: &str = "\
Gradial interaction prototype

Usage:
  gradial [--geometry=auto|fixed-origin|edge-assist] [--slices=4|8] [--reduced-motion]

Geometry modes:
  auto          Use edge assist only when the complete ring would clip.
  fixed-origin  Keep the ring and gesture origin at M4 button-down.
  edge-assist   Always inset the ring and use raw deltas as a virtual pointer.

Slice counts:
  4             Use the cardinal N/E/S/W menu.
  8             Use the eight-direction menu (default).

Animation:
  --reduced-motion  Remove visual interpolation without changing gesture timing.
";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct StartupOptions {
    pub(crate) geometry_mode: GeometryMode,
    pub(crate) slice_count: SliceCount,
    pub(crate) reduced_motion: bool,
}

impl StartupOptions {
    pub(crate) fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut geometry_mode = GeometryMode::Auto;
        let mut slice_count = SliceCount::Eight;
        let mut reduced_motion = false;
        let mut arguments = arguments.into_iter();

        while let Some(argument) = arguments.next() {
            if let Some(value) = argument.strip_prefix("--geometry=") {
                geometry_mode = parse_geometry_mode(value)?;
            } else if argument == "--geometry" {
                let value = arguments.next().ok_or_else(|| {
                    anyhow::anyhow!("--geometry requires auto, fixed-origin, or edge-assist")
                })?;
                geometry_mode = parse_geometry_mode(&value)?;
            } else if let Some(value) = argument.strip_prefix("--slices=") {
                slice_count = parse_slice_count(value)?;
            } else if argument == "--slices" {
                let value =
                    arguments.next().ok_or_else(|| anyhow::anyhow!("--slices requires 4 or 8"))?;
                slice_count = parse_slice_count(&value)?;
            } else if argument == "--reduced-motion" {
                reduced_motion = true;
            } else {
                bail!("unknown Gradial argument: {argument}");
            }
        }

        Ok(Self { geometry_mode, slice_count, reduced_motion })
    }
}

fn parse_slice_count(value: &str) -> Result<SliceCount> {
    match value {
        "4" => Ok(SliceCount::Four),
        "8" => Ok(SliceCount::Eight),
        _ => bail!("invalid slice count {value:?}; expected 4 or 8"),
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
    use gradial_core::{GeometryMode, SliceCount};

    use super::StartupOptions;

    #[test]
    fn geometry_defaults_to_automatic() {
        let options = StartupOptions::parse(Vec::new()).expect("default options");
        assert_eq!(options.geometry_mode, GeometryMode::Auto);
        assert_eq!(options.slice_count, SliceCount::Eight);
        assert!(!options.reduced_motion);
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

    #[test]
    fn slice_count_accepts_both_argument_forms_and_combines_with_geometry() {
        let four = StartupOptions::parse([
            "--slices=4".to_owned(),
            "--geometry".to_owned(),
            "edge-assist".to_owned(),
        ])
        .expect("four-slice options");
        assert_eq!(four.slice_count, SliceCount::Four);
        assert_eq!(four.geometry_mode, GeometryMode::EdgeAssist);

        let eight =
            StartupOptions::parse(["--slices".to_owned(), "8".to_owned()]).expect("separate form");
        assert_eq!(eight.slice_count, SliceCount::Eight);
    }

    #[test]
    fn invalid_slice_count_is_rejected() {
        let error =
            StartupOptions::parse(["--slices=6".to_owned()]).expect_err("unsupported count");
        assert!(error.to_string().contains("invalid slice count"));
    }

    #[test]
    fn reduced_motion_combines_with_other_development_options() {
        let options = StartupOptions::parse([
            "--geometry=edge-assist".to_owned(),
            "--slices=4".to_owned(),
            "--reduced-motion".to_owned(),
        ])
        .expect("reduced-motion options");
        assert_eq!(options.geometry_mode, GeometryMode::EdgeAssist);
        assert_eq!(options.slice_count, SliceCount::Four);
        assert!(options.reduced_motion);
    }
}
