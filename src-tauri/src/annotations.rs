use crate::model::Annotation;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn store_path() -> PathBuf {
    if let Ok(p) = std::env::var("CLAUDRON_STORE_PATH") {
        return PathBuf::from(p);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".claudron")
        .join("annotations.json")
}

/// Load the annotation store.
///
/// A missing file is a legitimately empty store. ANY other failure -- unreadable,
/// corrupt, wrong schema -- is an error, NOT an empty store. Collapsing those to
/// empty is a data-loss path: a caller that then saves would atomically overwrite
/// every existing note with whatever it just inserted.
pub fn load(path: &Path) -> std::io::Result<HashMap<String, Annotation>> {
    match std::fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(e) => Err(e),
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
    }
}

pub fn save(path: &Path, map: &HashMap<String, Annotation>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Write to a sibling temp file and rename, so a crash mid-write cannot
    // leave a half-written store behind.
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(map)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ManualStatus;

    #[test]
    fn round_trips_annotations() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("annotations.json");

        let mut map = HashMap::new();
        map.insert(
            "s1".to_string(),
            Annotation {
                notes: "check the migration".into(),
                status: Some(ManualStatus::Blocked),
                display_name: Some("Migration work".into()),
            },
        );
        save(&p, &map).unwrap();

        let loaded = load(&p).unwrap();
        assert_eq!(loaded.get("s1").unwrap().notes, "check the migration");
        assert_eq!(
            loaded.get("s1").unwrap().status,
            Some(ManualStatus::Blocked)
        );
    }

    #[test]
    fn missing_file_loads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("nope.json")).unwrap().is_empty());
    }

    #[test]
    fn corrupt_file_is_an_error_not_an_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.json");
        std::fs::write(&p, b"{ this is not json").unwrap();
        assert!(
            load(&p).is_err(),
            "a corrupt store must surface as an error, never silently become empty"
        );
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir
            .path()
            .join("nested")
            .join("deep")
            .join("annotations.json");
        let map = HashMap::new();
        save(&p, &map).unwrap();
        assert!(p.exists());
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("annotations.json");
        save(&p, &HashMap::new()).unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains("tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "atomic save must clean up its temp file"
        );
    }
}
