//! 数据库备份/恢复：把共享库与全部用户账本一致性地快照并打包为 zip。
//!
//! 快照使用 SQLite 的 `VACUUM INTO`（在线、一致性、自动压缩），逐文件生成
//! 临时快照后打包进 `backups/koku-<UTC时间戳>.zip`。恢复时先解压到临时目录，
//! 再逐个原子 `rename` 覆盖线上文件（并清理旧 WAL/SHM）；Linux 下 rename 对
//! 已打开的连接是安全的——旧连接继续写旧 inode，新连接自动打开新文件。
//! 调用方应在恢复后重新打开共享库连接并清空账本连接缓存（见 API 层）。

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use rusqlite::Connection;
use serde::Serialize;

use crate::error::{KokuError, Result};

/// 备份目录：数据库文件同级的 `backups/`（与部署脚本约定一致）。
pub fn backup_dir(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .unwrap_or_else(|| Path::new("data"))
        .join("backups")
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupMeta {
    /// 备份标识（文件名去扩展名，`koku-<YYYYmmdd-HHMMSS>`）。
    pub id: String,
    pub filename: String,
    /// RFC3339 创建时间（UTC）。
    pub created_at: String,
    pub size_bytes: u64,
    /// 包内文件相对路径（`koku.db` 与 `ledgers/ledger-<id>.db`）。
    pub files: Vec<String>,
}

/// 校验备份 id，防止路径穿越；合法字符集：字母数字、点、下划线、连字符。
fn validate_backup_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(KokuError::InvalidInput("invalid backup id".to_owned()));
    }
    Ok(())
}

/// 收集需要备份的数据库文件：共享库 + `ledgers/ledger-*.db`（排除 WAL/SHM）。
fn collect_db_files(db_path: &Path, ledger_dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut files = vec![(db_path.to_path_buf(), "koku.db".to_owned())];
    let mut ledgers: Vec<PathBuf> = fs::read_dir(ledger_dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("ledger-") && name.ends_with(".db"))
        })
        .collect();
    ledgers.sort();
    for ledger in ledgers {
        let name = ledger
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("ledger.db")
            .to_owned();
        files.push((ledger, format!("ledgers/{name}")));
    }
    Ok(files)
}

/// 在线快照单个数据库文件（`VACUUM INTO`），返回快照路径。
fn vacuum_snapshot(source: &Path, target: &Path) -> Result<()> {
    let conn = Connection::open(source)?;
    // VACUUM INTO 的目标必须不存在。
    if target.exists() {
        fs::remove_file(target)?;
    }
    // SQLite 字符串字面量里的单引号需要转义（路径可能包含单引号）。
    let escaped = target.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!("VACUUM INTO '{escaped}'"))?;
    Ok(())
}

/// 创建一份完整备份，返回元数据；同时按 `keep` 清理最旧的备份（0 表示不清理）。
pub fn create_backup(db_path: &Path, ledger_dir: &Path, keep: usize) -> Result<BackupMeta> {
    let dir = backup_dir(db_path);
    fs::create_dir_all(&dir)?;
    let now = Utc::now();
    let mut id = now.format("%Y%m%d-%H%M%S").to_string();
    let mut filename = format!("koku-{id}.zip");
    let archive_path = dir.join(&filename);
    if archive_path.exists() {
        // 同一秒内重复创建：追加随机后缀避免覆盖。
        let nonce: u64 = {
            let mut bytes = [0_u8; 8];
            getrandom::fill(&mut bytes)
                .map_err(|error| KokuError::InvalidInput(format!("rng failure: {error}")))?;
            u64::from_le_bytes(bytes) % 100_000
        };
        id = format!("{id}-{nonce:05}");
        filename = format!("koku-{id}.zip");
    }
    let archive_path = dir.join(&filename);

    let files = collect_db_files(db_path, ledger_dir)?;
    let snapshot_dir =
        std::env::temp_dir().join(format!("koku-snapshot-{id}-{}", std::process::id()));
    fs::create_dir_all(&snapshot_dir)?;

    let mut snapshots: Vec<(PathBuf, String)> = Vec::new();
    let mut archive_files: Vec<String> = Vec::new();
    for (source, relative) in &files {
        if !source.exists() {
            continue;
        }
        let snapshot = snapshot_dir.join(relative.replace('/', "__"));
        vacuum_snapshot(source, &snapshot)?;
        snapshots.push((snapshot, relative.clone()));
        archive_files.push(relative.clone());
    }

    let mut buffer = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut buffer));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (snapshot, relative) in &snapshots {
            writer
                .start_file(relative.as_str(), options)
                .map_err(|error| KokuError::InvalidInput(format!("zip write failed: {error}")))?;
            writer
                .write_all(&fs::read(snapshot)?)
                .map_err(|error| KokuError::InvalidInput(format!("zip write failed: {error}")))?;
        }
        writer
            .finish()
            .map_err(|error| KokuError::InvalidInput(format!("zip finish failed: {error}")))?;
    }
    let size_bytes = buffer.len() as u64;
    fs::write(&archive_path, buffer)?;
    let _ = fs::remove_dir_all(&snapshot_dir);

    if keep > 0 {
        prune_old_backups(&dir, keep);
    }

    Ok(BackupMeta {
        id,
        filename,
        created_at: now.to_rfc3339_opts(SecondsFormat::Secs, true),
        size_bytes,
        files: archive_files,
    })
}

/// 列出全部备份（按创建时间倒序）。
pub fn list_backups(db_path: &Path) -> Result<Vec<BackupMeta>> {
    let dir = backup_dir(db_path);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut backups: Vec<BackupMeta> = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let filename = entry.file_name().to_string_lossy().to_string();
        let Some(id) = filename
            .strip_prefix("koku-")
            .and_then(|rest| rest.strip_suffix(".zip"))
            .map(str::to_owned)
        else {
            continue;
        };
        let size_bytes = entry.metadata()?.len();
        // 打开 zip 读包内文件清单（备份数量通常很少，开销可忽略）。
        let mut files = Vec::new();
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) {
                for index in 0..archive.len() {
                    if let Ok(entry) = archive.by_index(index) {
                        files.push(entry.name().to_owned());
                    }
                }
            }
        }
        let created_at = created_at_from_id(&id);
        backups.push(BackupMeta {
            id,
            filename,
            created_at,
            size_bytes,
            files,
        });
    }
    backups.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(backups)
}

/// 从 `YYYYmmdd-HHMMSS`（UTC）推导创建时间；解析失败时回退到当前时间（不会发生）。
fn created_at_from_id(id: &str) -> String {
    let parse = || {
        let (date_part, time_part) = id.split_once('-')?;
        if date_part.len() != 8 || time_part.len() != 6 {
            return None;
        }
        let year: i32 = date_part[0..4].parse().ok()?;
        let month: u32 = date_part[4..6].parse().ok()?;
        let day: u32 = date_part[6..8].parse().ok()?;
        let hour: u32 = time_part[0..2].parse().ok()?;
        let minute: u32 = time_part[2..4].parse().ok()?;
        let second: u32 = time_part[4..6].parse().ok()?;
        chrono::NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, minute, second)
    };
    parse()
        .map(|naive| {
            chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc)
                .to_rfc3339_opts(SecondsFormat::Secs, true)
        })
        .unwrap_or_else(|| Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// 删除最旧的备份，只保留最近 `keep` 个（按文件修改时间）。
fn prune_old_backups(dir: &Path, keep: usize) {
    let mut backups: Vec<(String, std::time::SystemTime)> = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let filename = entry.file_name().to_string_lossy().to_string();
        if !(filename.starts_with("koku-") && filename.ends_with(".zip")) {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        backups.push((filename, modified));
    }
    backups.sort_by_key(|(_, modified)| *modified);
    while backups.len() > keep {
        let (oldest, _) = backups.remove(0);
        let _ = fs::remove_file(dir.join(oldest));
    }
}

/// 恢复备份：解压到临时目录后原子覆盖线上文件，并清理目标 WAL/SHM。
/// 调用方需在恢复后重开共享库连接并清空账本缓存。
pub fn restore_backup(db_path: &Path, ledger_dir: &Path, id: &str) -> Result<()> {
    validate_backup_id(id)?;
    let dir = backup_dir(db_path);
    let archive_path = dir.join(format!("koku-{id}.zip"));
    let bytes = fs::read(&archive_path)
        .map_err(|error| KokuError::InvalidInput(format!("backup not found: {error}")))?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| KokuError::InvalidInput(format!("invalid backup archive: {error}")))?;

    // 先全部解压到临时目录，确认无 zip-slip 路径后再逐个覆盖。
    let staging = std::env::temp_dir().join(format!("koku-restore-{id}-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    struct RestoreFile {
        target: PathBuf,
        staged: PathBuf,
    }
    let mut restores: Vec<RestoreFile> = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| KokuError::InvalidInput(format!("invalid backup entry: {error}")))?;
        let relative = entry.name().to_owned();
        // 防 zip-slip：拒绝绝对路径与 .. 片段。
        if relative.starts_with('/')
            || relative
                .split(['/', '\\'])
                .any(|segment| segment == ".." || segment.is_empty())
        {
            return Err(KokuError::InvalidInput(format!(
                "unsafe path in backup: {relative}"
            )));
        }
        let target = if relative == "koku.db" {
            db_path.to_path_buf()
        } else if let Some(ledger_name) = relative.strip_prefix("ledgers/") {
            ledger_dir.join(ledger_name)
        } else {
            return Err(KokuError::InvalidInput(format!(
                "unknown file in backup: {relative}"
            )));
        };
        let staged = staging.join(relative.replace('/', "__"));
        let mut buffer = Vec::with_capacity(entry.size() as usize);
        std::io::Read::read_to_end(&mut entry, &mut buffer)?;
        fs::write(&staged, buffer)?;
        restores.push(RestoreFile { target, staged });
    }

    // 逐个原子覆盖，并清理旧 WAL/SHM（防止残留 WAL 与新文件错配）。
    for restore in &restores {
        if let Some(parent) = restore.target.parent() {
            fs::create_dir_all(parent)?;
        }
        for suffix in ["", "-wal", "-shm"] {
            let path = PathBuf::from(format!("{}{suffix}", restore.target.display()));
            if path.exists() {
                fs::remove_file(&path)?;
            }
        }
        fs::rename(&restore.staged, &restore.target)?;
    }
    let _ = fs::remove_dir_all(&staging);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "koku-backup-test-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_db(path: &Path, marker: &str) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS t (value TEXT);")
            .unwrap();
        conn.execute("INSERT INTO t VALUES (?1)", [marker]).unwrap();
    }

    fn db_marker(path: &Path) -> String {
        let conn = Connection::open(path).unwrap();
        conn.query_row("SELECT value FROM t", [], |row| row.get::<_, String>(0))
            .unwrap()
    }

    #[test]
    fn backup_roundtrip_preserves_all_databases() {
        let root = temp_workspace();
        let db_path = root.join("koku.db");
        let ledger_dir = root.join("ledgers");
        fs::create_dir_all(&ledger_dir).unwrap();
        make_db(&db_path, "shared-v1");
        make_db(&ledger_dir.join("ledger-1.db"), "ledger-1-v1");
        make_db(&ledger_dir.join("ledger-2.db"), "ledger-2-v1");

        let meta = create_backup(&db_path, &ledger_dir, 0).unwrap();
        assert_eq!(meta.files.len(), 3);
        assert!(meta.files.iter().any(|file| file == "koku.db"));
        assert!(meta.files.iter().any(|file| file == "ledgers/ledger-1.db"));

        // 篡改线上数据后恢复，应还原到备份时的内容。
        make_db(&db_path, "shared-tampered");
        make_db(&ledger_dir.join("ledger-1.db"), "ledger-1-tampered");
        fs::remove_file(ledger_dir.join("ledger-2.db")).unwrap();

        restore_backup(&db_path, &ledger_dir, &meta.id).unwrap();
        assert_eq!(db_marker(&db_path), "shared-v1");
        assert_eq!(db_marker(&ledger_dir.join("ledger-1.db")), "ledger-1-v1");
        assert_eq!(db_marker(&ledger_dir.join("ledger-2.db")), "ledger-2-v1");

        let listed = list_backups(&db_path).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, meta.id);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_unsafe_backup_ids() {
        assert!(validate_backup_id("20240101-120000").is_ok());
        assert!(validate_backup_id("../evil").is_err());
        assert!(validate_backup_id("a/b").is_err());
        assert!(validate_backup_id("").is_err());
    }

    #[test]
    fn restore_missing_backup_errors() {
        let root = temp_workspace();
        let db_path = root.join("koku.db");
        make_db(&db_path, "v1");
        assert!(restore_backup(&db_path, &root.join("ledgers"), "20240101-000000").is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
