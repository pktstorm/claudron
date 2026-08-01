use crate::conversation::model::{ToolResultUpdate, Turn};
use crate::conversation::parse::{parse_with_pending, PendingCalls};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

pub struct TailRead {
    pub turns: Vec<Turn>,
    pub updates: Vec<ToolResultUpdate>,
    pub offset: u64,
    pub reset: bool,
    pub pending: PendingCalls,
}

/// Read new complete lines from `offset`, returning parsed turns, the new
/// offset, and whether the caller must reset.
///
/// Only complete newline-terminated lines are consumed. A transcript being
/// appended to right now can end mid-line; parsing that would render a torn
/// record, and advancing past it would lose the line entirely.
pub fn read_from(path: &Path, offset: u64, pending: PendingCalls) -> std::io::Result<TailRead> {
    let len = std::fs::metadata(path)?.len();

    // A shorter file means truncation or replacement: start over, and drop the
    // carried pending state -- its indices refer to turns the client is about
    // to discard.
    let (start, reset) = if len < offset {
        (0, true)
    } else {
        (offset, false)
    };
    let carried = if reset {
        PendingCalls::default()
    } else {
        pending
    };

    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(start))?;

    let mut consumed = start;
    let mut complete: Vec<String> = Vec::new();
    let mut reader = BufReader::new(file);
    loop {
        let mut buf = String::new();
        let n = reader.read_line(&mut buf)?;
        if n == 0 {
            break;
        }
        if !buf.ends_with('\n') {
            break; // partial trailing line -- leave it for the next poll
        }
        consumed += n as u64;
        complete.push(buf);
    }

    let out = parse_with_pending(complete.into_iter(), carried);
    Ok(TailRead {
        turns: out.turns,
        updates: out.updates,
        offset: consumed,
        reset,
        pending: out.pending,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const A1: &str = r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"text","text":"first"}]}}"#;
    const A2: &str = r#"{"type":"assistant","uuid":"a2","timestamp":"T2","message":{"role":"assistant","content":[{"type":"text","text":"second"}]}}"#;

    fn write_file(dir: &std::path::Path, lines: &[&str]) -> std::path::PathBuf {
        let p = dir.join("t.jsonl");
        let mut f = std::fs::File::create(&p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        p
    }

    #[test]
    fn reads_everything_from_offset_zero() {
        let d = tempfile::tempdir().unwrap();
        let p = write_file(d.path(), &[A1, A2]);
        let r = read_from(&p, 0, PendingCalls::default()).unwrap();
        let (turns, offset, reset) = (r.turns, r.offset, r.reset);
        assert_eq!(turns.len(), 2);
        assert_eq!(offset, std::fs::metadata(&p).unwrap().len());
        assert!(!reset);
    }

    #[test]
    fn a_second_read_at_the_same_offset_returns_nothing() {
        let d = tempfile::tempdir().unwrap();
        let p = write_file(d.path(), &[A1, A2]);
        let first = read_from(&p, 0, PendingCalls::default()).unwrap();
        let r = read_from(&p, first.offset, first.pending).unwrap();
        let (turns, offset2, reset) = (r.turns, r.offset, r.reset);
        let offset = first.offset;
        assert!(
            turns.is_empty(),
            "an unchanged file must yield no new turns"
        );
        assert_eq!(offset2, offset);
        assert!(!reset);
    }

    #[test]
    fn an_append_yields_only_the_new_turn() {
        let d = tempfile::tempdir().unwrap();
        let p = write_file(d.path(), &[A1]);
        let r0 = read_from(&p, 0, PendingCalls::default()).unwrap();
        let (first, offset) = (r0.turns, r0.offset);
        assert_eq!(first.len(), 1);

        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, "{A2}").unwrap();
        drop(f);

        let r1 = read_from(&p, offset, PendingCalls::default()).unwrap();
        let (new, reset) = (r1.turns, r1.reset);
        assert_eq!(new.len(), 1, "only the appended turn");
        assert_eq!(new[0].uuid, "a2");
        assert!(!reset);
    }

    #[test]
    fn a_truncated_file_signals_reset_and_re_reads() {
        let d = tempfile::tempdir().unwrap();
        let p = write_file(d.path(), &[A1, A2]);
        let offset = read_from(&p, 0, PendingCalls::default()).unwrap().offset;

        // Replace with a shorter file -- the offset is now past the end.
        write_file(d.path(), &[A1]);

        let r = read_from(&p, offset, PendingCalls::default()).unwrap();
        let (turns, new_offset, reset) = (r.turns, r.offset, r.reset);
        assert!(reset, "a shrunken file must signal reset");
        assert_eq!(turns.len(), 1, "and re-read from the start");
        assert_eq!(new_offset, std::fs::metadata(&p).unwrap().len());
    }

    #[test]
    fn a_missing_file_is_an_error_not_a_panic() {
        assert!(read_from(
            std::path::Path::new("/nonexistent/x.jsonl"),
            0,
            PendingCalls::default()
        )
        .is_err());
    }

    #[test]
    fn a_result_appended_after_its_call_surfaces_as_an_update() {
        // The realistic live case: a tool call is written, the poll fires, the
        // tool finishes seconds later. Measured: 60% of real calls take longer
        // than the 1s poll interval, so this is the common path, not an edge.
        const CALL: &str = r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"description":"Run tests"}}]}}"#;
        const RESULT: &str = r#"{"type":"user","uuid":"u1","timestamp":"T2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"all green"}]}}"#;

        let d = tempfile::tempdir().unwrap();
        let p = write_file(d.path(), &[CALL]);
        let first = read_from(&p, 0, PendingCalls::default()).unwrap();
        assert_eq!(first.turns.len(), 1);
        assert!(first.updates.is_empty());

        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, "{RESULT}").unwrap();
        drop(f);

        let second = read_from(&p, first.offset, first.pending).unwrap();
        assert!(second.turns.is_empty(), "no new turns, just a result");
        assert_eq!(second.updates.len(), 1, "the result must not be lost");
        assert_eq!(second.updates[0].result, "all green");
    }

    #[test]
    fn a_partial_trailing_line_is_not_consumed() {
        // A transcript being appended to right now can end mid-line. That
        // partial line must not be parsed, and the offset must stop before it
        // so the next poll picks it up whole.
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.jsonl");
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, "{A1}").unwrap();
        write!(f, "{{\"type\":\"assistant\",\"uuid\":\"partial\"").unwrap(); // no newline
        drop(f);

        let r = read_from(&p, 0, PendingCalls::default()).unwrap();
        let (turns, offset) = (r.turns, r.offset);
        assert_eq!(turns.len(), 1, "only the complete line parses");
        assert_eq!(
            offset,
            (A1.len() + 1) as u64,
            "offset must stop after the last complete line"
        );
    }

    #[test]
    #[ignore]
    fn a_warm_poll_of_the_largest_real_transcript_is_fast() {
        use crate::conversation::parse::PendingCalls;
        let root = crate::index::projects_root();
        if !root.exists() {
            return;
        }
        // Find the largest depth-2 transcript.
        let mut biggest: Option<(u64, std::path::PathBuf)> = None;
        for e in walkdir::WalkDir::new(&root)
            .max_depth(2)
            .into_iter()
            .filter_map(Result::ok)
        {
            if e.path().extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let len = e.metadata().map(|m| m.len()).unwrap_or(0);
            if biggest.as_ref().map(|(b, _)| len > *b).unwrap_or(true) {
                biggest = Some((len, e.path().to_path_buf()));
            }
        }
        let Some((len, path)) = biggest else { return };

        // Cold read to reach the end of the file.
        let cold = read_from(&path, 0, PendingCalls::default()).unwrap();
        // Warm poll: nothing new, so this should do almost no work.
        let t0 = std::time::Instant::now();
        let warm = read_from(&path, cold.offset, cold.pending).unwrap();
        let elapsed = t0.elapsed();
        println!(
            "file {len} bytes, cold turns {}, warm turns {}, warm poll {elapsed:?}",
            cold.turns.len(),
            warm.turns.len()
        );
        assert!(
            warm.turns.is_empty(),
            "an unchanged file must yield no new turns"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(50),
            "warm poll took {elapsed:?}, criterion 2 requires under 50ms"
        );
    }
}
