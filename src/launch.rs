//! Command-line launch options, parsed once in `main` and shared as a resource.
//!
//! `--new` ignores the saved progress; `--vram <GB>` states the player's GPU
//! memory, which wgpu cannot report. Unknown arguments are ignored.

use bevy::prelude::*;

const DEFAULT_VRAM_GB: f32 = 6.0;

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct LaunchOptions {
    /// Start afresh instead of restoring the saved progress.
    pub new: bool,
    /// GPU memory in GB, from which the UHD budget is derived.
    pub vram_gb: f32,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            new: false,
            vram_gb: DEFAULT_VRAM_GB,
        }
    }
}

impl LaunchOptions {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut options = Self::default();
        let mut args = args.into_iter().peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--new" => options.new = true,
                "--vram" => {
                    // A following flag is not the value: leave it for the next turn.
                    let value = args.next_if(|next| !next.starts_with("--"));
                    options.vram_gb = vram_gb(value.as_deref());
                }
                _ => {}
            }
        }
        options
    }

    pub fn from_env() -> Self {
        Self::parse(std::env::args().skip(1))
    }
}

/// A positive, finite number of GB; anything else warns and keeps the default.
fn vram_gb(value: Option<&str>) -> f32 {
    let parsed = value
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|gb| gb.is_finite() && *gb > 0.0);
    parsed.unwrap_or_else(|| {
        warn!("--vram needs a positive number of GB; assuming {DEFAULT_VRAM_GB}");
        DEFAULT_VRAM_GB
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> LaunchOptions {
        LaunchOptions::parse(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn defaults_are_a_saved_run_on_a_6_gb_card() {
        assert_eq!(
            parse(&[]),
            LaunchOptions {
                new: false,
                vram_gb: 6.0
            }
        );
    }

    #[test]
    fn flags_parse_in_any_order() {
        assert_eq!(
            parse(&["--vram", "12", "--new"]),
            LaunchOptions {
                new: true,
                vram_gb: 12.0
            }
        );
        assert_eq!(
            parse(&["--new", "--vram", "7.5"]),
            LaunchOptions {
                new: true,
                vram_gb: 7.5
            }
        );
    }

    #[test]
    fn a_bad_vram_value_keeps_the_default() {
        for args in [
            &["--vram", "abc"][..],
            &["--vram"],
            &["--vram", "-3"],
            &["--vram", "0"],
            &["--vram", "NaN"],
        ] {
            assert_eq!(parse(args).vram_gb, 6.0, "{args:?}");
        }
    }

    #[test]
    fn a_flag_after_vram_is_not_its_value() {
        assert_eq!(
            parse(&["--vram", "--new"]),
            LaunchOptions {
                new: true,
                vram_gb: 6.0
            }
        );
    }

    #[test]
    fn unknown_arguments_are_ignored() {
        assert_eq!(parse(&["--bogus", "12", "-x"]), LaunchOptions::default());
    }
}
