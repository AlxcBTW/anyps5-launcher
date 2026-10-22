//! What a game's own files say about its controls (docs/DESIGN.md 5.17). Unreal Engine games keep
//! their input bindings in `<Project>/Config/DefaultInput.ini` inside a `.pak`: every action with
//! the gamepad key it uses and, when the developers kept them, the PC keys too. Pairing the two
//! gives a keyboard layout in the game's own words ("Jump: ✕ and Space").
//!
//! Games on Unreal's Enhanced Input bind keys in Input Mapping Context assets instead; those are
//! read from the IoStore containers next to the paks (`iostore`), the gamepad and keyboard
//! contexts paired through the Input Action both name.
//!
//! Only unencrypted pak indexes are read (versions 8 to 11); entries may be stored raw, with
//! Oodle (`oozextract`) or with Zlib.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::controls::{Changes, Controls, PadAction, scancode_of};

mod iostore;

/// What reading a pak can run into.
#[derive(Debug, thiserror::Error)]
pub enum PakError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not an Unreal pak")]
    NotAPak,
    #[error("the pak index is encrypted")]
    Encrypted,
    #[error("unsupported pak version {0}")]
    Version(u32),
    #[error("damaged pak: {0}")]
    Damaged(&'static str),
    #[error("unsupported compression {0}")]
    Compression(String),
}

const MAGIC: [u8; 4] = [0xE1, 0x12, 0x6F, 0x5A];

/// An Unreal Engine `.pak` opened for reading single files.
pub struct Pak {
    file: File,
    methods: Vec<String>,
    encoded: Vec<u8>,
    /// Full path (mount point removed) to the entry's place in `encoded`.
    entries: BTreeMap<String, usize>,
}

/// A little-endian reader over a byte slice that reports running out instead of panicking.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], at: usize) -> Self {
        Self { bytes, at }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], PakError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(PakError::Damaged("overflow"))?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or(PakError::Damaged("short read"))?;
        self.at = end;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32, PakError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().unwrap_or_default(),
        ))
    }

    fn i32(&mut self) -> Result<i32, PakError> {
        Ok(self.u32()? as i32)
    }

    fn u64(&mut self) -> Result<u64, PakError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().unwrap_or_default(),
        ))
    }

    /// An `FString`: length with the terminator, positive for Latin-1, negative for UTF-16.
    fn string(&mut self) -> Result<String, PakError> {
        let n = self.i32()?;
        if n == 0 {
            return Ok(String::new());
        }
        if n > 0 {
            let raw = self.take(n as usize)?;
            Ok(raw[..raw.len() - 1].iter().map(|&b| b as char).collect())
        } else {
            let raw = self.take(n.unsigned_abs() as usize * 2)?;
            let units: Vec<u16> = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            Ok(String::from_utf16_lossy(&units[..units.len() - 1]))
        }
    }
}

/// An entry as the encoded index describes it.
struct Entry {
    offset: u64,
    size: u64,
    uncompressed: usize,
    method: usize,
    block_size: usize,
    blocks: Vec<u64>,
}

impl Pak {
    pub fn open(path: &Path) -> Result<Self, PakError> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        let tail_length = length.min(1024);
        let tail = read_at(&mut file, length - tail_length, tail_length as usize)?;
        let magic = tail
            .windows(4)
            .rposition(|w| w == MAGIC)
            .ok_or(PakError::NotAPak)?;
        let mut footer = Cursor::new(&tail, magic + 4);
        let version = footer.u32()?;
        if !(8..=11).contains(&version) {
            return Err(PakError::Version(version));
        }
        if magic == 0 || tail[magic - 1] != 0 {
            return Err(PakError::Encrypted);
        }
        let index_offset = footer.u64()?;
        let index_size = footer.u64()? as usize;
        footer.take(20)?;
        let methods = tail
            .get(footer.at..footer.at + 160)
            .unwrap_or_default()
            .chunks(32)
            .map(|c| {
                String::from_utf8_lossy(c)
                    .trim_end_matches('\0')
                    .to_string()
            })
            .collect();

        let index = read_at(&mut file, index_offset, index_size)?;
        let mut cursor = Cursor::new(&index, 0);
        let mount = cursor.string()?;
        let _count = cursor.i32()?;
        let _seed = cursor.u64()?;
        if cursor.u32()? != 0 {
            cursor.take(8 + 8 + 20)?;
        }
        if cursor.u32()? == 0 {
            return Err(PakError::Damaged("no full directory index"));
        }
        let directory_offset = cursor.u64()?;
        let directory_size = cursor.u64()? as usize;
        cursor.take(20)?;
        let encoded_size = cursor.i32()? as usize;
        let encoded = cursor.take(encoded_size)?.to_vec();

        let directory = read_at(&mut file, directory_offset, directory_size)?;
        let mut cursor = Cursor::new(&directory, 0);
        let prefix = mount.trim_start_matches("../").to_string();
        let mut entries = BTreeMap::new();
        for _ in 0..cursor.i32()? {
            let folder = cursor.string()?;
            for _ in 0..cursor.i32()? {
                let name = cursor.string()?;
                let location = cursor.i32()?;
                if location >= 0 {
                    let path = format!("{prefix}{folder}{name}");
                    entries.insert(path.trim_start_matches('/').to_string(), location as usize);
                }
            }
        }
        Ok(Self {
            file,
            methods,
            encoded,
            entries,
        })
    }

    /// Every file path in the pak.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    fn entry(&self, at: usize) -> Result<Entry, PakError> {
        let mut cursor = Cursor::new(&self.encoded, at);
        let bits = cursor.u32()?;
        let mut block_size = if bits & 0x3f == 0x3f {
            cursor.u32()? as usize
        } else {
            ((bits & 0x3f) << 11) as usize
        };
        let count = ((bits >> 6) & 0xffff) as usize;
        let encrypted = bits & (1 << 22) != 0;
        let method = ((bits >> 23) & 0x3f) as usize;
        let read = |small: bool, c: &mut Cursor| -> Result<u64, PakError> {
            if small { Ok(c.u32()? as u64) } else { c.u64() }
        };
        let offset = read(bits & (1 << 31) != 0, &mut cursor)?;
        let uncompressed = read(bits & (1 << 30) != 0, &mut cursor)? as usize;
        let size = if method != 0 {
            read(bits & (1 << 29) != 0, &mut cursor)?
        } else {
            uncompressed as u64
        };
        if encrypted {
            return Err(PakError::Encrypted);
        }
        let blocks = if count == 1 {
            vec![size]
        } else {
            (0..count)
                .map(|_| cursor.u32().map(u64::from))
                .collect::<Result<_, _>>()?
        };
        if block_size == 0 {
            block_size = uncompressed;
        }
        Ok(Entry {
            offset,
            size,
            uncompressed,
            method,
            block_size,
            blocks,
        })
    }

    /// The bytes of one file, or `None` when the pak has no such path (case-insensitive).
    pub fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>, PakError> {
        let Some(at) = self
            .entries
            .iter()
            .find(|(p, _)| p.eq_ignore_ascii_case(path))
            .map(|(_, at)| *at)
        else {
            return Ok(None);
        };
        let entry = self.entry(at)?;
        // The serialised entry header in front of the data (versions 8 to 11).
        let header = 8
            + 8
            + 8
            + 4
            + 20
            + if entry.method != 0 {
                4 + 16 * entry.blocks.len() as u64
            } else {
                0
            }
            + 1
            + 4;
        let start = entry.offset + header;
        if entry.method == 0 {
            return Ok(Some(read_at(&mut self.file, start, entry.uncompressed)?));
        }
        let method = self
            .methods
            .get(entry.method - 1)
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::with_capacity(entry.uncompressed);
        let mut position = start;
        for &compressed in &entry.blocks {
            let raw = read_at(&mut self.file, position, compressed as usize)?;
            position += compressed;
            let want = (entry.uncompressed - out.len()).min(entry.block_size);
            let mut block = vec![0u8; want];
            let written = match method.to_ascii_lowercase().as_str() {
                "oodle" => oozextract::Extractor::new()
                    .read_from_slice(&raw, &mut block)
                    .map_err(|_| PakError::Damaged("oodle block"))?,
                "zlib" => {
                    let mut decoder = flate2::read::ZlibDecoder::new(raw.as_slice());
                    decoder.read_exact(&mut block)?;
                    want
                }
                other => return Err(PakError::Compression(other.to_string())),
            };
            out.extend_from_slice(&block[..written]);
        }
        if out.len() != entry.uncompressed || entry.size == 0 && entry.uncompressed > 0 {
            return Err(PakError::Damaged("size mismatch"));
        }
        Ok(Some(out))
    }
}

fn read_at(file: &mut File, at: u64, length: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = vec![0; length];
    file.seek(SeekFrom::Start(at))?;
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

// ---------------------------------------------------------------------------------------------
// Input bindings
// ---------------------------------------------------------------------------------------------

/// One action of the game with what triggers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameAction {
    /// The game's own name, e.g. "Jump" or "MoveForward".
    pub name: String,
    /// The PS5 buttons it uses.
    pub pad: Vec<PadAction>,
    /// Its PC keys as launcher sources (`KEY:…`, `MOUSE:…`, `WHEEL:…`).
    pub keys: Vec<String>,
}

/// What the game's files told us.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameInput {
    /// Actions and axes from `DefaultInput.ini` or from Enhanced Input mapping contexts.
    Bindings {
        actions: Vec<GameAction>,
        /// The pak, or the folder of the containers, the bindings came from.
        source: PathBuf,
    },
    /// The game binds its input inside assets (Enhanced Input) this reader cannot decode.
    InAssets,
    /// Unreal paks we cannot read (encrypted index).
    Encrypted,
    /// No Unreal input settings found.
    Unknown,
}

/// Looks through the dump's `.pak` files for the project's `Config/DefaultInput.ini` and, for
/// an Enhanced Input game, through the IoStore containers beside them.
pub fn read_game_input(dump: &Path) -> GameInput {
    let mut paks = Vec::new();
    collect_paks(dump, 0, &mut paks);
    paks.sort();
    let mut encrypted = false;
    for path in &paks {
        let mut pak = match Pak::open(path) {
            Ok(pak) => pak,
            Err(PakError::Encrypted) => {
                encrypted = true;
                continue;
            }
            Err(err) => {
                tracing::debug!("{}: {err}", path.display());
                continue;
            }
        };
        let ini = pak
            .files()
            .find(|p| {
                let lower = p.to_ascii_lowercase();
                lower.ends_with("/config/defaultinput.ini") && !lower.starts_with("engine/")
            })
            .map(str::to_owned);
        let Some(ini) = ini else { continue };
        match pak.read(&ini) {
            Ok(Some(bytes)) => {
                let text = String::from_utf8_lossy(&bytes);
                let actions = parse_input_ini(&text);
                if actions.is_empty() && uses_enhanced_input(&text) {
                    return read_mapping_contexts(&paks);
                }
                return GameInput::Bindings {
                    actions,
                    source: path.clone(),
                };
            }
            Ok(None) => {}
            Err(PakError::Encrypted) => encrypted = true,
            Err(err) => tracing::debug!("{}: {ini}: {err}", path.display()),
        }
    }
    if encrypted {
        GameInput::Encrypted
    } else {
        GameInput::Unknown
    }
}

/// The Enhanced Input bindings of the containers in the paks' folders.
fn read_mapping_contexts(paks: &[PathBuf]) -> GameInput {
    let mut folders: Vec<&Path> = paks.iter().filter_map(|p| p.parent()).collect();
    folders.dedup();
    for folder in folders {
        match iostore::read_iostore_mappings(folder) {
            Ok(contexts) => {
                let actions = actions_from_contexts(&contexts);
                if !actions.is_empty() {
                    return GameInput::Bindings {
                        actions,
                        source: folder.to_path_buf(),
                    };
                }
            }
            Err(err) => tracing::debug!("{}: {err}", folder.display()),
        }
    }
    GameInput::InAssets
}

/// `.pak` files under the dump, a few levels deep, outside the launcher's own store.
fn collect_paks(folder: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 5 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if path.is_dir() {
            if !name.starts_with('.') && name != "sce_sys" {
                collect_paks(&path, depth + 1, out);
            }
        } else if name.ends_with(".pak") {
            out.push(path);
        }
    }
}

fn uses_enhanced_input(ini: &str) -> bool {
    ini.contains("EnhancedInput")
}

/// The value of `Field=` inside one `(…)` line, without quotes.
fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let pattern = format!("{name}=");
    let start = line
        .match_indices(&pattern)
        .find(|(i, _)| *i == 0 || matches!(line.as_bytes()[i - 1], b'(' | b',' | b' '))?
        .0
        + pattern.len();
    let rest = &line[start..];
    let value = if let Some(quoted) = rest.strip_prefix('"') {
        &quoted[..quoted.find('"')?]
    } else {
        &rest[..rest.find([',', ')']).unwrap_or(rest.len())]
    };
    Some(value.trim())
}

/// The actions of a `DefaultInput.ini`: `+ActionMappings` and `+AxisMappings` lines (and plain
/// ones), with removals (`-…`) ignored. Actions keep the order of their first line.
pub fn parse_input_ini(ini: &str) -> Vec<GameAction> {
    let mut actions: Vec<GameAction> = Vec::new();
    let mut add = |name: &str, pad: Option<PadAction>, key: Option<String>| {
        let index = match actions.iter().position(|a| a.name == name) {
            Some(i) => i,
            None => {
                actions.push(GameAction {
                    name: name.to_string(),
                    pad: Vec::new(),
                    keys: Vec::new(),
                });
                actions.len() - 1
            }
        };
        let action = &mut actions[index];
        if let Some(pad) = pad
            && !action.pad.contains(&pad)
        {
            action.pad.push(pad);
        }
        if let Some(key) = key
            && !action.keys.contains(&key)
        {
            action.keys.push(key);
        }
    };
    // Axes are split into one action per direction, so "MoveForward" with W (+1) and S (-1) on
    // the left stick's Y axis becomes "MoveForward +" (stick up, W) and "MoveForward -".
    let mut axes: Vec<(String, f32, String)> = Vec::new();
    for raw in ini.lines() {
        let line = raw.trim();
        let line = line.strip_prefix('+').unwrap_or(line);
        if let Some(body) = line.strip_prefix("ActionMappings=") {
            let (Some(name), Some(key)) = (field(body, "ActionName"), field(body, "Key")) else {
                continue;
            };
            let modified = ["bShift", "bCtrl", "bAlt", "bCmd"]
                .iter()
                .any(|m| field(body, m).is_some_and(|v| v.eq_ignore_ascii_case("true")));
            if let Some(pad) = pad_button(key) {
                add(name, Some(pad), None);
            } else if !modified && let Some(source) = launcher_source(key) {
                add(name, None, Some(source));
            }
        } else if let Some(body) = line.strip_prefix("AxisMappings=") {
            let (Some(name), Some(key)) = (field(body, "AxisName"), field(body, "Key")) else {
                continue;
            };
            let scale: f32 = field(body, "Scale")
                .and_then(|s| s.trim_end_matches('f').parse().ok())
                .unwrap_or(1.0);
            axes.push((name.to_string(), scale, key.to_string()));
        }
    }
    for (name, scale, key) in &axes {
        let sign = if *scale < 0.0 { '-' } else { '+' };
        if let Some((positive, negative)) = pad_axis(key) {
            // The stick direction for this axis's positive and negative halves.
            let (up, down) = if *scale < 0.0 {
                (negative, positive)
            } else {
                (positive, negative)
            };
            add(&format!("{name} +"), Some(up), None);
            add(&format!("{name} -"), Some(down), None);
        } else if let Some(pad) = pad_button(key) {
            add(&format!("{name} {sign}"), Some(pad), None);
        } else if let Some(source) = launcher_source(key) {
            add(&format!("{name} {sign}"), None, Some(source));
        }
    }
    actions
}

/// The PS5 button an Unreal gamepad key stands for.
fn pad_button(key: &str) -> Option<PadAction> {
    use PadAction::*;
    Some(match key {
        "Gamepad_FaceButton_Bottom" => Cross,
        "Gamepad_FaceButton_Right" => Circle,
        "Gamepad_FaceButton_Left" => Square,
        "Gamepad_FaceButton_Top" => Triangle,
        "Gamepad_LeftShoulder" => L1,
        "Gamepad_RightShoulder" => R1,
        "Gamepad_LeftTrigger" | "Gamepad_LeftTriggerAxis" => L2,
        "Gamepad_RightTrigger" | "Gamepad_RightTriggerAxis" => R2,
        "Gamepad_LeftThumbstick" => L3,
        "Gamepad_RightThumbstick" => R3,
        "Gamepad_Special_Right" => Options,
        "Gamepad_Special_Left" => TouchLeft,
        "Gamepad_DPad_Up" => Up,
        "Gamepad_DPad_Down" => Down,
        "Gamepad_DPad_Left" => Left,
        "Gamepad_DPad_Right" => Right,
        "Gamepad_LeftStick_Up" => LeftStickUp,
        "Gamepad_LeftStick_Down" => LeftStickDown,
        "Gamepad_LeftStick_Left" => LeftStickLeft,
        "Gamepad_LeftStick_Right" => LeftStickRight,
        "Gamepad_RightStick_Up" => RightStickUp,
        "Gamepad_RightStick_Down" => RightStickDown,
        "Gamepad_RightStick_Left" => RightStickLeft,
        "Gamepad_RightStick_Right" => RightStickRight,
        _ => return None,
    })
}

/// A stick axis as its (positive, negative) directions. Unreal's Y axes point up.
fn pad_axis(key: &str) -> Option<(PadAction, PadAction)> {
    use PadAction::*;
    Some(match key {
        "Gamepad_LeftX" => (LeftStickRight, LeftStickLeft),
        "Gamepad_LeftY" => (LeftStickUp, LeftStickDown),
        "Gamepad_RightX" => (RightStickRight, RightStickLeft),
        "Gamepad_RightY" => (RightStickUp, RightStickDown),
        _ => return None,
    })
}

/// An Unreal keyboard or mouse key as a launcher source, `None` for keys a binding cannot use.
fn launcher_source(key: &str) -> Option<String> {
    let mouse = match key {
        "LeftMouseButton" => Some("MOUSE:Left"),
        "RightMouseButton" => Some("MOUSE:Right"),
        "MiddleMouseButton" => Some("MOUSE:Middle"),
        "ThumbMouseButton" => Some("MOUSE:X1"),
        "ThumbMouseButton2" => Some("MOUSE:X2"),
        "MouseScrollUp" => Some("WHEEL:Up"),
        "MouseScrollDown" => Some("WHEEL:Down"),
        _ => None,
    };
    if let Some(source) = mouse {
        return Some(source.to_string());
    }
    let digits = [
        "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
    ];
    let name = match key {
        "SpaceBar" => "Space".to_string(),
        "Enter" => "Return".to_string(),
        "BackSpace" => "Backspace".to_string(),
        "LeftShift" => "Left Shift".to_string(),
        "RightShift" => "Right Shift".to_string(),
        "LeftControl" => "Left Ctrl".to_string(),
        "RightControl" => "Right Ctrl".to_string(),
        "LeftAlt" => "Left Alt".to_string(),
        "RightAlt" => "Right Alt".to_string(),
        "CapsLock" => "CapsLock".to_string(),
        "Comma" => ",".to_string(),
        "Period" => ".".to_string(),
        "Slash" => "/".to_string(),
        "Semicolon" => ";".to_string(),
        "Hyphen" | "Underscore" => "-".to_string(),
        "Equals" => "=".to_string(),
        "LeftBracket" => "[".to_string(),
        "RightBracket" => "]".to_string(),
        "Backslash" => "\\".to_string(),
        "Apostrophe" => "'".to_string(),
        "Tilde" => "`".to_string(),
        "PageUp" | "PageDown" | "Home" | "End" | "Insert" | "Delete" | "Escape" | "Tab" | "Up"
        | "Down" | "Left" | "Right" => key.to_string(),
        other => {
            if let Some(digit) = digits.iter().position(|d| *d == other) {
                digit.to_string()
            } else if other.len() == 1 && other.chars().all(|c| c.is_ascii_uppercase())
                || other.starts_with('F') && other[1..].parse::<u8>().is_ok_and(|n| n <= 12)
            {
                other.to_string()
            } else {
                return None;
            }
        }
    };
    scancode_of(&name).map(|_| format!("KEY:{name}"))
}

/// The layout the game's own PC keys give, on top of `changes`. Each key the game names moves to
/// the PS5 button of its action (the first action naming a key wins, at most three per button)
/// and leaves every other button; a button keeps its other keys after the game's. Two things
/// stay put: the keys that walk (the left stick's) unless the game itself puts keys on the left
/// stick, and the keys of an action on several stick directions (a 2D axis, whose keys cannot be
/// told apart by direction). Returns the new changes and how many buttons got a key of the game.
pub fn game_layout(actions: &[GameAction], changes: &Changes) -> (Changes, usize) {
    use PadAction::*;
    let walks = |a: PadAction| {
        matches!(
            a,
            LeftStickUp | LeftStickDown | LeftStickLeft | LeftStickRight
        )
    };
    let stick = |a: PadAction| {
        walks(a)
            || matches!(
                a,
                RightStickUp | RightStickDown | RightStickLeft | RightStickRight
            )
    };
    let moved = |a: &&GameAction| !(a.pad.len() > 1 && a.pad.iter().all(|p| stick(*p)));
    let game_walks = actions
        .iter()
        .filter(moved)
        .any(|a| !a.keys.is_empty() && a.pad.iter().any(|p| walks(*p)));
    let kept: Vec<String> = if game_walks {
        Vec::new()
    } else {
        PadAction::ALL
            .into_iter()
            .filter(|a| walks(*a))
            .flat_map(|a| Controls::sources(a, changes))
            .collect()
    };
    let mut wanted: Vec<(PadAction, Vec<String>)> = Vec::new();
    let mut claimed: Vec<String> = Vec::new();
    for action in actions.iter().filter(moved) {
        for &pad in &action.pad {
            for key in &action.keys {
                if claimed.contains(key) || kept.contains(key) {
                    continue;
                }
                let index = match wanted.iter().position(|(p, _)| *p == pad) {
                    Some(index) => index,
                    None => {
                        wanted.push((pad, Vec::new()));
                        wanted.len() - 1
                    }
                };
                if wanted[index].1.len() < 3 {
                    wanted[index].1.push(key.clone());
                    claimed.push(key.clone());
                }
            }
        }
    }
    let mut result = changes.clone();
    for action in PadAction::ALL {
        let current = Controls::sources(action, changes);
        let mut next: Vec<String> = wanted
            .iter()
            .find(|(p, _)| *p == action)
            .map(|(_, keys)| keys.clone())
            .unwrap_or_default();
        next.extend(current.iter().filter(|s| !claimed.contains(s)).cloned());
        if next != current {
            result.insert(action.name().to_string(), next);
        }
    }
    result.retain(|name, value| {
        PadAction::from_name(name).is_some_and(|a| Controls::default_sources(a) != value.as_slice())
    });
    (result, wanted.len())
}

/// Enhanced Input mapping contexts as actions: one per Input Action, named without its `IA_` and
/// device prefixes, so a gamepad context's `IA_Gamepad_Interact` and a keyboard one's
/// `IA_Interact` meet. Contexts of other platforms and of tests are left out.
fn actions_from_contexts(contexts: &[iostore::MappingContext]) -> Vec<GameAction> {
    const LEFT_OUT: [&str; 5] = ["switch", "joycon", "test", "debug", "cheat"];
    let mut actions: Vec<GameAction> = Vec::new();
    for context in contexts {
        let lower = context.name.to_ascii_lowercase();
        if LEFT_OUT.iter().any(|word| lower.contains(word)) {
            continue;
        }
        for (asset, key) in &context.mappings {
            let pad = pad_button(key)
                .map(|p| vec![p])
                .or_else(|| pad_stick(key))
                .unwrap_or_default();
            let source = launcher_source(key);
            if pad.is_empty() && source.is_none() {
                continue;
            }
            let name = action_name(asset);
            let index = match actions.iter().position(|a| a.name == name) {
                Some(index) => index,
                None => {
                    actions.push(GameAction {
                        name,
                        pad: Vec::new(),
                        keys: Vec::new(),
                    });
                    actions.len() - 1
                }
            };
            let action = &mut actions[index];
            for p in pad {
                if !action.pad.contains(&p) {
                    action.pad.push(p);
                }
            }
            if let Some(source) = source
                && !action.keys.contains(&source)
            {
                action.keys.push(source);
            }
        }
    }
    actions
}

/// `IA_Gamepad_MeleeAttack` → `MeleeAttack`, `IA_OpenUI_WorldMap` → `WorldMap`.
fn action_name(asset: &str) -> String {
    let mut name = asset.strip_prefix("IA_").unwrap_or(asset);
    for prefix in ["Gamepad_", "KBM_", "Keyboard_", "Mouse_", "PC_", "OpenUI_"] {
        name = name.strip_prefix(prefix).unwrap_or(name);
    }
    name.to_string()
}

/// The stick directions of an Enhanced Input 2D or 1D stick key.
fn pad_stick(key: &str) -> Option<Vec<PadAction>> {
    use PadAction::*;
    match key {
        "Gamepad_Left2D" => Some(vec![
            LeftStickUp,
            LeftStickDown,
            LeftStickLeft,
            LeftStickRight,
        ]),
        "Gamepad_Right2D" => Some(vec![
            RightStickUp,
            RightStickDown,
            RightStickLeft,
            RightStickRight,
        ]),
        _ => pad_axis(key).map(|(positive, negative)| vec![positive, negative]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The input mapping contexts of Minecraft Dungeons II (PPSA16064, 01.100.100), as the IoStore
    /// reader lists them: `== <context> (<package>)`, then `   <input action> -> <key>`.
    fn md2_contexts() -> Vec<iostore::MappingContext> {
        let mut contexts: Vec<iostore::MappingContext> = Vec::new();
        for line in include_str!("../tests/data/md2-mappings.txt").lines() {
            if let Some(head) = line.strip_prefix("== ") {
                let (name, package) = head.split_once(" (").unwrap();
                contexts.push(iostore::MappingContext {
                    name: name.to_string(),
                    package: package.trim_end_matches(')').to_string(),
                    mappings: Vec::new(),
                });
            } else if let Some((action, key)) = line.trim().split_once(" -> ") {
                contexts
                    .last_mut()
                    .unwrap()
                    .mappings
                    .push((action.to_string(), key.to_string()));
            }
        }
        contexts
    }

    /// `community/layouts/PPSA16064/from-the-game.ini` is what "From the Game" makes of the game's
    /// own keys, exported. Regenerate it with `WRITE_COMMUNITY_LAYOUT=1 cargo test -p anyps5-core
    /// the_community_layout_of_minecraft_dungeons_ii`.
    #[test]
    fn the_community_layout_of_minecraft_dungeons_ii_is_the_games_own_keys() {
        let actions = actions_from_contexts(&md2_contexts());
        let (changes, buttons) = game_layout(&actions, &Changes::new());
        assert!(buttons >= 10, "{buttons}");
        let text = Controls::export(
            &changes,
            &crate::controls::InputTuning::default(),
            "Minecraft Dungeons II (PPSA16064), the keys the game names itself",
        );
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../community/layouts/PPSA16064/from-the-game.ini"
        );
        if std::env::var_os("WRITE_COMMUNITY_LAYOUT").is_some() {
            std::fs::write(path, &text).unwrap();
        }
        let file = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
        assert_eq!(file, text);
        // And the engine would take it.
        let parsed = Controls::parse(&file).unwrap();
        assert_eq!(parsed.changes, changes);
    }

    const INI: &str = r#"
[/Script/Engine.InputSettings]
+ActionMappings=(ActionName="Jump",bShift=False,bCtrl=False,bAlt=False,bCmd=False,Key=SpaceBar)
+ActionMappings=(ActionName="Jump",bShift=False,bCtrl=False,bAlt=False,bCmd=False,Key=Gamepad_FaceButton_Bottom)
+ActionMappings=(ActionName="Interact",Key=E)
+ActionMappings=(ActionName="Interact",Key=Gamepad_FaceButton_Left)
+ActionMappings=(ActionName="Sprint",bShift=True,Key=W)
+ActionMappings=(ActionName="Sprint",Key=LeftShift)
+ActionMappings=(ActionName="Sprint",Key=Gamepad_LeftThumbstick)
+ActionMappings=(ActionName="Attack",Key=LeftMouseButton)
+ActionMappings=(ActionName="Attack",Key=Gamepad_RightTrigger)
+ActionMappings=(ActionName="Menu",Key=Gamepad_Special_Right)
-ActionMappings=(ActionName="Old",Key=Q)
+AxisMappings=(AxisName="MoveForward",Scale=1.000000,Key=W)
+AxisMappings=(AxisName="MoveForward",Scale=-1.000000,Key=S)
+AxisMappings=(AxisName="MoveForward",Scale=1.000000,Key=Gamepad_LeftY)
+AxisMappings=(AxisName="MoveRight",Scale=1.000000,Key=D)
+AxisMappings=(AxisName="MoveRight",Scale=-1.000000,Key=A)
+AxisMappings=(AxisName="MoveRight",Scale=1.000000,Key=Gamepad_LeftX)
+AxisMappings=(AxisName="Turn",Scale=1.000000,Key=MouseX)
"#;

    #[test]
    fn actions_pair_buttons_with_pc_keys() {
        let actions = parse_input_ini(INI);
        let jump = actions.iter().find(|a| a.name == "Jump").unwrap();
        assert_eq!(jump.pad, vec![PadAction::Cross]);
        assert_eq!(jump.keys, vec!["KEY:Space".to_string()]);
        let sprint = actions.iter().find(|a| a.name == "Sprint").unwrap();
        // The Shift+W chord is skipped: a binding is one key.
        assert_eq!(sprint.keys, vec!["KEY:Left Shift".to_string()]);
        assert_eq!(sprint.pad, vec![PadAction::L3]);
        let attack = actions.iter().find(|a| a.name == "Attack").unwrap();
        assert_eq!(attack.keys, vec!["MOUSE:Left".to_string()]);
        assert!(!actions.iter().any(|a| a.name == "Old"));
        let menu = actions.iter().find(|a| a.name == "Menu").unwrap();
        assert!(menu.keys.is_empty());
    }

    #[test]
    fn axes_split_into_stick_directions() {
        let actions = parse_input_ini(INI);
        let forward = actions.iter().find(|a| a.name == "MoveForward +").unwrap();
        assert_eq!(forward.pad, vec![PadAction::LeftStickUp]);
        assert_eq!(forward.keys, vec!["KEY:W".to_string()]);
        let back = actions.iter().find(|a| a.name == "MoveForward -").unwrap();
        assert_eq!(back.pad, vec![PadAction::LeftStickDown]);
        assert_eq!(back.keys, vec!["KEY:S".to_string()]);
        let left = actions.iter().find(|a| a.name == "MoveRight -").unwrap();
        assert_eq!(left.pad, vec![PadAction::LeftStickLeft]);
        assert_eq!(left.keys, vec!["KEY:A".to_string()]);
        // Mouse axes have no key source.
        assert!(
            !actions
                .iter()
                .any(|a| a.name.starts_with("Turn") && !a.keys.is_empty())
        );
    }

    #[test]
    fn the_game_layout_moves_keys_and_keeps_the_rest() {
        let actions = parse_input_ini(INI);
        let (layout, count) = game_layout(&actions, &Changes::new());
        assert_eq!(count, 8);
        // The game's keys come first; the button keeps the rest of its own.
        assert_eq!(
            Controls::sources(PadAction::Cross, &layout),
            vec![
                "KEY:Space".to_string(),
                "KEY:K".to_string(),
                "KEY:Return".to_string()
            ]
        );
        assert_eq!(
            Controls::sources(PadAction::Square, &layout),
            vec!["KEY:E".to_string(), "KEY:J".to_string()],
            "the left mouse button went to R2"
        );
        assert_eq!(
            Controls::sources(PadAction::R2, &layout),
            vec![
                "MOUSE:Left".to_string(),
                "KEY:O".to_string(),
                "MOUSE:Right".to_string()
            ]
        );
        // E was R1's default key: it is gone there now.
        assert!(Controls::sources(PadAction::R1, &layout).is_empty());
        // Untouched buttons keep their defaults (Triangle: I).
        assert_eq!(
            Controls::sources(PadAction::Triangle, &layout),
            vec!["KEY:I".to_string()]
        );
        // No key ends up on two buttons.
        let mut seen = std::collections::HashSet::new();
        for action in PadAction::ALL {
            for source in Controls::sources(action, &layout) {
                assert!(seen.insert(source.clone()), "{source} twice");
            }
        }
    }

    #[test]
    fn walking_keys_and_2d_axes_stay_put() {
        let actions = vec![
            // A menu on S, as some PC layouts have it, must not take S from walking.
            GameAction {
                name: "Wheel".into(),
                pad: vec![PadAction::Options],
                keys: vec!["KEY:S".into(), "KEY:K".into()],
            },
            // A 2D dodge on the right stick: its keys cannot be told apart by direction.
            GameAction {
                name: "Dodge".into(),
                pad: vec![
                    PadAction::RightStickUp,
                    PadAction::RightStickDown,
                    PadAction::RightStickLeft,
                    PadAction::RightStickRight,
                ],
                keys: vec!["KEY:R".into()],
            },
        ];
        let (layout, count) = game_layout(&actions, &Changes::new());
        assert_eq!(count, 1);
        assert_eq!(
            Controls::sources(PadAction::Options, &layout),
            vec!["KEY:K".to_string(), "KEY:Escape".to_string()]
        );
        assert_eq!(
            Controls::sources(PadAction::LeftStickDown, &layout),
            vec!["KEY:S".to_string()]
        );
        assert_eq!(
            Controls::sources(PadAction::RightStickUp, &layout),
            vec!["KEY:T".to_string()]
        );
        assert_eq!(
            Controls::sources(PadAction::Cross, &layout),
            vec!["KEY:Space".to_string(), "KEY:Return".to_string()],
            "K moved to Options"
        );
    }

    #[test]
    fn mapping_contexts_pair_through_the_input_action() {
        let context = |name: &str, mappings: &[(&str, &str)]| iostore::MappingContext {
            name: name.into(),
            package: format!("Game/Input/{name}.uasset"),
            mappings: mappings
                .iter()
                .map(|(a, k)| ((*a).to_string(), (*k).to_string()))
                .collect(),
        };
        let contexts = [
            context(
                "IMC_DefaultGamepad",
                &[
                    ("IA_Jump", "Gamepad_FaceButton_Bottom"),
                    ("IA_Gamepad_Move", "Gamepad_Left2D"),
                    ("IA_OpenUI_WorldMap", "Gamepad_Special_Left"),
                    ("IA_Gamepad_Interact", "Gamepad_FaceButton_Bottom"),
                ],
            ),
            context(
                "IMC_DefaultKBM",
                &[
                    ("IA_Jump", "SpaceBar"),
                    ("IA_OpenUI_WorldMap", "M"),
                    ("IA_KBM_Interact", "F"),
                ],
            ),
            context(
                "IMC_SwitchGamepad",
                &[("IA_Jump", "Gamepad_FaceButton_Right")],
            ),
        ];
        let actions = actions_from_contexts(&contexts);
        let find = |name: &str| actions.iter().find(|a| a.name == name).unwrap();
        assert_eq!(
            find("Jump").pad,
            vec![PadAction::Cross],
            "Switch is left out"
        );
        assert_eq!(find("Jump").keys, vec!["KEY:Space".to_string()]);
        assert_eq!(find("WorldMap").pad, vec![PadAction::TouchLeft]);
        assert_eq!(find("WorldMap").keys, vec!["KEY:M".to_string()]);
        assert_eq!(find("Interact").keys, vec!["KEY:F".to_string()]);
        assert_eq!(find("Move").pad.len(), 4);
        let (layout, _) = game_layout(&actions, &Changes::new());
        assert_eq!(
            Controls::sources(PadAction::Cross, &layout),
            vec![
                "KEY:Space".to_string(),
                "KEY:F".to_string(),
                "KEY:K".to_string(),
                "KEY:Return".to_string()
            ]
        );
    }

    #[test]
    fn unreal_key_names_map_to_launcher_names() {
        assert_eq!(launcher_source("One").as_deref(), Some("KEY:1"));
        assert_eq!(launcher_source("F5").as_deref(), Some("KEY:F5"));
        assert_eq!(launcher_source("Enter").as_deref(), Some("KEY:Return"));
        assert_eq!(
            launcher_source("LeftControl").as_deref(),
            Some("KEY:Left Ctrl")
        );
        assert_eq!(
            launcher_source("MouseScrollUp").as_deref(),
            Some("WHEEL:Up")
        );
        assert_eq!(launcher_source("F24"), None);
        assert_eq!(launcher_source("Gamepad_FaceButton_Bottom"), None);
        assert_eq!(
            field(r#"(ActionName="A B",Key=X)"#, "ActionName"),
            Some("A B")
        );
        assert_eq!(
            field("(AxisName=Y,Scale=-1.0,Key=S)", "Scale"),
            Some("-1.0")
        );
    }

    /// Writes a minimal version-11 pak with uncompressed files, laid out as Unreal does.
    fn write_pak(path: &Path, mount: &str, files: &[(&str, &str, &[u8])]) {
        fn fstring(out: &mut Vec<u8>, s: &str) {
            out.extend_from_slice(&((s.len() + 1) as i32).to_le_bytes());
            out.extend_from_slice(s.as_bytes());
            out.push(0);
        }
        let mut data = Vec::new();
        let mut encoded = Vec::new();
        let mut locations = Vec::new();
        for (_, _, bytes) in files {
            let offset = data.len() as u32;
            // Entry header: offset, size, uncompressed, method, hash, flags, block size.
            data.extend_from_slice(&0u64.to_le_bytes());
            data.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            data.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            data.extend_from_slice(&0u32.to_le_bytes());
            data.extend_from_slice(&[0u8; 20]);
            data.push(0);
            data.extend_from_slice(&0u32.to_le_bytes());
            data.extend_from_slice(bytes);
            locations.push(encoded.len() as i32);
            let bits: u32 = (1 << 31) | (1 << 30) | (1 << 29);
            encoded.extend_from_slice(&bits.to_le_bytes());
            encoded.extend_from_slice(&offset.to_le_bytes());
            encoded.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        }
        let mut directory = Vec::new();
        directory.extend_from_slice(&(files.len() as i32).to_le_bytes());
        for ((folder, name, _), location) in files.iter().zip(&locations) {
            fstring(&mut directory, folder);
            directory.extend_from_slice(&1i32.to_le_bytes());
            fstring(&mut directory, name);
            directory.extend_from_slice(&location.to_le_bytes());
        }
        let directory_offset = data.len() as u64;
        data.extend_from_slice(&directory);
        let mut index = Vec::new();
        fstring(&mut index, mount);
        index.extend_from_slice(&(files.len() as i32).to_le_bytes());
        index.extend_from_slice(&0u64.to_le_bytes());
        index.extend_from_slice(&0u32.to_le_bytes());
        index.extend_from_slice(&1u32.to_le_bytes());
        index.extend_from_slice(&directory_offset.to_le_bytes());
        index.extend_from_slice(&(directory.len() as u64).to_le_bytes());
        index.extend_from_slice(&[0u8; 20]);
        index.extend_from_slice(&(encoded.len() as i32).to_le_bytes());
        index.extend_from_slice(&encoded);
        index.extend_from_slice(&0i32.to_le_bytes());
        let index_offset = data.len() as u64;
        data.extend_from_slice(&index);
        // Footer: key guid, encrypted flag, magic, version, index offset/size/hash, methods.
        data.extend_from_slice(&[0u8; 16]);
        data.push(0);
        data.extend_from_slice(&MAGIC);
        data.extend_from_slice(&11u32.to_le_bytes());
        data.extend_from_slice(&index_offset.to_le_bytes());
        data.extend_from_slice(&(index.len() as u64).to_le_bytes());
        data.extend_from_slice(&[0u8; 20]);
        let mut methods = [0u8; 160];
        methods[..5].copy_from_slice(b"Oodle");
        data.extend_from_slice(&methods);
        std::fs::write(path, data).unwrap();
    }

    #[test]
    fn a_dump_with_an_unreal_pak_gives_its_bindings() {
        let dir = tempfile::tempdir().unwrap();
        let paks = dir.path().join("Game/Content/Paks");
        std::fs::create_dir_all(&paks).unwrap();
        write_pak(
            &paks.join("pakchunk0-ps5.pak"),
            "../../../",
            &[
                ("Game/Config/", "DefaultInput.ini", INI.as_bytes()),
                ("Engine/Config/", "BaseInput.ini", b"[x]"),
            ],
        );
        let mut pak = Pak::open(&paks.join("pakchunk0-ps5.pak")).unwrap();
        assert!(pak.files().any(|f| f == "Game/Config/DefaultInput.ini"));
        assert_eq!(
            pak.read("game/config/defaultinput.ini").unwrap().unwrap(),
            INI.as_bytes()
        );
        assert_eq!(pak.read("missing.ini").unwrap(), None);
        match read_game_input(dir.path()) {
            GameInput::Bindings { actions, .. } => {
                assert!(actions.iter().any(|a| a.name == "Jump"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn enhanced_input_games_are_recognised() {
        let dir = tempfile::tempdir().unwrap();
        write_pak(
            &dir.path().join("game.pak"),
            "../../../",
            &[(
                "Game/Config/",
                "DefaultInput.ini",
                b"[/Script/EnhancedInput.EnhancedInputDeveloperSettings]\n",
            )],
        );
        assert_eq!(read_game_input(dir.path()), GameInput::InAssets);
        assert_eq!(
            read_game_input(&dir.path().join("nothing")),
            GameInput::Unknown
        );
    }

    /// Against a real dump: `ANYPS5_TEST_DUMP=<dump folder> cargo test -- --ignored real_dump`.
    #[test]
    #[ignore = "needs a real dump"]
    fn real_dump() {
        let dump = std::env::var_os("ANYPS5_TEST_DUMP").expect("ANYPS5_TEST_DUMP");
        let input = read_game_input(Path::new(&dump));
        let GameInput::Bindings { actions, source } = &input else {
            panic!("{input:?}");
        };
        println!("{}", source.display());
        for action in actions {
            println!("{:<28} {:?} {:?}", action.name, action.pad, action.keys);
        }
        let (layout, count) = game_layout(actions, &Changes::new());
        println!("{count} buttons");
        for action in PadAction::ALL {
            println!(
                "{:<16} {:?}",
                action.name(),
                Controls::sources(action, &layout)
            );
        }
    }
}
