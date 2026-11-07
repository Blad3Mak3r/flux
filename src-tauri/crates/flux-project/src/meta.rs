//! Who a project is and when it was made: the part of `project.json` that says nothing about the show.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::{ProjectError, Result};

/// The FLUX version that writes the file.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Identity and history of a project. Dates are UTC, RFC 3339 (`2026-11-05T14:29:03Z`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    /// A UUID that stays with the project through renames, copies of the file and saves.
    pub id: String,
    pub created_at: String,
    /// The last save.
    pub modified_at: String,
    /// The FLUX version that created the project; empty when it is not known.
    pub created_with: String,
    /// The FLUX version of the last save. Empty means the file was written before projects
    /// carried metadata, which is how opening one knows to save it again.
    pub saved_with: String,
    /// Who made the show; free text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    /// What the show is; free text, possibly several lines.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

impl Meta {
    /// The metadata of a project created now.
    pub fn new() -> Self {
        let now = now();
        Self {
            id: Uuid::new_v4().to_string(),
            created_at: now.clone(),
            modified_at: now,
            created_with: APP_VERSION.to_owned(),
            saved_with: String::new(),
            author: String::new(),
            description: String::new(),
        }
    }

    /// Records a save happening now.
    pub fn stamp(&mut self) {
        self.modified_at = now();
        self.saved_with = APP_VERSION.to_owned();
    }

    /// Whether a version that writes metadata has saved the project.
    pub fn is_stamped(&self) -> bool {
        !self.saved_with.is_empty()
    }

    pub(super) fn validate(&self) -> Result<()> {
        Uuid::parse_str(&self.id)
            .map(drop)
            .map_err(|_| ProjectError::Invalid(format!("project id is not a UUID: {}", self.id)))
    }
}

impl Default for Meta {
    fn default() -> Self {
        Self::new()
    }
}

/// The current time as RFC 3339 UTC.
pub fn now() -> String {
    format_time(SystemTime::now())
}

/// `time` as RFC 3339 UTC, whole seconds. A time before 1970 is written as the epoch.
pub fn format_time(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Year, month and day of a count of days since 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(secs: u64) -> String {
        format_time(UNIX_EPOCH + Duration::from_secs(secs))
    }

    #[test]
    fn format_time_should_write_rfc_3339_utc() {
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(at(1_783_261_743), "2026-07-05T14:29:03Z");
        assert_eq!(at(4_107_542_399), "2100-02-28T23:59:59Z");
    }

    #[test]
    fn new_should_make_a_uuid_that_validates_and_dates_that_agree() {
        let meta = Meta::new();
        assert!(meta.validate().is_ok());
        assert_eq!(meta.created_at, meta.modified_at);
        assert_eq!(meta.created_with, APP_VERSION);
        assert!(!meta.is_stamped());
    }

    #[test]
    fn new_should_make_a_different_id_each_time() {
        assert_ne!(Meta::new().id, Meta::new().id);
    }

    #[test]
    fn stamp_should_record_the_version_and_leave_the_creation_alone() {
        let mut meta = Meta::new();
        meta.created_at = "2020-01-01T00:00:00Z".into();
        meta.stamp();
        assert!(meta.is_stamped());
        assert_eq!(meta.saved_with, APP_VERSION);
        assert_eq!(meta.created_at, "2020-01-01T00:00:00Z");
        assert_ne!(meta.modified_at, meta.created_at);
    }

    #[test]
    fn validate_should_reject_an_id_that_is_not_a_uuid() {
        let mut meta = Meta::new();
        meta.id = "titanium".into();
        assert!(matches!(meta.validate(), Err(ProjectError::Invalid(_))));
    }
}
