// SPDX-License-Identifier: MIT
use crate::{
    model::MF4_PID,
    state::{Change, State, changed, hex, predict},
    transport::{Backend, Device},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u32,
    pub layout: String,
    pub captured_at_unix: u64,
    pub device: Device,
    pub payload_hex: String,
}

impl Snapshot {
    pub fn new(device: Device, state: &State) -> Self {
        Self {
            schema_version: 1,
            layout: "minifuse4-b2-190-v1".into(),
            captured_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            device,
            payload_hex: hex(&state.0),
        }
    }
    pub fn state(&self) -> Result<State, String> {
        if self.schema_version != 1
            || self.layout != "minifuse4-b2-190-v1"
            || self.device.product_id != MF4_PID
        {
            return Err("Unsupported snapshot schema, model or B2 layout".into());
        }
        self.device.require_mf4()?;
        State::from_hex(&self.payload_hex)
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        // Bounded input: this format contains a 380-character hex payload.
        if fs::metadata(path).map_err(|e| e.to_string())?.len() > 64 * 1024 {
            return Err("Snapshot exceeds 64 KiB".into());
        }
        let snapshot: Self = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        snapshot.state()?;
        Ok(snapshot)
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        save_new(
            path,
            &serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
    }
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}
fn sync_parent(path: &Path) -> Result<(), String> {
    File::open(parent(path))
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}

fn create_backup_dir(path: &Path) -> Result<(), String> {
    let mut missing = Vec::new();
    let mut cursor = path;
    while !cursor.as_os_str().is_empty() && !cursor.exists() {
        missing.push(cursor.to_path_buf());
        cursor = parent(cursor);
    }
    fs::create_dir_all(path).map_err(|e| format!("Cannot create backup directory: {e}"))?;
    // Sync newly created directory entries as well as the eventual files.
    for directory in missing.into_iter().rev() {
        File::open(&directory)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        sync_parent(&directory)?;
    }
    Ok(())
}

pub fn save_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("Cannot create {}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    sync_parent(path)
}

pub fn default_backup_dir() -> PathBuf {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(state).join("mf-cli");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/mf-cli")
}

pub fn apply<B: Backend>(
    backend: &mut B,
    device: &Device,
    before: &State,
    changes: Vec<Change>,
    directory: &Path,
) -> Result<Option<PathBuf>, String> {
    device.require_mf4()?;
    let changes = changed(before, changes)?;
    if changes.is_empty() {
        return Ok(None);
    }
    let expected = predict(before, &changes)?;
    create_backup_dir(directory)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let base = format!("mf4-{stamp}-{}", std::process::id());
    let backup = directory.join(format!("{base}.json"));
    Snapshot::new(device.clone(), before).save(&backup)?;
    let journal_path = directory.join(format!("{base}.operation.json"));
    let mut journal = serde_json::json!({ "schema_version": 1, "status": "prepared", "backup": backup,
        "device": device, "changes": changes, "expected_after_hex": hex(&expected.0) });
    save_new(
        &journal_path,
        &serde_json::to_vec_pretty(&journal).map_err(|e| e.to_string())?,
    )?;
    // Both files have been synced before invoking any device mutation.
    let result = backend.apply(device, before, &changes).and_then(|after| {
        if after != expected {
            Err("Backend returned an unverified state".into())
        } else {
            Ok(after)
        }
    });
    match &result {
        Ok(after) => {
            journal["status"] = "verified".into();
            journal["after_hex"] = hex(&after.0).into();
        }
        Err(error) => {
            journal["status"] = "failed".into();
            journal["error"] = error.clone().into();
        }
    }
    let temp = directory.join(format!("{base}.operation.pending"));
    let persist = save_new(
        &temp,
        &serde_json::to_vec_pretty(&journal).map_err(|e| e.to_string())?,
    )
    .and_then(|_| fs::rename(&temp, &journal_path).map_err(|e| e.to_string()))
    .and_then(|_| sync_parent(&journal_path));
    if let Err(error) = persist {
        return Err(format!(
            "Device result: {}. Could not finalize journal: {error}. Backup: {}",
            if result.is_ok() {
                "verified".into()
            } else {
                result.unwrap_err()
            },
            backup.display()
        ));
    }
    result.map_err(|e| format!("{e} Backup and operation record: {}", backup.display()))?;
    Ok(Some(journal_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        calls: usize,
        fail: bool,
        directory: PathBuf,
    }
    impl Backend for Fake {
        fn apply(&mut self, _: &Device, s: &State, c: &[Change]) -> Result<State, String> {
            self.calls += 1;
            let paths: Vec<_> = fs::read_dir(&self.directory).unwrap().collect();
            assert_eq!(
                paths.len(),
                2,
                "Backup and prepared journal must precede writes"
            );
            if self.fail {
                Err("Simulated USB failure; rollback verified".into())
            } else {
                predict(s, c)
            }
        }
    }
    fn temp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "mf-cli-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        p
    }
    #[test]
    fn snapshot_roundtrip_and_no_overwrite() {
        let dir = temp();
        let path = dir.join("state.json");
        let state = State([0; 190]);
        let snap = Snapshot::new(crate::transport::reference_device(), &state);
        snap.save(&path).unwrap();
        assert!(snap.save(&path).is_err());
        assert_eq!(Snapshot::load(&path).unwrap().state().unwrap(), state);
        let mut bad = snap;
        bad.device.product_id = 0xaf90;
        assert!(bad.state().is_err());
        bad.device.product_id = MF4_PID;
        bad.schema_version = 2;
        assert!(bad.state().is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn journals_success_failure_and_noops() {
        for fail in [false, true] {
            let directory = temp();
            let mut fake = Fake {
                calls: 0,
                fail,
                directory: directory.clone(),
            };
            let state = State([0; 190]);
            let dev = crate::transport::reference_device();
            assert!(
                apply(&mut fake, &dev, &state, vec![], &directory)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(fake.calls, 0);
            let result = apply(
                &mut fake,
                &dev,
                &state,
                vec![Change {
                    selector: 0x0900,
                    value: 0x8000,
                }],
                &directory,
            );
            assert_eq!(result.is_err(), fail);
            assert_eq!(fake.calls, 1);
            let journal = fs::read_dir(&directory)
                .unwrap()
                .map(|p| p.unwrap().path())
                .find(|p| p.to_string_lossy().ends_with("operation.json"))
                .unwrap();
            let data: serde_json::Value =
                serde_json::from_slice(&fs::read(journal).unwrap()).unwrap();
            assert_eq!(data["status"], if fail { "failed" } else { "verified" });
            fs::remove_dir_all(directory).unwrap();
        }
    }
    #[test]
    fn invalid_plan_or_unwritable_backup_never_reaches_backend() {
        let directory = temp();
        let mut fake = Fake {
            calls: 0,
            fail: false,
            directory: directory.clone(),
        };
        let state = State([0; 190]);
        let dev = crate::transport::reference_device();
        assert!(
            apply(
                &mut fake,
                &dev,
                &state,
                vec![Change {
                    selector: 0x5600,
                    value: 1
                }],
                &directory
            )
            .is_err()
        );
        let file = directory.join("file");
        fs::write(&file, b"x").unwrap();
        assert!(
            apply(
                &mut fake,
                &dev,
                &state,
                vec![Change {
                    selector: 0x0900,
                    value: 0x8000
                }],
                &file
            )
            .is_err()
        );
        assert_eq!(fake.calls, 0);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn wrong_readback_is_never_reported_as_verified() {
        struct Wrong;
        impl Backend for Wrong {
            fn apply(&mut self, _: &Device, state: &State, _: &[Change]) -> Result<State, String> {
                Ok(state.clone())
            }
        }
        let dir = temp();
        let result = apply(
            &mut Wrong,
            &crate::transport::reference_device(),
            &State([0; 190]),
            vec![Change {
                selector: 0x0900,
                value: 0x8000,
            }],
            &dir,
        );
        assert!(result.is_err());
        let journal = fs::read_dir(&dir)
            .unwrap()
            .map(|p| p.unwrap().path())
            .find(|p| p.to_string_lossy().ends_with("operation.json"))
            .unwrap();
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(journal).unwrap()).unwrap();
        assert_eq!(record["status"], "failed");
        fs::remove_dir_all(dir).unwrap();
    }
}
