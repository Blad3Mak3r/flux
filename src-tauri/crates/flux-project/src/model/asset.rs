//! The media files a project uses, in the archive or linked by path.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioAsset {
    pub id: String,
    pub name: String,
    /// Archive path, `audio/<file name>`: where the bytes live when `link` is absent, and in any
    /// case the stable key of the asset (its preview, its extension).
    pub file: String,
    /// Where the audio lives outside the archive; absent when the archive holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<MediaLink>,
    /// The tempo found in the audio, or set by the user; absent when it was never detected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
}

/// A video (or image) file, in the archive or linked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoAsset {
    pub id: String,
    pub name: String,
    /// Archive path, `video/<file name>`; like [`AudioAsset::file`], the key of the asset.
    pub file: String,
    /// Where the file lives outside the archive; absent when the archive holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<MediaLink>,
}

/// A media file kept outside the archive, found by path (a project with videos of several GB must
/// not copy them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaLink {
    /// Absolute, as the system gave it.
    pub path: String,
    /// Relative to the folder of the project file, `/`-separated: tried when `path` is gone, so a
    /// project moved together with its media still finds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative: Option<String>,
}

#[cfg(test)]
mod tests {

    use crate::error::ProjectError;

    use crate::model::tests::*;

    use crate::model::*;

    #[test]
    fn metronome_and_asset_bpm_should_stay_out_of_the_file_when_unset() {
        let mut project = sample_project();
        project.output.metronome = None;
        project.assets[0].bpm = None;
        let written = serde_json::to_value(&project).unwrap();
        assert!(written["output"].get("metronome").is_none());
        assert!(written["assets"][0].get("bpm").is_none());
        let parsed: Project = serde_json::from_value(written).unwrap();
        assert_eq!(parsed, project);
    }

    #[test]
    fn validate_should_reject_an_asset_tempo_out_of_range() {
        for bpm in [0.0, 5.0, 1_000.0, f64::INFINITY] {
            let mut project = sample_project();
            project.assets[0].bpm = Some(bpm);
            assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
        }
    }

    #[test]
    fn validate_should_reject_a_video_id_taken_by_an_audio_asset() {
        let mut project = video_project();
        project.videos[0].id = "a1".into();
        video_clip(&mut project).asset = "a1".into();
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn validate_should_reject_video_paths_outside_the_video_folder() {
        for bad in [
            "audio/intro.mp4",
            "video/../x.mp4",
            "video/sub/x.mp4",
            "video/",
            "intro.mp4",
        ] {
            let mut project = video_project();
            project.videos[0].file = bad.into();
            assert!(
                matches!(project.validate(), Err(ProjectError::InvalidAssetPath(_))),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn validate_should_reject_asset_paths_outside_the_audio_folder() {
        for bad in [
            "audio/../evil.wav",
            "audio/sub/x.wav",
            "audio/",
            "click.wav",
            "/audio/click.wav",
            "audio\\x.wav",
        ] {
            let mut project = sample_project();
            project.assets[0].file = bad.into();
            assert!(
                matches!(project.validate(), Err(ProjectError::InvalidAssetPath(_))),
                "{bad} should be rejected"
            );
        }
    }
}
