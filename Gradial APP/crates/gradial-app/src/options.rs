use anyhow::{Result, bail};
use gradial_core::GeometryMode;
#[cfg(debug_assertions)]
use gradial_core::{
    MAX_CONTINUOUS_FULL_SCALE_RADIUS_DIP, MIN_CONTINUOUS_FULL_SCALE_RADIUS_DIP, SliceCount,
};

#[cfg(debug_assertions)]
pub(crate) const HELP: &str = "\
Gradial interaction prototype

Usage:
  gradial [--geometry=auto|fixed-origin|edge-assist] [--slices=4|8] [--continuous-meter|--continuous-study|--continuous-distance-study] [--continuous-full-scale-dip=<160-400>] [--reduced-motion] [--open-settings] [--skip-onboarding]

Geometry modes:
  auto          Use edge assist only when the complete ring would clip.
  fixed-origin  Keep the ring and gesture origin at M4 button-down.
  edge-assist   Always inset the ring and use raw deltas as a virtual pointer.

Slice counts:
  4             Use the cardinal N/E/S/W menu.
  8             Use the eight-direction menu (default).

Continuous input:
  --continuous-meter  Make every direction an inert 0-100% distance meter.
  --continuous-study  Run the guided, anonymous distance usability study.
  --continuous-distance-study
                      Compare the 160/200/250/300 DIP full-scale candidates.
  --continuous-full-scale-dip
                      Override the inert meter's 100% distance (160-400 DIP).

Animation:
  --reduced-motion  Remove visual interpolation without changing gesture timing.

Settings:
  --open-settings   Open the settings window at startup for development checks.

Onboarding:
  --skip-onboarding Skip the first-run window for isolated development harnesses.
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
pub(crate) enum ContinuousMeterMode {
    #[default]
    Disabled,
    #[cfg(debug_assertions)]
    Enabled,
    #[cfg(debug_assertions)]
    Study,
    #[cfg(debug_assertions)]
    DistanceStudy,
}

impl ContinuousMeterMode {
    pub(crate) const fn is_enabled(self) -> bool {
        match self {
            Self::Disabled => false,
            #[cfg(debug_assertions)]
            Self::Enabled | Self::Study | Self::DistanceStudy => true,
        }
    }

    pub(crate) const fn is_study(self) -> bool {
        #[cfg(debug_assertions)]
        {
            matches!(self, Self::Study | Self::DistanceStudy)
        }
        #[cfg(not(debug_assertions))]
        {
            false
        }
    }

    pub(crate) const fn is_distance_study(self) -> bool {
        #[cfg(debug_assertions)]
        {
            matches!(self, Self::DistanceStudy)
        }
        #[cfg(not(debug_assertions))]
        {
            false
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct StartupOptions {
    pub(crate) geometry_mode: GeometryMode,
    pub(crate) debug_slice_override: Option<gradial_core::SliceCount>,
    pub(crate) debug_open_settings: bool,
    pub(crate) debug_skip_onboarding: bool,
    pub(crate) debug_continuous_meter: ContinuousMeterMode,
    pub(crate) debug_continuous_full_scale_radius_dip: Option<u32>,
    pub(crate) reduced_motion: bool,
}

impl StartupOptions {
    pub(crate) const fn bypasses_onboarding(self) -> bool {
        self.debug_skip_onboarding || self.debug_continuous_meter.is_study()
    }

    #[allow(clippy::too_many_lines)]
    pub(crate) fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut geometry_mode = GeometryMode::Auto;
        #[cfg(debug_assertions)]
        let mut debug_slice_override = None;
        #[cfg(not(debug_assertions))]
        let debug_slice_override = None;
        let mut reduced_motion = false;
        #[cfg(debug_assertions)]
        let mut debug_open_settings = false;
        #[cfg(not(debug_assertions))]
        let debug_open_settings = false;
        #[cfg(debug_assertions)]
        let mut debug_skip_onboarding = false;
        #[cfg(not(debug_assertions))]
        let debug_skip_onboarding = false;
        #[cfg(debug_assertions)]
        let mut debug_continuous_meter = ContinuousMeterMode::Disabled;
        #[cfg(not(debug_assertions))]
        let debug_continuous_meter = ContinuousMeterMode::Disabled;
        #[cfg(debug_assertions)]
        let mut debug_continuous_full_scale_radius_dip = None;
        #[cfg(not(debug_assertions))]
        let debug_continuous_full_scale_radius_dip = None;
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
                    if argument == "--open-settings" {
                        debug_open_settings = true;
                        continue;
                    }
                    if argument == "--skip-onboarding" {
                        debug_skip_onboarding = true;
                        continue;
                    }
                    if argument == "--continuous-meter" {
                        if debug_continuous_meter.is_enabled() {
                            bail!("choose only one continuous development mode");
                        }
                        debug_continuous_meter = ContinuousMeterMode::Enabled;
                        continue;
                    }
                    if argument == "--continuous-study" {
                        if debug_continuous_meter.is_enabled() {
                            bail!("choose only one continuous development mode");
                        }
                        debug_continuous_meter = ContinuousMeterMode::Study;
                        continue;
                    }
                    if argument == "--continuous-distance-study" {
                        if debug_continuous_meter.is_enabled() {
                            bail!("choose only one continuous development mode");
                        }
                        debug_continuous_meter = ContinuousMeterMode::DistanceStudy;
                        continue;
                    }
                    if let Some(value) = argument.strip_prefix("--continuous-full-scale-dip=") {
                        debug_continuous_full_scale_radius_dip =
                            Some(parse_continuous_full_scale_radius(value)?);
                        continue;
                    }
                    if argument == "--continuous-full-scale-dip" {
                        let value = arguments.next().ok_or_else(|| {
                            anyhow::anyhow!(
                                "--continuous-full-scale-dip requires an integer from 160 to 400"
                            )
                        })?;
                        debug_continuous_full_scale_radius_dip =
                            Some(parse_continuous_full_scale_radius(&value)?);
                        continue;
                    }
                }
                bail!("unknown Gradial argument: {argument}");
            }
        }

        #[cfg(debug_assertions)]
        if debug_continuous_meter.is_study()
            && (debug_slice_override.is_some()
                || debug_open_settings
                || debug_continuous_full_scale_radius_dip.is_some()
                || geometry_mode != GeometryMode::Auto)
        {
            bail!(
                "guided continuous studies require automatic geometry and cannot combine with --slices or --open-settings"
            );
        }

        #[cfg(debug_assertions)]
        if debug_continuous_full_scale_radius_dip.is_some()
            && debug_continuous_meter != ContinuousMeterMode::Enabled
        {
            bail!("--continuous-full-scale-dip requires --continuous-meter");
        }

        Ok(Self {
            geometry_mode,
            debug_slice_override,
            debug_open_settings,
            debug_skip_onboarding,
            debug_continuous_meter,
            debug_continuous_full_scale_radius_dip,
            reduced_motion,
        })
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

#[cfg(debug_assertions)]
fn parse_continuous_full_scale_radius(value: &str) -> Result<u32> {
    let radius = value.parse::<u32>().map_err(|_| {
        anyhow::anyhow!("invalid continuous full-scale distance {value:?}; expected 160 to 400 DIP")
    })?;
    if !(MIN_CONTINUOUS_FULL_SCALE_RADIUS_DIP..=MAX_CONTINUOUS_FULL_SCALE_RADIUS_DIP)
        .contains(&radius)
    {
        bail!("invalid continuous full-scale distance {value:?}; expected 160 to 400 DIP");
    }
    Ok(radius)
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

    use super::{ContinuousMeterMode, StartupOptions};

    #[test]
    fn geometry_defaults_to_automatic() {
        let options = StartupOptions::parse(Vec::new()).expect("default options");
        assert_eq!(options.geometry_mode, GeometryMode::Auto);
        assert_eq!(options.debug_slice_override, None);
        assert!(!options.debug_open_settings);
        assert!(!options.debug_skip_onboarding);
        assert_eq!(options.debug_continuous_meter, ContinuousMeterMode::Disabled);
        assert_eq!(options.debug_continuous_full_scale_radius_dip, None);
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
            "--open-settings".to_owned(),
            "--skip-onboarding".to_owned(),
            "--continuous-meter".to_owned(),
            "--continuous-full-scale-dip=250".to_owned(),
        ])
        .expect("reduced-motion options");
        assert_eq!(options.geometry_mode, GeometryMode::EdgeAssist);
        assert_eq!(options.debug_slice_override, Some(SliceCount::Four));
        assert!(options.debug_open_settings);
        assert!(options.debug_skip_onboarding);
        assert_eq!(options.debug_continuous_meter, ContinuousMeterMode::Enabled);
        assert_eq!(options.debug_continuous_full_scale_radius_dip, Some(250));
        assert!(options.reduced_motion);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn continuous_full_scale_override_is_bounded_and_requires_the_inert_meter() {
        for (arguments, expected) in [
            (vec!["--continuous-meter", "--continuous-full-scale-dip=160"], 160),
            (vec!["--continuous-full-scale-dip", "400", "--continuous-meter"], 400),
        ] {
            let options = StartupOptions::parse(arguments.into_iter().map(str::to_owned))
                .expect("valid continuous distance override");
            assert_eq!(options.debug_continuous_full_scale_radius_dip, Some(expected));
        }

        for arguments in [
            vec!["--continuous-full-scale-dip=159", "--continuous-meter"],
            vec!["--continuous-meter", "--continuous-full-scale-dip=401"],
            vec!["--continuous-full-scale-dip=300"],
            vec!["--continuous-study", "--continuous-full-scale-dip=300"],
        ] {
            let described_arguments = format!("{arguments:?}");
            assert!(
                StartupOptions::parse(arguments.into_iter().map(str::to_owned)).is_err(),
                "override must reject {described_arguments}"
            );
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    fn continuous_study_is_isolated_from_geometry_and_editor_overrides() {
        let study =
            StartupOptions::parse(["--continuous-study".to_owned()]).expect("continuous study");
        assert_eq!(study.debug_continuous_meter, ContinuousMeterMode::Study);
        assert!(study.debug_continuous_meter.is_study());

        let distance = StartupOptions::parse(["--continuous-distance-study".to_owned()])
            .expect("continuous distance study");
        assert_eq!(distance.debug_continuous_meter, ContinuousMeterMode::DistanceStudy);
        assert!(distance.debug_continuous_meter.is_study());
        assert!(distance.debug_continuous_meter.is_distance_study());

        for arguments in [
            vec!["--continuous-study", "--slices=4"],
            vec!["--continuous-study", "--open-settings"],
            vec!["--continuous-study", "--geometry=fixed-origin"],
            vec!["--continuous-study", "--continuous-meter"],
            vec!["--continuous-distance-study", "--slices=4"],
            vec!["--continuous-distance-study", "--open-settings"],
            vec!["--continuous-distance-study", "--geometry=edge-assist"],
            vec!["--continuous-distance-study", "--continuous-study"],
        ] {
            let error = StartupOptions::parse(arguments.into_iter().map(str::to_owned))
                .expect_err("incompatible study options");
            assert!(error.to_string().contains("continuous"), "unexpected error: {error}");
        }
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn release_build_rejects_development_only_options() {
        for argument in [
            "--slices=4",
            "--open-settings",
            "--skip-onboarding",
            "--continuous-meter",
            "--continuous-study",
            "--continuous-distance-study",
            "--continuous-full-scale-dip=300",
        ] {
            let error =
                StartupOptions::parse([argument.to_owned()]).expect_err("debug-only option");
            assert!(error.to_string().contains("unknown Gradial argument"));
        }
    }
}
