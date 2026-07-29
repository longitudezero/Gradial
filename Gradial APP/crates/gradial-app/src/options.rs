use anyhow::{Result, bail};
use gradial_core::GeometryMode;
#[cfg(debug_assertions)]
use gradial_core::SliceCount;

#[cfg(debug_assertions)]
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

#[cfg(not(debug_assertions))]
pub(crate) const HELP: &str = "\
Gradial

Usage:
  gradial [--geometry=auto|fixed-origin|edge-assist] [--reduced-motion]

Geometry modes:
  auto          Use edge assist only when the complete ring would clip.
  fixed-origin  Keep the ring and gesture origin at the activation button-down.
  edge-assist   Always inset the ring and use raw deltas as a virtual pointer.

Animation:
  --reduced-motion  Remove visual interpolation without changing gesture timing.

Menu geometry and labels are selected from the resolved profile configuration.
";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct StartupOptions {
    pub(crate) geometry_mode: GeometryMode,
    pub(crate) debug_slice_override: Option<gradial_core::SliceCount>,
    pub(crate) reduced_motion: bool,
}

impl StartupOptions {
    pub(crate) fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut geometry_mode = GeometryMode::Auto;
        #[cfg(debug_assertions)]
        let mut debug_slice_override = None;
        #[cfg(not(debug_assertions))]
        let debug_slice_override = None;
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
            } else if argument == "--reduced-motion" {
                reduced_motion = true;
            } else {
                #[cfg(debug_assertions)]
                {
                    if let Some(value) = argument.strip_prefix("--slices=") {
                        debug_slice_override = Some(parse_slice_count(value)?);
                        continue;
                    }
                    if argument == "--slices" {
                        let value = arguments
                            .next()
                            .ok_or_else(|| anyhow::anyhow!("--slices requires 4 or 8"))?;
                        debug_slice_override = Some(parse_slice_count(&value)?);
                        continue;
                    }
                }
                bail!("unknown Gradial argument: {argument}");
            }
        }

        Ok(Self { geometry_mode, debug_slice_override, reduced_motion })
    }
}

#[cfg(debug_assertions)]
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
    use gradial_core::GeometryMode;
    #[cfg(debug_assertions)]
    use gradial_core::SliceCount;

    use super::StartupOptions;

    #[test]
    fn geometry_defaults_to_automatic() {
        let options = StartupOptions::parse(Vec::new()).expect("default options");
        assert_eq!(options.geometry_mode, GeometryMode::Auto);
        assert_eq!(options.debug_slice_override, None);
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
    #[cfg(debug_assertions)]
    fn slice_count_accepts_both_argument_forms_and_combines_with_geometry() {
        let four = StartupOptions::parse([
            "--slices=4".to_owned(),
            "--geometry".to_owned(),
            "edge-assist".to_owned(),
        ])
        .expect("four-slice options");
        assert_eq!(four.debug_slice_override, Some(SliceCount::Four));
        assert_eq!(four.geometry_mode, GeometryMode::EdgeAssist);

        let eight =
            StartupOptions::parse(["--slices".to_owned(), "8".to_owned()]).expect("separate form");
        assert_eq!(eight.debug_slice_override, Some(SliceCount::Eight));
    }

    #[test]
    #[cfg(debug_assertions)]
    fn invalid_slice_count_is_rejected() {
        let error =
            StartupOptions::parse(["--slices=6".to_owned()]).expect_err("unsupported count");
        assert!(error.to_string().contains("invalid slice count"));
    }

    #[test]
    #[cfg(debug_assertions)]
    fn reduced_motion_combines_with_other_development_options() {
        let options = StartupOptions::parse([
            "--geometry=edge-assist".to_owned(),
            "--slices=4".to_owned(),
            "--reduced-motion".to_owned(),
        ])
        .expect("reduced-motion options");
        assert_eq!(options.geometry_mode, GeometryMode::EdgeAssist);
        assert_eq!(options.debug_slice_override, Some(SliceCount::Four));
        assert!(options.reduced_motion);
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn release_build_rejects_the_debug_slice_override() {
        let error =
            StartupOptions::parse(["--slices=4".to_owned()]).expect_err("debug-only override");
        assert!(error.to_string().contains("unknown Gradial argument"));
    }
}
