use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TranscriptSummary {
    pub session_id: String,
    pub ai_title: Option<String>,
    pub last_prompt: Option<String>,
    pub git_branch: Option<String>,
    pub cwd: Option<String>,
    pub version: Option<String>,
    pub interrupted: bool,
}

pub fn parse_transcript(path: &Path) -> Option<TranscriptSummary> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut summary = TranscriptSummary::default();
    let mut is_cli = false;

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        // A sidechain record anywhere disqualifies the whole transcript.
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            return None;
        }

        if let Some(ep) = v.get("entrypoint").and_then(Value::as_str) {
            if ep == "cli" {
                is_cli = true;
            }
        }

        if let Some(id) = v.get("sessionId").and_then(Value::as_str) {
            if summary.session_id.is_empty() {
                summary.session_id = id.to_string();
            }
        }

        // `ai-title` and `last-prompt` repeat; the last one wins. A
        // `last-prompt` record may carry only a leafUuid and no prompt --
        // those must not overwrite a real value.
        if let Some(t) = v.get("aiTitle").and_then(Value::as_str) {
            summary.ai_title = Some(t.to_string());
        }
        if let Some(p) = v.get("lastPrompt").and_then(Value::as_str) {
            summary.last_prompt = Some(p.to_string());
        }
        if let Some(b) = v.get("gitBranch").and_then(Value::as_str) {
            summary.git_branch = Some(b.to_string());
        }
        if let Some(c) = v.get("cwd").and_then(Value::as_str) {
            summary.cwd = Some(c.to_string());
        }
        if let Some(ver) = v.get("version").and_then(Value::as_str) {
            summary.version = Some(ver.to_string());
        }
        if v.get("interruptedByShutdown").and_then(Value::as_bool) == Some(true) {
            summary.interrupted = true;
        }
    }

    if !is_cli || summary.session_id.is_empty() {
        return None;
    }
    Some(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_jsonl(lines: &[&str]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for l in lines {
            writeln!(f, "{}", l).unwrap();
        }
        f.flush().unwrap();
        f
    }

    #[test]
    fn parses_a_cli_session() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"cli","sessionId":"s1","cwd":"/repo","gitBranch":"main","version":"2.1.220","isSidechain":false}"#,
            r#"{"type":"ai-title","aiTitle":"Fix the parser","sessionId":"s1"}"#,
            r#"{"type":"last-prompt","lastPrompt":"do the thing","sessionId":"s1"}"#,
        ]);
        let s = parse_transcript(f.path()).expect("should parse");
        assert_eq!(s.session_id, "s1");
        assert_eq!(s.ai_title.as_deref(), Some("Fix the parser"));
        assert_eq!(s.last_prompt.as_deref(), Some("do the thing"));
        assert_eq!(s.git_branch.as_deref(), Some("main"));
        assert_eq!(s.cwd.as_deref(), Some("/repo"));
        assert!(!s.interrupted);
    }

    #[test]
    fn rejects_sdk_sessions() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"sdk-py","sessionId":"s2","cwd":"/repo"}"#,
        ]);
        assert!(parse_transcript(f.path()).is_none());
    }

    #[test]
    fn rejects_transcripts_with_no_entrypoint() {
        let f = write_jsonl(&[r#"{"type":"user","sessionId":"s3","cwd":"/repo"}"#]);
        assert!(parse_transcript(f.path()).is_none());
    }

    #[test]
    fn rejects_sidechains() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"cli","sessionId":"s4","cwd":"/repo","isSidechain":true}"#,
        ]);
        assert!(parse_transcript(f.path()).is_none());
    }

    #[test]
    fn takes_the_last_title_and_prompt() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"cli","sessionId":"s5","cwd":"/repo"}"#,
            r#"{"type":"ai-title","aiTitle":"First title","sessionId":"s5"}"#,
            r#"{"type":"last-prompt","lastPrompt":"first prompt","sessionId":"s5"}"#,
            r#"{"type":"ai-title","aiTitle":"Second title","sessionId":"s5"}"#,
            r#"{"type":"last-prompt","lastPrompt":"second prompt","sessionId":"s5"}"#,
        ]);
        let s = parse_transcript(f.path()).unwrap();
        assert_eq!(s.ai_title.as_deref(), Some("Second title"));
        assert_eq!(s.last_prompt.as_deref(), Some("second prompt"));
    }

    #[test]
    fn ignores_last_prompt_records_without_a_prompt_field() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"cli","sessionId":"s6","cwd":"/repo"}"#,
            r#"{"type":"last-prompt","lastPrompt":"real prompt","sessionId":"s6"}"#,
            r#"{"type":"last-prompt","leafUuid":"abc","sessionId":"s6"}"#,
        ]);
        let s = parse_transcript(f.path()).unwrap();
        assert_eq!(s.last_prompt.as_deref(), Some("real prompt"));
    }

    #[test]
    fn detects_interruption() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"cli","sessionId":"s7","cwd":"/repo","interruptedByShutdown":true}"#,
        ]);
        assert!(parse_transcript(f.path()).unwrap().interrupted);
    }

    #[test]
    fn skips_malformed_lines_without_failing() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"cli","sessionId":"s8","cwd":"/repo"}"#,
            r#"this is not json at all"#,
            r#"{"type":"ai-title","aiTitle":"Survived","sessionId":"s8"}"#,
        ]);
        let s = parse_transcript(f.path()).unwrap();
        assert_eq!(s.ai_title.as_deref(), Some("Survived"));
    }

    #[test]
    fn returns_none_for_missing_file() {
        assert!(parse_transcript(Path::new("/nonexistent/x.jsonl")).is_none());
    }
}
