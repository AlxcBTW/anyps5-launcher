//! Backups of a game's save data: what the game keeps beside `app0` in the unlocked copy.
//!
//! The game writes its saves to `_sd` and `_sd_mem` and its trophies to `_trophy`
//! ([`unlock`](crate::unlock) carries them across an unlock). [`SaveData`] zips those three folders
//! into one file per backup under the launcher's data folder, lists the backups and puts one back.
//! A restore refuses while the game runs, checks the zip before it touches anything and backs up
//! the current state first, so it can be undone.
//!
//! Backups live in `<data>\Backups\<TITLEID>\<time>-<reason>.zip`. The name carries everything the
//! list needs; the zips hold only the three folders, with `/` separators.

use std::{
    fs::{self, File},
    io::{self, BufReader, BufWriter},
    path::{Path, PathBuf},
};

use thiserror::Error;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{fsx, paths::AppPaths};

/// The folders of an unlocked copy a backup holds, in the order they are written.
pub const FOLDERS: [&str; 3] = ["_sd", "_sd_mem", "_trophy"];

/// How many backups a game keeps.
pub const KEEP: usize = 10;

/// Automatic backups after playing come at most this often (seconds).
pub const AFTER_PLAYING_EVERY: i64 = 3600;

const EXTENSION: &str = "zip";
/// Where a restore unpacks before it swaps, inside the copy so the swap is a rename.
const STAGING: &str = ".restore";

/// Why a backup was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The player asked.
    Manual,
    /// Right before the copy is replaced by an unlock.
    BeforeUnlock,
    /// Right before a restore replaces the saves.
    BeforeRestore,
    /// Right before the copy, and with it the saves, is deleted.
    BeforeUninstall,
    /// The game ended and the last backup is older than [`AFTER_PLAYING_EVERY`].
    AfterPlaying,
}

impl Reason {
    fn tag(self) -> &'static str {
        match self {
            Reason::Manual => "manual",
            Reason::BeforeUnlock => "unlock",
            Reason::BeforeRestore => "restore",
            Reason::BeforeUninstall => "uninstall",
            Reason::AfterPlaying => "played",
        }
    }

    fn from_tag(tag: &str) -> Option<Reason> {
        [
            Reason::Manual,
            Reason::BeforeUnlock,
            Reason::BeforeRestore,
            Reason::BeforeUninstall,
            Reason::AfterPlaying,
        ]
        .into_iter()
        .find(|r| r.tag() == tag)
    }
}

/// One backup file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backup {
    pub path: PathBuf,
    /// Seconds since the Unix epoch, UTC (from the file name).
    pub created: i64,
    pub reason: Reason,
    /// The size of the zip.
    pub bytes: u64,
}

#[derive(Debug, Error)]
pub enum SavesError {
    /// None of the three folders exists or holds a file.
    #[error("the game has no save data yet")]
    NothingToSave,
    #[error("the game is running")]
    GameRunning,
    /// The zip cannot be read, fails its checksums or holds something else than save folders.
    #[error("the backup is damaged")]
    Corrupt,
    /// The swap began and could not finish: the save folders may be missing or half replaced.
    /// `kept`: what they held before is in the newest backup.
    #[error("the restore stopped part way: {source}")]
    Incomplete {
        kept: bool,
        #[source]
        source: io::Error,
    },
    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        #[source]
        source: io::Error,
    },
}

fn io_error(context: &'static str) -> impl Fn(io::Error) -> SavesError {
    move |source| SavesError::Io { context, source }
}

fn zip_error(context: &'static str) -> impl Fn(zip::result::ZipError) -> SavesError {
    move |error| match error {
        zip::result::ZipError::Io(source) => SavesError::Io { context, source },
        _ => SavesError::Corrupt,
    }
}

/// The save data of one game: where the game writes it and where its backups go.
#[derive(Clone, Debug)]
pub struct SaveData {
    copy: PathBuf,
    backups: PathBuf,
    keep: usize,
}

impl SaveData {
    /// The save data of the unlocked copy `copy`, backed up under `backups`.
    pub fn new(copy: PathBuf, backups: PathBuf) -> SaveData {
        SaveData {
            copy,
            backups,
            keep: KEEP,
        }
    }

    /// The same, with the backup folder of `title_id` under the launcher's data folder.
    pub fn for_title(paths: &AppPaths, title_id: &str, copy: PathBuf) -> SaveData {
        SaveData::new(copy, paths.game_backups_dir(title_id))
    }

    /// Keeps `keep` backups instead of [`KEEP`] (at least one).
    pub fn keeping(mut self, keep: usize) -> SaveData {
        self.keep = keep.max(1);
        self
    }

    /// The game has something to back up.
    pub fn exists(&self) -> bool {
        FOLDERS.iter().any(|name| has_files(&self.copy.join(name)))
    }

    /// Zips the save folders into a new backup and prunes the old ones.
    pub fn backup(&self, reason: Reason) -> Result<Backup, SavesError> {
        self.backup_at(reason, unix_now())
    }

    /// Like [`backup`](Self::backup), but does nothing (`Ok(None)`) when there is no save data or
    /// the newest backup is younger than `min_age` seconds.
    pub fn backup_unless_recent(
        &self,
        reason: Reason,
        min_age: i64,
    ) -> Result<Option<Backup>, SavesError> {
        let now = unix_now();
        if self
            .list()
            .first()
            .is_some_and(|newest| now - newest.created < min_age)
        {
            return Ok(None);
        }
        match self.backup_at(reason, now) {
            Ok(backup) => Ok(Some(backup)),
            Err(SavesError::NothingToSave) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn backup_at(&self, reason: Reason, now: i64) -> Result<Backup, SavesError> {
        if !self.exists() {
            return Err(SavesError::NothingToSave);
        }
        fs::create_dir_all(&self.backups).map_err(io_error("create the backup folder"))?;
        let path = self.free_name(now, reason);
        let partial = path.with_extension("zip.partial");
        let written = write_zip(&self.copy, &partial);
        if let Err(error) = written {
            let _ = fs::remove_file(&partial);
            return Err(error);
        }
        fs::rename(&partial, &path).map_err(io_error("finish the backup"))?;
        let bytes = fs::metadata(&path).map_or(0, |m| m.len());
        self.prune();
        Ok(Backup {
            path,
            created: now,
            reason,
            bytes,
        })
    }

    /// The file name for a backup made at `now`; a second backup in the same second moves on.
    fn free_name(&self, now: i64, reason: Reason) -> PathBuf {
        let mut at = now;
        loop {
            let path = self.backups.join(file_name(at, reason));
            if !path.exists() {
                return path;
            }
            at += 1;
        }
    }

    /// The backups of this game, newest first. Files that are not backups are ignored.
    pub fn list(&self) -> Vec<Backup> {
        let Ok(entries) = fs::read_dir(&self.backups) else {
            return Vec::new();
        };
        let mut found: Vec<Backup> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let (created, reason) = parse_name(path.file_name()?.to_str()?)?;
                let bytes = entry.metadata().ok()?.len();
                Some(Backup {
                    path,
                    created,
                    reason,
                    bytes,
                })
            })
            .collect();
        found.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.path.cmp(&a.path)));
        found
    }

    /// Puts `backup` in place of the game's saves and trophies, which are backed up first
    /// (returned, so the player can undo; `None` when there was nothing to keep). Refuses while
    /// the game runs. A damaged zip is found before anything changes.
    pub fn restore(
        &self,
        backup: &Backup,
        game_running: bool,
    ) -> Result<Option<Backup>, SavesError> {
        if game_running {
            return Err(SavesError::GameRunning);
        }
        let staging = self.copy.join(STAGING);
        let _clean = RemoveOnDrop(staging.clone());
        fsx::remove_tree(&staging).map_err(io_error("clear the staging folder"))?;
        unpack(&backup.path, &staging)?;
        // The zip is sound. The state it replaces is kept before the swap.
        let safety = match self.backup_at(Reason::BeforeRestore, unix_now()) {
            Ok(kept) => Some(kept),
            Err(SavesError::NothingToSave) => None,
            Err(error) => return Err(error),
        };
        for name in FOLDERS {
            let current = self.copy.join(name);
            let stopped = |source| SavesError::Incomplete {
                kept: safety.is_some(),
                source,
            };
            if current.exists() {
                fsx::remove_tree_retry(&current).map_err(stopped)?;
            }
            let unpacked = staging.join(name);
            if unpacked.is_dir() {
                fsx::rename_retry(&unpacked, &current).map_err(stopped)?;
            }
        }
        Ok(safety)
    }

    /// Deletes the oldest backups beyond [`keep`](Self::keeping). Backups made after playing go
    /// first, so the ones the player or a restore made are not pushed out by hours of play.
    fn prune(&self) {
        let mut backups = self.list();
        while backups.len() > self.keep {
            let victim = backups
                .iter()
                .rposition(|b| b.reason == Reason::AfterPlaying)
                .unwrap_or(backups.len() - 1);
            let gone = backups.remove(victim);
            let _ = fs::remove_file(&gone.path);
        }
    }
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fsx::remove_tree(&self.0);
    }
}

fn unix_now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

// ---------------------------------------------------------------------------------------------
// File names

/// `20261022T140500Z-manual.zip`
fn file_name(created: i64, reason: Reason) -> String {
    let t = time::OffsetDateTime::from_unix_timestamp(created)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z-{}.{EXTENSION}",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        reason.tag()
    )
}

fn parse_name(name: &str) -> Option<(i64, Reason)> {
    let stem = name.strip_suffix(".zip")?;
    let (stamp, tag) = stem.split_once('-')?;
    let reason = Reason::from_tag(tag)?;
    let bytes = stamp.as_bytes();
    if bytes.len() != 16 || bytes[8] != b'T' || bytes[15] != b'Z' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i32> { stamp.get(range)?.parse().ok() };
    let date = time::Date::from_calendar_date(
        number(0..4)?,
        time::Month::try_from(u8::try_from(number(4..6)?).ok()?).ok()?,
        u8::try_from(number(6..8)?).ok()?,
    )
    .ok()?;
    let clock = time::Time::from_hms(
        u8::try_from(number(9..11)?).ok()?,
        u8::try_from(number(11..13)?).ok()?,
        u8::try_from(number(13..15)?).ok()?,
    )
    .ok()?;
    Some((
        time::PrimitiveDateTime::new(date, clock)
            .assume_utc()
            .unix_timestamp(),
        reason,
    ))
}

// ---------------------------------------------------------------------------------------------
// Zip

/// A folder holds a file somewhere below it.
fn has_files(folder: &Path) -> bool {
    let Ok(entries) = fs::read_dir(folder) else {
        return false;
    };
    entries.flatten().any(|entry| match entry.file_type() {
        Ok(kind) if kind.is_file() => true,
        Ok(kind) if kind.is_dir() => has_files(&entry.path()),
        _ => false,
    })
}

fn write_zip(copy: &Path, target: &Path) -> Result<(), SavesError> {
    let file = File::create(target).map_err(io_error("create the backup"))?;
    let mut zip = ZipWriter::new(BufWriter::new(file));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for name in FOLDERS {
        let folder = copy.join(name);
        if folder.is_dir() {
            add_tree(&mut zip, &folder, name, options)?;
        }
    }
    let mut out = zip.finish().map_err(zip_error("finish the backup"))?;
    io::Write::flush(&mut out).map_err(io_error("finish the backup"))?;
    out.into_inner()
        .map_err(|e| e.into_error())
        .and_then(|file| file.sync_all())
        .map_err(io_error("finish the backup"))
}

/// Adds `dir` as `entry` and everything below it, sorted. Links are left out: they belong to
/// whatever made them, not to the game's own data.
fn add_tree(
    zip: &mut ZipWriter<BufWriter<File>>,
    dir: &Path,
    entry: &str,
    options: SimpleFileOptions,
) -> Result<(), SavesError> {
    zip.add_directory(format!("{entry}/"), options)
        .map_err(zip_error("write the backup"))?;
    let mut children: Vec<_> = fs::read_dir(dir)
        .map_err(io_error("read the saves"))?
        .flatten()
        .collect();
    children.sort_by_key(fs::DirEntry::file_name);
    for child in children {
        let name = format!("{entry}/{}", child.file_name().to_string_lossy());
        let Ok(kind) = child.file_type() else {
            continue;
        };
        if kind.is_dir() {
            add_tree(zip, &child.path(), &name, options)?;
        } else if kind.is_file() {
            zip.start_file(name, options)
                .map_err(zip_error("write the backup"))?;
            let mut source = File::open(child.path()).map_err(io_error("read the saves"))?;
            io::copy(&mut source, zip).map_err(io_error("write the backup"))?;
        }
    }
    Ok(())
}

/// Unpacks the backup into `staging`, reading every byte so a damaged file is found here.
fn unpack(zip_path: &Path, staging: &Path) -> Result<(), SavesError> {
    let file = File::open(zip_path).map_err(io_error("open the backup"))?;
    let mut archive =
        ZipArchive::new(BufReader::new(file)).map_err(zip_error("open the backup"))?;
    fs::create_dir_all(staging).map_err(io_error("create the staging folder"))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(zip_error("read the backup"))?;
        let relative = entry.enclosed_name().ok_or(SavesError::Corrupt)?;
        let top = relative
            .components()
            .next()
            .and_then(|c| c.as_os_str().to_str())
            .ok_or(SavesError::Corrupt)?;
        if !FOLDERS.contains(&top) {
            return Err(SavesError::Corrupt);
        }
        let target = staging.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(io_error("unpack the backup"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(io_error("unpack the backup"))?;
        }
        let mut out = File::create(&target).map_err(io_error("unpack the backup"))?;
        // A failed checksum surfaces as an `InvalidData` error at the end of the entry.
        io::copy(&mut entry, &mut out).map_err(|source| match source.kind() {
            io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => SavesError::Corrupt,
            _ => SavesError::Io {
                context: "unpack the backup",
                source,
            },
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rig {
        _dir: tempfile::TempDir,
        copy: PathBuf,
        saves: SaveData,
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("copy");
        write(&copy.join("_sd/slot0.sav"), "SAVE-1");
        write(&copy.join("_sd/deep/er/slot1.sav"), "SAVE-2");
        write(&copy.join("_sd_mem/u1/mem.bin"), "MEM");
        write(&copy.join("_trophy/trophy00.ucp"), "TROPHY");
        write(&copy.join("app0/not-a-save"), "x");
        let saves = SaveData::new(copy.clone(), dir.path().join("Backups/PPSA1"));
        Rig {
            _dir: dir,
            copy,
            saves,
        }
    }

    fn read(rig: &Rig, relative: &str) -> Option<String> {
        fs::read_to_string(rig.copy.join(relative)).ok()
    }

    #[test]
    fn backup_and_restore_round_trip() {
        let rig = rig();
        let backup = rig.saves.backup(Reason::Manual).unwrap();
        assert_eq!(backup.reason, Reason::Manual);
        assert!(backup.bytes > 0);
        // The game moves on: a save changes, one is new, the trophies are gone.
        write(&rig.copy.join("_sd/slot0.sav"), "SAVE-NEWER");
        write(&rig.copy.join("_sd/slot9.sav"), "NEW");
        fs::remove_dir_all(rig.copy.join("_trophy")).unwrap();
        let safety = rig.saves.restore(&backup, false).unwrap().unwrap();
        assert_eq!(safety.reason, Reason::BeforeRestore);
        assert_eq!(read(&rig, "_sd/slot0.sav").as_deref(), Some("SAVE-1"));
        assert_eq!(
            read(&rig, "_sd/deep/er/slot1.sav").as_deref(),
            Some("SAVE-2")
        );
        assert_eq!(read(&rig, "_sd_mem/u1/mem.bin").as_deref(), Some("MEM"));
        assert_eq!(
            read(&rig, "_trophy/trophy00.ucp").as_deref(),
            Some("TROPHY")
        );
        // What was not in the backup is gone; the game's other files are untouched.
        assert!(read(&rig, "_sd/slot9.sav").is_none());
        assert_eq!(read(&rig, "app0/not-a-save").as_deref(), Some("x"));
        assert!(!rig.copy.join(STAGING).exists());
        // The restore can be undone.
        rig.saves.restore(&safety, false).unwrap();
        assert_eq!(read(&rig, "_sd/slot0.sav").as_deref(), Some("SAVE-NEWER"));
        assert_eq!(read(&rig, "_sd/slot9.sav").as_deref(), Some("NEW"));
        assert!(read(&rig, "_trophy/trophy00.ucp").is_none());
    }

    #[test]
    fn restore_refuses_while_the_game_runs_and_changes_nothing() {
        let rig = rig();
        let backup = rig.saves.backup(Reason::Manual).unwrap();
        write(&rig.copy.join("_sd/slot0.sav"), "LIVE");
        assert!(matches!(
            rig.saves.restore(&backup, true),
            Err(SavesError::GameRunning)
        ));
        assert_eq!(read(&rig, "_sd/slot0.sav").as_deref(), Some("LIVE"));
        assert_eq!(rig.saves.list().len(), 1, "no safety copy for a refusal");
    }

    #[test]
    fn only_the_newest_backups_are_kept_and_play_backups_go_first() {
        let rig = rig();
        let saves = rig.saves.clone().keeping(3);
        let kept = saves.backup_at(Reason::BeforeRestore, 1_000).unwrap();
        for second in 1..=5 {
            saves
                .backup_at(Reason::AfterPlaying, 2_000 + second)
                .unwrap();
        }
        saves.backup_at(Reason::Manual, 3_000).unwrap();
        let listed = saves.list();
        assert_eq!(listed.len(), 3);
        // The old restore copy outlives the older play backups.
        assert!(listed.iter().any(|b| b.path == kept.path));
        assert_eq!(listed[0].reason, Reason::Manual);
        assert_eq!(listed[1].created, 2_005);
        // With nothing but manual copies, the oldest goes.
        let saves = rig.saves.clone().keeping(2);
        saves.backup_at(Reason::Manual, 4_000).unwrap();
        saves.backup_at(Reason::Manual, 5_000).unwrap();
        let times: Vec<i64> = saves.list().iter().map(|b| b.created).collect();
        assert_eq!(times, [5_000, 4_000]);
    }

    #[test]
    fn a_game_without_saves_has_nothing_to_back_up() {
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("copy");
        fs::create_dir_all(copy.join("_sd/empty")).unwrap();
        write(&copy.join("app0/x"), "x");
        let saves = SaveData::new(copy, dir.path().join("b"));
        assert!(!saves.exists());
        assert!(matches!(
            saves.backup(Reason::Manual),
            Err(SavesError::NothingToSave)
        ));
        assert!(matches!(
            saves.backup_unless_recent(Reason::AfterPlaying, 0),
            Ok(None)
        ));
        assert!(saves.list().is_empty());
        assert!(!dir.path().join("b").exists());
    }

    #[test]
    fn a_missing_sd_folder_is_fine_when_another_save_folder_exists() {
        let rig = rig();
        fs::remove_dir_all(rig.copy.join("_sd")).unwrap();
        let backup = rig.saves.backup(Reason::Manual).unwrap();
        fs::remove_dir_all(rig.copy.join("_sd_mem")).unwrap();
        rig.saves.restore(&backup, false).unwrap();
        assert!(!rig.copy.join("_sd").exists());
        assert_eq!(read(&rig, "_sd_mem/u1/mem.bin").as_deref(), Some("MEM"));
    }

    #[test]
    fn a_damaged_zip_is_refused_before_anything_changes() {
        let rig = rig();
        let backup = rig.saves.backup(Reason::Manual).unwrap();
        // Garbage, and a zip cut in half.
        let bytes = fs::read(&backup.path).unwrap();
        for damaged in [b"not a zip".to_vec(), bytes[..bytes.len() / 2].to_vec()] {
            fs::write(&backup.path, damaged).unwrap();
            write(&rig.copy.join("_sd/slot0.sav"), "LIVE");
            assert!(matches!(
                rig.saves.restore(&backup, false),
                Err(SavesError::Corrupt)
            ));
            assert_eq!(read(&rig, "_sd/slot0.sav").as_deref(), Some("LIVE"));
            assert!(!rig.copy.join(STAGING).exists());
        }
        assert_eq!(rig.saves.list().len(), 1, "no safety copy for a refusal");
    }

    #[test]
    fn a_flipped_byte_fails_the_checksum() {
        let rig = rig();
        // Stored, not deflated, so the damage lands in file data and not in the compressor.
        let path = rig.saves.backups.join(file_name(10, Reason::Manual));
        fs::create_dir_all(&rig.saves.backups).unwrap();
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("_sd/slot0.sav", stored).unwrap();
        io::Write::write_all(&mut zip, b"AAAAAAAA").unwrap();
        zip.finish().unwrap();
        let mut bytes = fs::read(&path).unwrap();
        let at = bytes.windows(8).position(|w| w == b"AAAAAAAA").unwrap();
        bytes[at + 3] = b'B';
        fs::write(&path, bytes).unwrap();
        let backup = rig.saves.list().remove(0);
        assert!(matches!(
            rig.saves.restore(&backup, false),
            Err(SavesError::Corrupt)
        ));
        assert_eq!(read(&rig, "_sd/slot0.sav").as_deref(), Some("SAVE-1"));
    }

    #[test]
    fn a_zip_with_other_folders_or_escaping_paths_is_refused() {
        let rig = rig();
        fs::create_dir_all(&rig.saves.backups).unwrap();
        for (stamp, entry) in [(11, "app0/eboot.exe"), (12, "../escape.txt")] {
            let path = rig.saves.backups.join(file_name(stamp, Reason::Manual));
            let mut zip = ZipWriter::new(File::create(&path).unwrap());
            zip.start_file(entry, SimpleFileOptions::default()).unwrap();
            zip.finish().unwrap();
        }
        for backup in rig.saves.list() {
            assert!(matches!(
                rig.saves.restore(&backup, false),
                Err(SavesError::Corrupt)
            ));
        }
        assert_eq!(read(&rig, "_sd/slot0.sav").as_deref(), Some("SAVE-1"));
        assert!(!rig.copy.parent().unwrap().join("escape.txt").exists());
    }

    #[test]
    fn backups_in_the_same_second_do_not_overwrite_each_other() {
        let rig = rig();
        let a = rig.saves.backup_at(Reason::Manual, 7_000).unwrap();
        let b = rig.saves.backup_at(Reason::Manual, 7_000).unwrap();
        assert_ne!(a.path, b.path);
        assert_eq!(rig.saves.list().len(), 2);
    }

    #[test]
    fn a_recent_backup_holds_back_the_one_after_playing() {
        let rig = rig();
        rig.saves.backup(Reason::Manual).unwrap();
        assert!(
            rig.saves
                .backup_unless_recent(Reason::AfterPlaying, AFTER_PLAYING_EVERY)
                .unwrap()
                .is_none()
        );
        let made = rig
            .saves
            .backup_unless_recent(Reason::AfterPlaying, 0)
            .unwrap();
        assert_eq!(made.unwrap().reason, Reason::AfterPlaying);
    }

    #[test]
    fn names_round_trip_and_strangers_are_not_backups() {
        for reason in [
            Reason::Manual,
            Reason::BeforeUnlock,
            Reason::BeforeRestore,
            Reason::BeforeUninstall,
            Reason::AfterPlaying,
        ] {
            assert_eq!(
                parse_name(&file_name(86_400, reason)),
                Some((86_400, reason))
            );
        }
        let name = file_name(1_792_677_900, Reason::BeforeUnlock);
        assert_eq!(name, "20261022T140500Z-unlock.zip");
        assert_eq!(
            parse_name(&name),
            Some((1_792_677_900, Reason::BeforeUnlock))
        );
        for stranger in [
            "notes.txt",
            "20261022T140500Z-manual.zip.partial",
            "20261022T140500Z-other.zip",
            "20261340T140500Z-manual.zip",
            "2026-manual.zip",
        ] {
            assert_eq!(parse_name(stranger), None, "{stranger}");
        }
        let rig = rig();
        fs::create_dir_all(&rig.saves.backups).unwrap();
        fs::write(rig.saves.backups.join("readme.txt"), "x").unwrap();
        assert!(rig.saves.list().is_empty());
    }
}
