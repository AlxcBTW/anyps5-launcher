//! What each toolkit release can do, in one table. The launcher asks [`supports`] before it
//! writes a line, sets a variable or shows a control that an older toolkit would not understand.
//!
//! An older toolkit is never refused: the feature is left out and the rest works. A toolkit newer
//! than [`TESTED_UP_TO`] is assumed to keep everything the tested one had, and is only noted as
//! "newer than tested". A local build has no release tag; it is assumed to be recent.
//!
//! The table is kept by hand per release (the release notes do not carry it). Sources: the first
//! release containing each feature, found with `git log -S` and `git tag --contains` on upstream.

use std::cmp::Ordering;

/// Something the launcher uses that not every toolkit release has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    /// `anyps5-input.ini` and `ANYPS5_INPUT_CONFIG`.
    InputFile,
    /// Stick deadzone and mouse tuning settings in the input file.
    InputTuning,
    /// `ANYPS5_GPU`.
    GpuChoice,
    /// `ANYPS5_PRESENT_MODE`.
    PresentMode,
    /// `ANYPS5_LANGUAGE`.
    GameLanguage,
    /// The `StickModifier` action and its scale.
    StickModifier,
    /// `KeyboardRampMs` and `TriggerRampMs`.
    InputRamps,
    /// `ANYPS5_OVERLAY` and `ANYPS5_OVERLAY_POS`.
    Overlay,
    /// `ANYPS5_SHADER_CACHE_DIR`.
    ShaderCacheDir,
    /// `relinker-patches.toml` and `--patches`.
    Patches,
    /// F12 screenshots and `ANYPS5_SCREENSHOTS`.
    Screenshots,
}

/// The first toolkit that has a feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Since {
    /// This release and every later one.
    Release(&'static str),
    /// Every release after this one: the feature is on upstream's main branch only.
    After(&'static str),
}

/// Feature to first toolkit. Seeded from the research of 2026-10-22.
pub const FEATURE_SINCE: &[(Feature, Since)] = &[
    (Feature::InputFile, Since::Release("0.1.0")),
    (Feature::InputTuning, Since::Release("0.1.2")),
    (Feature::GpuChoice, Since::Release("0.1.2")),
    (Feature::PresentMode, Since::Release("0.1.2")),
    (Feature::GameLanguage, Since::Release("0.1.2")),
    (Feature::StickModifier, Since::Release("0.1.3")),
    (Feature::InputRamps, Since::Release("0.1.3")),
    (Feature::Overlay, Since::Release("0.1.3")),
    (Feature::ShaderCacheDir, Since::Release("0.1.3")),
    (Feature::Patches, Since::Release("0.1.3")),
    (Feature::Screenshots, Since::After("0.1.3")),
];

/// The newest release this table was checked against.
pub const TESTED_UP_TO: &str = "0.1.3";

/// How a toolkit relates to what the launcher was tested with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// The tested release.
    Tested,
    /// Older: some features are left out.
    Older,
    /// Newer than tested: assumed to work, noted quietly.
    Newer,
    /// A local build, or a version that is not a release number.
    Local,
}

impl Feature {
    /// The first toolkit that has the feature, for a hint such as "Needs toolkit v0.1.3".
    pub fn since(self) -> Since {
        FEATURE_SINCE
            .iter()
            .find(|(f, _)| *f == self)
            .map(|(_, s)| *s)
            .expect("every feature has a row")
    }
}

/// `v0.1.3`, `0.1.3` and `0.1.3-abc123` (the stored version) all give `[0, 1, 3]`; anything else
/// (`local-abc123`, an empty string) gives `None`.
fn release_numbers(version: &str) -> Option<Vec<u64>> {
    let tag = version.trim().trim_start_matches('v');
    let tag = tag.split_once('-').map_or(tag, |(tag, _)| tag);
    tag.split('.').map(|part| part.parse().ok()).collect()
}

fn compare(a: &[u64], b: &[u64]) -> Ordering {
    let len = a.len().max(b.len());
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    (0..len)
        .map(|i| at(a, i).cmp(&at(b, i)))
        .find(|o| o.is_ne())
        .unwrap_or(Ordering::Equal)
}

/// Whether the toolkit `version` has `feature`. A local build is assumed to have everything.
pub fn supports(feature: Feature, version: &str) -> bool {
    let Some(have) = release_numbers(version) else {
        return true;
    };
    let order = |tag: &str| compare(&have, &release_numbers(tag).unwrap_or_default());
    match feature.since() {
        Since::Release(tag) => order(tag).is_ge(),
        Since::After(tag) => order(tag).is_gt(),
    }
}

/// Compares two versions or tags by their release numbers; `None` if either is not a release.
pub fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    Some(compare(&release_numbers(a)?, &release_numbers(b)?))
}

/// How `version` relates to [`TESTED_UP_TO`].
pub fn fit(version: &str) -> Fit {
    let Some(have) = release_numbers(version) else {
        return Fit::Local;
    };
    match compare(&have, &release_numbers(TESTED_UP_TO).unwrap_or_default()) {
        Ordering::Less => Fit::Older,
        Ordering::Equal => Fit::Tested,
        Ordering::Greater => Fit::Newer,
    }
}

/// The release tag inside a stored version (`0.1.3-abc123` gives `0.1.3`); `None` for a local
/// build.
pub fn release_tag(version: &str) -> Option<String> {
    let numbers = release_numbers(version)?;
    Some(
        numbers
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join("."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_feature_has_exactly_one_row() {
        for (feature, _) in FEATURE_SINCE {
            assert_eq!(
                FEATURE_SINCE.iter().filter(|(f, _)| f == feature).count(),
                1
            );
        }
    }

    #[test]
    fn features_follow_the_release_that_added_them() {
        assert!(supports(Feature::InputFile, "v0.1.0"));
        assert!(!supports(Feature::GpuChoice, "0.1.1"));
        assert!(supports(Feature::GpuChoice, "0.1.2-abc"));
        assert!(!supports(Feature::StickModifier, "0.1.2"));
        assert!(supports(Feature::StickModifier, "v0.1.3"));
        assert!(!supports(Feature::Patches, "0.1.2-abcdef"));
        assert!(supports(Feature::Patches, "0.1.3-abcdef"));
        assert!(
            supports(Feature::Patches, "0.1.10"),
            "0.1.10 is after 0.1.3"
        );
    }

    #[test]
    fn screenshots_need_a_build_after_the_last_release() {
        assert!(!supports(Feature::Screenshots, "0.1.3"));
        assert!(!supports(Feature::Screenshots, "v0.1.3-abc"));
        assert!(supports(Feature::Screenshots, "0.1.4"));
        assert!(supports(Feature::Screenshots, "0.2"));
    }

    #[test]
    fn a_local_build_is_assumed_to_have_everything() {
        for version in ["local-0123456789ab", "", "unknown"] {
            assert!(supports(Feature::Screenshots, version), "{version}");
            assert_eq!(fit(version), Fit::Local);
            assert_eq!(release_tag(version), None);
        }
    }

    #[test]
    fn fit_compares_with_the_tested_release() {
        assert_eq!(fit("0.1.2"), Fit::Older);
        assert_eq!(fit("v0.1.3"), Fit::Tested);
        assert_eq!(fit("0.1.3-abc"), Fit::Tested);
        assert_eq!(fit("0.1.4"), Fit::Newer);
        assert_eq!(fit("1.0"), Fit::Newer);
    }

    #[test]
    fn the_tag_is_taken_out_of_the_stored_version() {
        assert_eq!(release_tag("0.1.3-abc").as_deref(), Some("0.1.3"));
        assert_eq!(release_tag("v0.1.2").as_deref(), Some("0.1.2"));
    }

    #[test]
    fn hints_name_the_toolkit_that_has_the_feature() {
        assert_eq!(Feature::Patches.since(), Since::Release("0.1.3"));
        assert_eq!(Feature::Screenshots.since(), Since::After("0.1.3"));
    }
}
