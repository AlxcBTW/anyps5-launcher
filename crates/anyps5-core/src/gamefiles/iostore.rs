//! Reads Unreal Enhanced Input key mappings (`IMC_*` Input Mapping Context assets) straight from
//! an IoStore container (`.utoc` / `.ucas`), without a schema file and without running the game.
//!
//! Pipeline: utoc header -> chunk table -> directory index (package file names) -> Oodle/none
//! compressed blocks -> zen package header (name map, import map, imported package names,
//! export map) -> unversioned property decoder for `UInputMappingContext::Mappings`.
//!
//! Verified against Minecraft Dungeons II (UE 5.6, utoc version 8, unversioned zen packages with
//! the 5.6 "cell offsets" in the summary). Only std and `oozextract` are used.
//!
//! Scope and known limits (see the end of this header for what breaks elsewhere):
//! * unencrypted, unsigned containers with a directory index;
//! * zen packages without versioning info (the normal cooked shipping case) and with the UE 5.3+
//!   summary (imported package names inside the header);
//! * the mapping struct layout is the UE 5.6 one (Triggers, Modifiers, Action, Key, ..., settings
//!   enum byte, settings object). Anything else makes the affected IMC fail loudly (it is listed
//!   in the report of [`read_iostore_mappings_report`]) instead of yielding wrong keys.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

type Res<T> = Result<T, String>;

/// One Input Mapping Context asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MappingContext {
    /// Asset name, e.g. `IMC_DefaultKBM`.
    pub(crate) name: String,
    /// Package path as stored in the container, e.g. `Game/Spicewood/.../IMC_DefaultKBM.uasset`.
    pub(crate) package: String,
    /// (Input Action asset name such as `IA_Jump`, Unreal key name such as `SpaceBar`).
    /// Unbound entries (key `None`) are dropped.
    pub(crate) mappings: Vec<(String, String)>,
}

/// Reads all decodable `IMC_*` assets from every `.utoc` in `paks`.
/// A game without IoStore containers yields an empty list. IMCs that cannot be decoded are skipped;
/// use [`read_iostore_mappings_report`] to see why.
pub(crate) fn read_iostore_mappings(paks: &Path) -> Result<Vec<MappingContext>, String> {
    read_iostore_mappings_report(paks).map(|(c, _)| c)
}

/// Like [`read_iostore_mappings`], plus one message per skipped IMC or container.
pub(crate) fn read_iostore_mappings_report(
    paks: &Path,
) -> Result<(Vec<MappingContext>, Vec<String>), String> {
    let mut tocs: Vec<PathBuf> = std::fs::read_dir(paks)
        .map_err(|e| format!("{}: {e}", paks.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("utoc"))
        })
        .collect();
    // Patch containers sort after the base container; later ones override earlier ones.
    tocs.sort();
    let mut by_package: Vec<MappingContext> = Vec::new();
    let mut skipped = Vec::new();
    for utoc in tocs {
        let label = utoc
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut c = match Container::open(&utoc) {
            Ok(c) => c,
            Err(e) => {
                skipped.push(format!("{label}: {e}"));
                continue;
            }
        };
        let imcs: Vec<(String, u32)> = c
            .toc
            .files
            .iter()
            .filter(|(p, _)| {
                p.ends_with(".uasset")
                    && p.rsplit('/').next().is_some_and(|f| f.starts_with("IMC_"))
            })
            .cloned()
            .collect();
        for (path, chunk) in imcs {
            let stem = path
                .rsplit('/')
                .next()
                .unwrap_or("")
                .trim_end_matches(".uasset")
                .to_string();
            let decoded = c
                .read_chunk(chunk as usize)
                .and_then(Zen::parse)
                .and_then(|z| {
                    let ex = z.exports.iter().position(|e| e.name == stem).unwrap_or(0);
                    z.decode_imc(ex)
                });
            match decoded {
                Ok(m) => {
                    let ctx = MappingContext {
                        name: stem,
                        package: path,
                        mappings: m.into_iter().filter(|(_, k)| k != "None").collect(),
                    };
                    match by_package.iter_mut().find(|x| x.package == ctx.package) {
                        Some(slot) => *slot = ctx,
                        None => by_package.push(ctx),
                    }
                }
                Err(e) => skipped.push(format!("{label}: {path}: {e}")),
            }
        }
    }
    by_package.sort_by(|a, b| a.package.cmp(&b.package));
    Ok((by_package, skipped))
}

// ------------------------------------------------------------------ byte helpers

fn rd_u32(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}
fn rd_u64(b: &[u8], p: usize) -> u64 {
    u64::from_le_bytes(b[p..p + 8].try_into().unwrap())
}

// ------------------------------------------------------------------ utoc

struct Toc {
    block_size: u64,
    offs_lens: Vec<(u64, u64)>,
    /// (offset in ucas, compressed size, uncompressed size, method index)
    blocks: Vec<(u64, u32, u32, u8)>,
    methods: Vec<String>,
    /// (path below the mount point, toc entry index)
    files: Vec<(String, u32)>,
    partition_count: u32,
    partition_size: u64,
}

impl Toc {
    fn parse(b: &[u8]) -> Res<Toc> {
        if b.len() < 144 || &b[0..16] != b"-==--==--==--==-" {
            return Err("not a utoc".into());
        }
        let version = b[16];
        if version < 4 {
            return Err(format!("utoc version {version} unsupported (need >= 4)"));
        }
        let n = rd_u32(b, 24) as usize;
        let nblocks = rd_u32(b, 28) as usize;
        if rd_u32(b, 32) != 12 {
            return Err("unexpected compression block entry size".into());
        }
        let nmeth = rd_u32(b, 36) as usize;
        let methlen = rd_u32(b, 40) as usize;
        let block_size = rd_u32(b, 44) as u64;
        let dir_size = rd_u32(b, 48) as usize;
        let partition_count = rd_u32(b, 52).max(1);
        let flags = b[0x50];
        let seeds = rd_u32(b, 0x54) as usize;
        let mut partition_size = rd_u64(b, 0x58);
        if partition_size == 0 {
            partition_size = u64::MAX;
        }
        let overflow = rd_u32(b, 0x60) as usize;
        if flags & 2 != 0 {
            return Err("encrypted container".into());
        }
        if flags & 4 != 0 {
            return Err("signed containers not supported".into());
        }
        let mut p = rd_u32(b, 20) as usize;
        let need = |p: usize, len: usize| {
            if p.checked_add(len).is_some_and(|e| e <= b.len()) {
                Ok(())
            } else {
                Err("truncated utoc".to_string())
            }
        };
        need(p, n * 22)?;
        p += n * 12; // chunk ids are not needed: the directory index carries the entry index
        let be = |s: &[u8]| s.iter().fold(0u64, |a, &x| (a << 8) | x as u64);
        let offs_lens = (0..n)
            .map(|i| {
                let e = &b[p + i * 10..p + i * 10 + 10];
                (be(&e[0..5]), be(&e[5..10]))
            })
            .collect();
        p += n * 10;
        p += (seeds + overflow) * 4;
        need(p, nblocks * 12 + nmeth * methlen)?;
        let blocks = (0..nblocks)
            .map(|i| {
                let e = &b[p + i * 12..p + i * 12 + 12];
                let off =
                    u32::from_le_bytes(e[0..4].try_into().unwrap()) as u64 | ((e[4] as u64) << 32);
                let csz = e[5] as u32 | (e[6] as u32) << 8 | (e[7] as u32) << 16;
                let usz = e[8] as u32 | (e[9] as u32) << 8 | (e[10] as u32) << 16;
                (off, csz, usz, e[11])
            })
            .collect();
        p += nblocks * 12;
        let mut methods = vec!["None".to_string()];
        for i in 0..nmeth {
            methods.push(
                String::from_utf8_lossy(&b[p + i * methlen..p + (i + 1) * methlen])
                    .trim_end_matches('\0')
                    .to_string(),
            );
        }
        p += nmeth * methlen;
        let mut files = Vec::new();
        if flags & 8 != 0 && dir_size > 0 {
            need(p, dir_size)?;
            files = parse_dir_index(&b[p..p + dir_size])?;
        }
        Ok(Toc {
            block_size,
            offs_lens,
            blocks,
            methods,
            files,
            partition_count,
            partition_size,
        })
    }
}

fn parse_dir_index(d: &[u8]) -> Res<Vec<(String, u32)>> {
    let chk = |p: usize, l: usize| {
        if p.checked_add(l).is_some_and(|e| e <= d.len()) {
            Ok(())
        } else {
            Err("truncated directory index".to_string())
        }
    };
    let u32at = |p: &mut usize| -> Res<u32> {
        chk(*p, 4)?;
        let v = rd_u32(d, *p);
        *p += 4;
        Ok(v)
    };
    let fstr = |p: &mut usize| -> Res<String> {
        let n = u32at(p)? as i32;
        if n == 0 {
            return Ok(String::new());
        }
        if n > 0 {
            let n = n as usize;
            chk(*p, n)?;
            let s = String::from_utf8_lossy(&d[*p..*p + n - 1]).into_owned();
            *p += n;
            Ok(s)
        } else {
            let n = (-(n as i64)) as usize;
            chk(*p, n * 2)?;
            let w: Vec<u16> = (0..n - 1)
                .map(|i| u16::from_le_bytes([d[*p + 2 * i], d[*p + 2 * i + 1]]))
                .collect();
            *p += n * 2;
            Ok(String::from_utf16_lossy(&w))
        }
    };
    let mut p = 0usize;
    let _mount = fstr(&mut p)?;
    let nd = u32at(&mut p)? as usize;
    chk(p, nd.checked_mul(16).ok_or("bad dir count")?)?;
    // (name, first_child, next_sibling, first_file)
    let dirs: Vec<(u32, u32, u32, u32)> = (0..nd)
        .map(|i| {
            let o = p + i * 16;
            (
                rd_u32(d, o),
                rd_u32(d, o + 4),
                rd_u32(d, o + 8),
                rd_u32(d, o + 12),
            )
        })
        .collect();
    p += nd * 16;
    let nf = u32at(&mut p)? as usize;
    chk(p, nf.checked_mul(12).ok_or("bad file count")?)?;
    // (name, next_file, entry index)
    let fl: Vec<(u32, u32, u32)> = (0..nf)
        .map(|i| {
            let o = p + i * 12;
            (rd_u32(d, o), rd_u32(d, o + 4), rd_u32(d, o + 8))
        })
        .collect();
    p += nf * 12;
    let ns = u32at(&mut p)? as usize;
    let mut strs = Vec::with_capacity(ns.min(1 << 20));
    for _ in 0..ns {
        strs.push(fstr(&mut p)?);
    }
    const NONE: u32 = u32::MAX;
    let mut out = Vec::new();
    if nd == 0 {
        return Ok(out);
    }
    let mut stack = vec![(0usize, String::new())];
    while let Some((di, prefix)) = stack.pop() {
        let (name, child, _sib, first_file) = dirs[di];
        let path = if di == 0 {
            String::new()
        } else {
            format!(
                "{prefix}{}/",
                strs.get(name as usize).ok_or("bad dir name")?
            )
        };
        let (mut f, mut guard) = (first_file, 0usize);
        while f != NONE && (f as usize) < fl.len() && guard <= nf {
            let (fname, next, user) = fl[f as usize];
            out.push((
                format!("{path}{}", strs.get(fname as usize).ok_or("bad file name")?),
                user,
            ));
            f = next;
            guard += 1;
        }
        let (mut c, mut guard) = (child, 0usize);
        while c != NONE && (c as usize) < dirs.len() && guard <= nd {
            stack.push((c as usize, path.clone()));
            c = dirs[c as usize].2;
            guard += 1;
        }
    }
    Ok(out)
}

struct Container {
    toc: Toc,
    parts: Vec<File>,
}

impl Container {
    fn open(utoc: &Path) -> Res<Container> {
        let b = std::fs::read(utoc).map_err(|e| format!("{}: {e}", utoc.display()))?;
        let toc = Toc::parse(&b)?;
        let mut parts = Vec::new();
        for i in 0..toc.partition_count {
            let path = if i == 0 {
                utoc.with_extension("ucas")
            } else {
                let stem = utoc
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                utoc.with_file_name(format!("{stem}_s{i}.ucas"))
            };
            parts.push(File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?);
        }
        Ok(Container { toc, parts })
    }

    /// Reads and decompresses one chunk (a package's export bundle data).
    fn read_chunk(&mut self, idx: usize) -> Res<Vec<u8>> {
        let &(off, len) = self
            .toc
            .offs_lens
            .get(idx)
            .ok_or("chunk index out of range")?;
        let bs = self.toc.block_size;
        if bs == 0 {
            return Err("block size 0".into());
        }
        let first = (off / bs) as usize;
        let last = ((off + len).div_ceil(bs)) as usize;
        if last > self.toc.blocks.len() {
            return Err("chunk beyond block table".into());
        }
        let mut out = Vec::with_capacity(len as usize + bs as usize);
        for bi in first..last {
            let (boff, csz, usz, m) = self.toc.blocks[bi];
            let part = (boff / self.toc.partition_size) as usize;
            let file = self.parts.get_mut(part).ok_or("partition missing")?;
            file.seek(SeekFrom::Start(boff % self.toc.partition_size))
                .map_err(|e| e.to_string())?;
            let mut raw = vec![0u8; csz as usize];
            file.read_exact(&mut raw).map_err(|e| e.to_string())?;
            if m == 0 {
                out.extend_from_slice(&raw[..(usz as usize).min(raw.len())]);
            } else {
                let name = self
                    .toc
                    .methods
                    .get(m as usize)
                    .map(String::as_str)
                    .unwrap_or("");
                if !name.eq_ignore_ascii_case("oodle") {
                    return Err(format!("compression method {name:?} unsupported"));
                }
                let mut buf = vec![0u8; usz as usize];
                let n = oozextract::Extractor::new()
                    .read_from_slice(&raw, &mut buf)
                    .map_err(|e| format!("oodle: {e:?}"))?;
                out.extend_from_slice(&buf[..n]);
            }
        }
        let skip = (off - first as u64 * bs) as usize;
        if out.len() < skip + len as usize {
            return Err("short chunk".into());
        }
        Ok(out[skip..skip + len as usize].to_vec())
    }
}

// ------------------------------------------------------------------ zen package

struct ExportEntry {
    offset: u64,
    size: u64,
    name: String,
}

struct Zen {
    names: Vec<String>,
    imports: Vec<u64>,
    imported_packages: Vec<String>,
    exports: Vec<ExportEntry>,
    header_size: usize,
    data: Vec<u8>,
}

/// FNameEntrySerialized name batch. Returns (names, end position).
fn parse_name_batch(b: &[u8], mut p: usize) -> Res<(Vec<String>, usize)> {
    let bad = || "bad name batch".to_string();
    if p + 4 > b.len() {
        return Err(bad());
    }
    let num = rd_u32(b, p) as usize;
    p += 4;
    if num == 0 {
        return Ok((Vec::new(), p));
    }
    if p + 12 > b.len() || num > 1 << 22 {
        return Err(bad());
    }
    let str_bytes = rd_u32(b, p) as usize;
    p += 12; // byte count + hash algorithm id
    if p + num * 10 + str_bytes > b.len() {
        return Err(bad());
    }
    p += num * 8; // hashes
    let hdr = p;
    p += num * 2;
    let (mut names, mut sp) = (Vec::with_capacity(num), p);
    for i in 0..num {
        let h = u16::from_be_bytes([b[hdr + i * 2], b[hdr + i * 2 + 1]]);
        let len = (h & 0x7fff) as usize;
        if h & 0x8000 != 0 {
            if sp + len * 2 > p + str_bytes {
                return Err(bad());
            }
            let w: Vec<u16> = (0..len)
                .map(|k| u16::from_le_bytes([b[sp + 2 * k], b[sp + 2 * k + 1]]))
                .collect();
            names.push(String::from_utf16_lossy(&w));
            sp += len * 2;
        } else {
            if sp + len > p + str_bytes {
                return Err(bad());
            }
            names.push(String::from_utf8_lossy(&b[sp..sp + len]).into_owned());
            sp += len;
        }
    }
    Ok((names, p + str_bytes))
}

impl Zen {
    /// Parses an unversioned UE 5.3+ zen package header (with or without the 5.6 cell offsets).
    fn parse(data: Vec<u8>) -> Res<Zen> {
        let b = &data[..];
        if b.len() < 0x40 {
            return Err("zen: too short".into());
        }
        if rd_u32(b, 0) != 0 {
            return Err("zen: versioned package header not supported".into());
        }
        let header_size = rd_u32(b, 4) as usize;
        if header_size > b.len() {
            return Err("zen: header size beyond chunk".into());
        }
        let hashes_off = rd_u32(b, 24) as usize;
        let imp_off = rd_u32(b, 28) as usize;
        let exp_off = rd_u32(b, 32) as usize;
        let bundle_off = rd_u32(b, 36) as usize;
        let pkgnames_off = rd_u32(b, 48) as usize;
        if !(hashes_off <= imp_off
            && imp_off <= exp_off
            && exp_off <= bundle_off
            && bundle_off <= pkgnames_off
            && pkgnames_off <= header_size)
        {
            return Err("zen: header offsets not monotonic (pre-5.3 zen layout?)".into());
        }
        // The name map follows the 0x34-byte summary, plus 8 bytes of cell offsets since UE 5.6.
        let names = [0x34usize, 0x3c]
            .into_iter()
            .find_map(|start| {
                let (names, end) = parse_name_batch(b, start).ok()?;
                (!names.is_empty() && end <= hashes_off && rd_u64(b, start + 8) == 0xC164_0000)
                    .then_some(names)
            })
            .ok_or("zen: no valid name map")?;
        let imports = (0..(exp_off - imp_off) / 8)
            .map(|i| rd_u64(b, imp_off + i * 8))
            .collect();
        let mapped = |v: u32| {
            names
                .get((v & 0x3fff_ffff) as usize)
                .cloned()
                .unwrap_or_default()
        };
        let exports = (0..(bundle_off - exp_off) / 72)
            .map(|i| {
                let o = exp_off + i * 72;
                ExportEntry {
                    offset: rd_u64(b, o),
                    size: rd_u64(b, o + 8),
                    name: mapped(rd_u32(b, o + 16)),
                }
            })
            .collect();
        let (imported_packages, _) = parse_name_batch(b, pkgnames_off)?;
        Ok(Zen {
            names,
            imports,
            imported_packages,
            exports,
            header_size,
            data,
        })
    }

    fn export_bytes(&self, i: usize) -> Res<&[u8]> {
        let e = self.exports.get(i).ok_or("no such export")?;
        let s = self
            .header_size
            .checked_add(e.offset as usize)
            .ok_or("bad export offset")?;
        let end = s.checked_add(e.size as usize).ok_or("bad export size")?;
        self.data
            .get(s..end)
            .ok_or_else(|| "export beyond chunk".to_string())
    }

    /// Name of the asset behind an FPackageIndex import (`IA_Jump` for `/Game/.../IA_Jump`).
    fn import_asset_name(&self, v: i32) -> Res<String> {
        if v >= 0 {
            return Err(format!("action reference {v} is not an import"));
        }
        let imp = *self
            .imports
            .get((-(v as i64) - 1) as usize)
            .ok_or("import index out of range")?;
        if imp >> 62 != 2 {
            return Err("action import is not a package import".into());
        }
        let pkg = ((imp >> 32) & 0x3fff_ffff) as usize;
        let full = self
            .imported_packages
            .get(pkg)
            .ok_or("imported package index out of range")?;
        Ok(full.rsplit('/').next().unwrap_or(full).to_string())
    }
}

// ------------------------------------------------------- unversioned properties

/// Parsed FUnversionedHeader: (property index, is_zero) for each present property.
fn read_uheader(b: &[u8], p: &mut usize) -> Res<Vec<(u32, bool)>> {
    let mut frags: Vec<(u32, u32, bool)> = Vec::new(); // skip, values, has_zero
    loop {
        if *p + 2 > b.len() {
            return Err("unversioned header: eof".into());
        }
        let f = u16::from_le_bytes([b[*p], b[*p + 1]]) as u32;
        *p += 2;
        frags.push((f & 0x7f, f >> 9, f & 0x80 != 0));
        if f & 0x100 != 0 {
            break;
        }
        if frags.len() > 4096 {
            return Err("unversioned header: runaway".into());
        }
    }
    let zero_n: u32 = frags.iter().filter(|f| f.2).map(|f| f.1).sum();
    let mut mask: Vec<bool> = Vec::new();
    if zero_n > 0 {
        let nbytes = match zero_n {
            1..=8 => 1,
            9..=16 => 2,
            n => 4 * (n as usize).div_ceil(32),
        };
        if *p + nbytes > b.len() {
            return Err("unversioned header: mask eof".into());
        }
        mask = (0..zero_n as usize)
            .map(|i| b[*p + i / 8] >> (i % 8) & 1 != 0)
            .collect();
        *p += nbytes;
    }
    let (mut idx, mut mi) = (0u32, 0usize);
    let mut props = Vec::new();
    for (skip, n, hz) in frags {
        idx += skip;
        for _ in 0..n {
            let z = if hz {
                mi += 1;
                mask[mi - 1]
            } else {
                false
            };
            props.push((idx, z));
            idx += 1;
        }
    }
    Ok(props)
}

struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl Cur<'_> {
    fn u32(&mut self) -> Res<u32> {
        if self.p + 4 > self.b.len() {
            return Err("export data: eof".into());
        }
        let v = rd_u32(self.b, self.p);
        self.p += 4;
        Ok(v)
    }
    fn skip(&mut self, n: usize) -> Res<()> {
        if self.p + n > self.b.len() {
            return Err("export data: eof".into());
        }
        self.p += n;
        Ok(())
    }
    fn fstring(&mut self) -> Res<()> {
        let n = self.u32()? as i32;
        let bytes = if n >= 0 {
            n as usize
        } else {
            (-(n as i64)) as usize * 2
        };
        if bytes > 1 << 20 {
            return Err("export data: implausible FString".into());
        }
        self.skip(bytes)
    }
    /// Skips a serialized FText (flags, history type; Base: namespace/key/source; None: optional string).
    fn skip_ftext(&mut self) -> Res<()> {
        self.u32()?;
        let h = *self.b.get(self.p).ok_or("export data: eof")? as i8;
        self.p += 1;
        match h {
            0 => {
                self.fstring()?;
                self.fstring()?;
                self.fstring()
            }
            -1 => {
                if self.u32()? != 0 {
                    self.fstring()?;
                }
                Ok(())
            }
            _ => Err(format!("unsupported FText history type {h}")),
        }
    }
}

// ------------------------------------------------------------- IMC decoding

impl Zen {
    /// Decodes `UInputMappingContext` (an export) into (action asset name, key name) pairs.
    ///
    /// Property indices in the unversioned schema, as found in UE 5.6 cooks:
    /// IMC: 0 = Mappings, 4 = ContextDescription (FText).
    /// FEnhancedActionKeyMapping: 0/1 = Triggers/Modifiers (arrays of FPackageIndex), 2 = Action
    /// (FPackageIndex import), 3 = Key (FKey struct: KeyName FName), 6 = settings behaviour (u8),
    /// 7 = player-mappable settings (FPackageIndex). Every IMC must be consumed exactly
    /// (plus the 4-byte object footer) or the decode is rejected.
    fn decode_imc(&self, export: usize) -> Res<Vec<(String, String)>> {
        let b = self.export_bytes(export)?;
        let mut c = Cur { b, p: 0 };
        let mut out = Vec::new();
        for (idx, zero) in read_uheader(b, &mut c.p)? {
            if zero {
                continue;
            }
            match idx {
                0 => {
                    let n = c.u32()? as usize;
                    if n > 100_000 {
                        return Err("implausible mapping count".into());
                    }
                    for _ in 0..n {
                        out.push(self.decode_mapping(&mut c)?);
                    }
                }
                4 => c.skip_ftext()?,
                _ => return Err(format!("unknown IMC property index {idx}")),
            }
        }
        if b[c.p..] != [0, 0, 0, 0] {
            return Err(format!(
                "{} unexpected trailing bytes in IMC export",
                b.len() - c.p
            ));
        }
        Ok(out)
    }

    fn decode_mapping(&self, c: &mut Cur) -> Res<(String, String)> {
        let (mut action, mut key) = (None, None);
        for (idx, zero) in read_uheader(c.b, &mut c.p)? {
            if zero {
                continue;
            }
            match idx {
                0 | 1 => {
                    let n = c.u32()? as usize;
                    if n > 1024 {
                        return Err("implausible trigger/modifier count".into());
                    }
                    c.skip(n * 4)?;
                }
                2 => action = Some(self.import_asset_name(c.u32()? as i32)?),
                3 => {
                    for (ki, kz) in read_uheader(c.b, &mut c.p)? {
                        if kz {
                            continue;
                        }
                        if ki != 0 {
                            return Err(format!("unknown FKey property {ki}"));
                        }
                        let (ni, num) = (c.u32()?, c.u32()?);
                        let nm = self
                            .names
                            .get(ni as usize)
                            .ok_or("key name index out of range")?;
                        key = Some(if num == 0 {
                            nm.clone()
                        } else {
                            format!("{nm}_{}", num - 1)
                        });
                    }
                }
                6 => c.skip(1)?,
                7 => c.skip(4)?,
                _ => return Err(format!("unknown mapping property index {idx}")),
            }
        }
        // A missing KeyName is FName "None": an unbound mapping.
        Ok((
            action.ok_or("mapping without action")?,
            key.unwrap_or_else(|| "None".to_string()),
        ))
    }
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    fn fstr(s: &str) -> Vec<u8> {
        let mut v = ((s.len() + 1) as i32).to_le_bytes().to_vec();
        v.extend_from_slice(s.as_bytes());
        v.push(0);
        v
    }

    fn name_batch(names: &[&str]) -> Vec<u8> {
        let mut v = (names.len() as u32).to_le_bytes().to_vec();
        if names.is_empty() {
            return v;
        }
        v.extend_from_slice(&(names.iter().map(|n| n.len()).sum::<usize>() as u32).to_le_bytes());
        v.extend_from_slice(&0xC164_0000u64.to_le_bytes());
        v.extend(std::iter::repeat_n(0u8, names.len() * 8));
        for n in names {
            v.extend_from_slice(&(n.len() as u16).to_be_bytes());
        }
        for n in names {
            v.extend_from_slice(n.as_bytes());
        }
        v
    }

    /// Builds an unversioned zen package. `exports`: (name index, data).
    fn build_zen(
        cells: bool,
        names: &[&str],
        imports: &[u64],
        pkgs: &[&str],
        exports: &[(u32, Vec<u8>)],
    ) -> Vec<u8> {
        let mut h = vec![0u8; 0x34 + if cells { 8 } else { 0 }];
        h.extend(name_batch(names));
        h.extend([0u8; 16]); // bulk data padding + empty bulk data map
        let hashes_off = h.len();
        let imp_off = h.len();
        for i in imports {
            h.extend(i.to_le_bytes());
        }
        let exp_off = h.len();
        let mut off = 0u64;
        for (n, d) in exports {
            let mut e = Vec::new();
            e.extend(off.to_le_bytes());
            e.extend((d.len() as u64).to_le_bytes());
            e.extend((*n as u64).to_le_bytes());
            e.extend([0xffu8; 8]);
            e.extend(vec![0u8; 72 - e.len()]);
            h.extend(e);
            off += d.len() as u64;
        }
        let bundle_off = h.len();
        h.extend(vec![0u8; exports.len() * 16]);
        let pkg_off = h.len();
        h.extend(name_batch(pkgs));
        let header_size = h.len();
        h[4..8].copy_from_slice(&(header_size as u32).to_le_bytes());
        for (at, v) in [
            (24, hashes_off),
            (28, imp_off),
            (32, exp_off),
            (36, bundle_off),
            (40, bundle_off),
            (44, bundle_off),
            (48, pkg_off),
        ] {
            h[at..at + 4].copy_from_slice(&(v as u32).to_le_bytes());
        }
        for (_, d) in exports {
            h.extend(d);
        }
        h.extend([0xc1, 0x83, 0x2a, 0x9e]);
        h
    }

    fn mapping(imp_idx: i32, key: Option<u32>, with_settings: bool) -> Vec<u8> {
        // 4 leading values; then either (6,7) both present, or (6,7) zero-masked.
        let mut v = if with_settings {
            vec![0x00, 0x08, 0x02, 0x05]
        } else {
            vec![0x00, 0x08, 0x82, 0x05, 0x03]
        };
        v.extend(0u32.to_le_bytes()); // triggers
        v.extend(0u32.to_le_bytes()); // modifiers
        v.extend((-(imp_idx) - 1).to_le_bytes()); // action
        match key {
            Some(k) => {
                v.extend([0x00, 0x03]);
                v.extend(k.to_le_bytes());
                v.extend(0u32.to_le_bytes());
            }
            None => v.extend([0x80, 0x03, 0x01]),
        }
        if with_settings {
            v.push(1);
            v.extend(7u32.to_le_bytes());
        } // enum byte, settings object
        v
    }

    fn imc_zen(cells: bool) -> Vec<u8> {
        let names = ["Gamepad_FaceButton_Bottom", "SpaceBar", "IMC_Test"];
        let imports = [
            0x7ec7_1521_e8db_ce5du64,
            0x8000_0000_0000_0000,
            0x8000_0001_0000_0000,
        ];
        let pkgs = ["/Game/Input/Actions/IA_Jump", "/Game/Input/Actions/IA_Fire"];
        let mut d = vec![0x00, 0x03]; // property 0 only
        d.extend(3u32.to_le_bytes());
        d.extend(mapping(1, Some(0), false));
        d.extend(mapping(2, Some(1), true));
        d.extend(mapping(1, None, false));
        d.extend([0, 0, 0, 0]);
        build_zen(cells, &names, &imports, &pkgs, &[(2, d)])
    }

    #[test]
    fn name_batch_roundtrip() {
        let b = name_batch(&["abc", "", "Gamepad_Left2D"]);
        let (n, end) = parse_name_batch(&b, 0).unwrap();
        assert_eq!(n, ["abc", "", "Gamepad_Left2D"]);
        assert_eq!(end, b.len());
        assert!(parse_name_batch(&b[..b.len() - 3], 0).is_err());
    }

    #[test]
    fn unversioned_header_fragments_and_zero_mask() {
        // [skip 0, 4 values] [skip 2, 2 values, has zeroes, last], mask 0b01: index 6 zero, 7 present.
        let b = [0x00, 0x08, 0x82, 0x05, 0x01];
        let mut p = 0;
        let h = read_uheader(&b, &mut p).unwrap();
        assert_eq!(p, 5);
        assert_eq!(
            h,
            [
                (0, false),
                (1, false),
                (2, false),
                (3, false),
                (6, true),
                (7, false)
            ]
        );
        // [skip 0, 1 value] [skip 3, 1 value, last]
        let b = [0x00, 0x02, 0x03, 0x03];
        let h = read_uheader(&b, &mut 0).unwrap();
        assert_eq!(h, [(0, false), (4, false)]);
        assert!(read_uheader(&[0x00], &mut 0).is_err());
    }

    #[test]
    fn decodes_synthetic_imc_with_and_without_cell_offsets() {
        for cells in [true, false] {
            let z = Zen::parse(imc_zen(cells)).unwrap();
            let m = z.decode_imc(0).unwrap();
            assert_eq!(
                m,
                [
                    (
                        "IA_Jump".to_string(),
                        "Gamepad_FaceButton_Bottom".to_string()
                    ),
                    ("IA_Fire".to_string(), "SpaceBar".to_string()),
                    ("IA_Jump".to_string(), "None".to_string()),
                ]
            );
        }
    }

    #[test]
    fn rejects_unknown_layout_instead_of_guessing() {
        let mut b = imc_zen(true);
        let z = Zen::parse(b.clone()).unwrap();
        let at = z.header_size + 2 + 4; // first mapping header: bump the second fragment to index 9
        b[at + 2] = 0x04; // skip 4, no zero mask -> property indices 8,9
        let z = Zen::parse(b).unwrap();
        assert!(z.decode_imc(0).is_err());
    }

    #[test]
    fn rejects_versioned_and_garbage_headers() {
        let mut b = imc_zen(true);
        b[0] = 1;
        assert!(Zen::parse(b).is_err());
        assert!(Zen::parse(vec![0u8; 0x80]).is_err());
    }

    fn dir_index() -> Vec<u8> {
        let mut d = fstr("../../../");
        d.extend(2u32.to_le_bytes());
        for e in [
            [u32::MAX, 1, u32::MAX, u32::MAX],
            [0, u32::MAX, u32::MAX, 0],
        ] {
            for x in e {
                d.extend(x.to_le_bytes());
            }
        }
        d.extend(1u32.to_le_bytes());
        for x in [1u32, u32::MAX, 0] {
            d.extend(x.to_le_bytes());
        }
        d.extend(2u32.to_le_bytes());
        d.extend(fstr("Game"));
        d.extend(fstr("IMC_Test.uasset"));
        d
    }

    #[test]
    fn directory_index_paths() {
        assert_eq!(
            parse_dir_index(&dir_index()).unwrap(),
            [("Game/IMC_Test.uasset".to_string(), 0)]
        );
        assert!(parse_dir_index(&dir_index()[..20]).is_err());
    }

    /// Writes a one-chunk, one-block, uncompressed container into a fresh temp dir.
    fn write_container(chunk: &[u8]) -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let di = dir_index();
        let mut t = vec![0u8; 144];
        t[0..16].copy_from_slice(b"-==--==--==--==-");
        t[16] = 8;
        for (at, v) in [
            (20, 144u32),
            (24, 1),
            (28, 1),
            (32, 12),
            (36, 0),
            (40, 32),
            (44, 0x10000),
            (48, di.len() as u32),
            (52, 1),
        ] {
            t[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        t[0x50] = 1 | 8;
        t[0x58..0x60].copy_from_slice(&u64::MAX.to_le_bytes());
        t.extend([0u8; 12]); // chunk id
        t.extend([0, 0, 0, 0, 0]); // offset (5 bytes BE)
        t.extend((chunk.len() as u64).to_be_bytes()[3..].iter()); // length (5 bytes BE)
        t.extend(0u32.to_le_bytes()); // block offset low
        t.push(0); // block offset high
        t.extend(&(chunk.len() as u32).to_le_bytes()[..3]);
        t.extend(&(chunk.len() as u32).to_le_bytes()[..3]);
        t.push(0); // method none
        t.extend(di);
        std::fs::write(dir.join("test.utoc"), t).unwrap();
        let mut padded = chunk.to_vec();
        padded.resize(chunk.len().div_ceil(16) * 16, 0);
        std::fs::write(dir.join("test.ucas"), padded).unwrap();
        temp
    }

    #[test]
    fn end_to_end_on_a_synthetic_container() {
        let dir = write_container(&imc_zen(true));
        let (ctx, skipped) = read_iostore_mappings_report(dir.path()).unwrap();
        assert!(skipped.is_empty(), "{skipped:?}");
        assert_eq!(ctx.len(), 1);
        assert_eq!(ctx[0].name, "IMC_Test");
        assert_eq!(ctx[0].package, "Game/IMC_Test.uasset");
        assert_eq!(
            ctx[0].mappings,
            [
                (
                    "IA_Jump".to_string(),
                    "Gamepad_FaceButton_Bottom".to_string()
                ),
                ("IA_Fire".to_string(), "SpaceBar".to_string()),
            ]
        );
    }

    #[test]
    fn broken_imc_is_reported_not_returned() {
        let dir = write_container(&[0u8; 100]);
        let (ctx, skipped) = read_iostore_mappings_report(dir.path()).unwrap();
        assert!(ctx.is_empty());
        assert_eq!(skipped.len(), 1);
    }

    #[test]
    fn folder_without_containers_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_iostore_mappings(dir.path()).unwrap().is_empty());
    }

    #[test]
    #[ignore = "needs the Minecraft Dungeons II dump"]
    fn real_dump_minecraft_dungeons_ii() {
        // `ANYPS5_TEST_PAKS=<dump>/dungeons/content/paks cargo test -- --ignored real_dump_minecraft`
        let paks = std::env::var("ANYPS5_TEST_PAKS").expect("ANYPS5_TEST_PAKS");
        let (ctx, skipped) = read_iostore_mappings_report(Path::new(&paks)).unwrap();
        assert!(skipped.is_empty(), "{skipped:?}");
        assert_eq!(ctx.len(), 13);
        let get = |n: &str, a: &str| -> Vec<String> {
            ctx.iter()
                .find(|c| c.name == n)
                .unwrap()
                .mappings
                .iter()
                .filter(|(x, _)| x == a)
                .map(|(_, k)| k.clone())
                .collect()
        };
        // Facts stated by the game's own automated test steps.
        assert_eq!(
            get("IMC_DefaultGamepad", "IA_Jump"),
            ["Gamepad_FaceButton_Bottom"]
        );
        assert_eq!(
            get("IMC_DefaultGamepad", "IA_RangedAttack"),
            ["Gamepad_RightTrigger"]
        );
        assert_eq!(
            get("IMC_DefaultGamepad_Interact", "IA_Gamepad_Interact"),
            ["Gamepad_FaceButton_Bottom"]
        );
        assert_eq!(
            get("IMC_Switch_Interact", "IA_Gamepad_Interact"),
            ["Gamepad_FaceButton_Right"]
        );
        // Keyboard profile.
        assert_eq!(get("IMC_DefaultKBM", "IA_Jump"), ["SpaceBar"]);
        assert_eq!(
            get("IMC_DefaultKBM", "IA_RangedAttack"),
            ["RightMouseButton"]
        );
        assert_eq!(
            get("IMC_Telescope", "IA_TelescopePan"),
            ["W", "S", "A", "D"]
        );
    }
}
