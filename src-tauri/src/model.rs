use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Liveness {
    Managed,
    Legacy,
    Interrupted,
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ManualStatus {
    Blocked,
    NeedsReview,
    WaitingOnMe,
    Background,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub status: Option<ManualStatus>,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub session_id: String,
    pub ai_title: Option<String>,
    pub last_prompt: Option<String>,
    pub git_branch: Option<String>,
    pub cwd: String,
    pub project_label: String,
    pub version: Option<String>,
    pub last_activity: i64,
    pub liveness: Liveness,
    pub annotation: Annotation,
}

/// The `list_sessions` response: the sessions plus the version everything is
/// compared against, so the client does not have to work it out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionList {
    pub sessions: Vec<Session>,
    pub version_baseline: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_serializes_to_camel_case() {
        let s = Session {
            session_id: "abc".into(),
            ai_title: Some("Title".into()),
            last_prompt: None,
            git_branch: Some("main".into()),
            cwd: "/tmp".into(),
            project_label: "repo".into(),
            version: Some("2.1.220".into()),
            last_activity: 1234,
            liveness: Liveness::Idle,
            annotation: Annotation::default(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"sessionId\":\"abc\""));
        assert!(json.contains("\"lastActivity\":1234"));
        assert!(json.contains("\"liveness\":\"idle\""));
    }

    #[test]
    fn manual_status_round_trips() {
        let j = serde_json::to_string(&ManualStatus::NeedsReview).unwrap();
        assert_eq!(j, "\"needsReview\"");
        let back: ManualStatus = serde_json::from_str(&j).unwrap();
        assert_eq!(back, ManualStatus::NeedsReview);
    }

    #[test]
    fn session_serializes_exactly_the_expected_camel_case_keys() {
        let s = Session {
            session_id: "abc".into(),
            ai_title: None,
            last_prompt: None,
            git_branch: None,
            cwd: "/tmp".into(),
            project_label: "repo".into(),
            version: None,
            last_activity: 1234,
            liveness: Liveness::Idle,
            annotation: Annotation::default(),
        };
        let v = serde_json::to_value(&s).unwrap();
        let obj = v.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "aiTitle",
                "annotation",
                "cwd",
                "gitBranch",
                "lastActivity",
                "lastPrompt",
                "liveness",
                "projectLabel",
                "sessionId",
                "version",
            ],
            "Session wire shape changed — src/types.ts must be updated to match"
        );
        // Option fields must serialize as explicit null, never be omitted.
        assert!(obj.get("aiTitle").unwrap().is_null());
    }

    #[test]
    fn session_list_serializes_to_camel_case() {
        let l = SessionList {
            sessions: vec![],
            version_baseline: Some("2.1.220".into()),
        };
        let j = serde_json::to_string(&l).unwrap();
        assert!(j.contains("\"versionBaseline\":\"2.1.220\""), "got {j}");
        assert!(j.contains("\"sessions\":[]"), "got {j}");
    }

    #[test]
    fn session_list_baseline_is_explicit_null_when_absent() {
        let l = SessionList {
            sessions: vec![],
            version_baseline: None,
        };
        let j = serde_json::to_string(&l).unwrap();
        assert!(j.contains("\"versionBaseline\":null"), "got {j}");
    }
}
