//! Pure Rust `.aios-plugin` (`.tar.gz`) archive packer, SHA-256 verifier, and safe extractor.
//!
//! Enforces strict supply-chain protections before installing any external plugin bundle:
//! - Optional mandatory SHA-256 verification before decompression
//! - Bounded compressed (16 MiB) and uncompressed (64 MiB) payload limits against archive bombs
//! - Rejection of absolute paths, `..` traversal components, symlinks, hardlinks, and device nodes
//! - Atomic staging + manifest validation before moving into `$XDG_DATA_HOME/aios/plugins/<id>/`

use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use aios_core::{AiosError, Result};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};

use crate::manifest::{InstalledPlugin, load_plugin_from_dir};

pub const PLUGIN_ARCHIVE_EXT: &str = "aios-plugin";
pub const MAX_COMPRESSED_ARCHIVE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_UNCOMPRESSED_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const TAR_BLOCK_SIZE: usize = 512;

/// Computes a lowercase hex SHA-256 digest of `bytes`.
#[must_use]
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Computes a lowercase hex SHA-256 digest of the file at `path`.
pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).map_err(|source| AiosError::IoPath {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(sha256_bytes(&bytes))
}

/// Validates a local plugin directory and packs it into a deterministic `.aios-plugin` (`.tar.gz`)
/// archive at `output_archive`, returning the archive's lowercase hex SHA-256 checksum.
pub fn pack_plugin_dir(src_dir: &Path, output_archive: &Path) -> Result<(InstalledPlugin, String)> {
    let validated = load_plugin_from_dir(src_dir)?;
    let mut rel_files = Vec::new();
    collect_packable_files(src_dir, src_dir, &mut rel_files)?;
    rel_files.sort();

    let mut tar_buf = Vec::new();
    for rel in &rel_files {
        let abs = src_dir.join(rel);
        let data = fs::read(&abs).map_err(|source| AiosError::IoPath { path: abs, source })?;
        let rel_str = rel
            .to_str()
            .ok_or_else(|| AiosError::InvalidConfig("non-UTF8 path inside plugin bundle".into()))?
            .replace('\\', "/");
        append_tar_regular_file(&mut tar_buf, &rel_str, &data)?;
    }
    // Standard POSIX tar end-of-archive marker (two 512-byte zero blocks).
    tar_buf.resize(tar_buf.len() + TAR_BLOCK_SIZE * 2, 0);

    let mut gz = GzEncoder::new(Vec::new(), Compression::best());
    gz.write_all(&tar_buf).map_err(|e| {
        AiosError::InvalidConfig(format!("failed to gzip-compress plugin archive: {e}"))
    })?;
    let compressed = gz.finish().map_err(|e| {
        AiosError::InvalidConfig(format!("failed to finalize gzip plugin archive: {e}"))
    })?;

    if let Some(parent) = output_archive.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| AiosError::IoPath {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(output_archive, &compressed).map_err(|source| AiosError::IoPath {
        path: output_archive.to_path_buf(),
        source,
    })?;

    let sha256 = sha256_bytes(&compressed);
    Ok((validated, sha256))
}

/// Verifies optional `expected_sha256`, safely extracts a `.aios-plugin` (`.tar.gz`) payload,
/// validates `aios-plugin.toml` + `.wasm`, and atomically installs it into `plugins_dir/<id>/`.
pub fn install_plugin_archive_bytes(
    archive_bytes: &[u8],
    expected_sha256: Option<&str>,
    plugins_dir: &Path,
) -> Result<(InstalledPlugin, String)> {
    if archive_bytes.len() > MAX_COMPRESSED_ARCHIVE_BYTES {
        return Err(AiosError::InvalidConfig(format!(
            "plugin archive exceeds maximum compressed size of {} MiB",
            MAX_COMPRESSED_ARCHIVE_BYTES / (1024 * 1024)
        )));
    }

    let actual_sha = sha256_bytes(archive_bytes);
    if let Some(expected) = expected_sha256 {
        let norm_expected = expected.trim().to_ascii_lowercase();
        if !norm_expected.is_empty() && norm_expected != actual_sha {
            return Err(AiosError::InvalidConfig(format!(
                "SHA-256 mismatch: expected {norm_expected}, got {actual_sha}"
            )));
        }
    }

    // Bounded decompression to defeat gzip bombs.
    let decoder = GzDecoder::new(archive_bytes);
    let mut tar_bytes = Vec::new();
    decoder
        .take((MAX_UNCOMPRESSED_ARCHIVE_BYTES + 1) as u64)
        .read_to_end(&mut tar_bytes)
        .map_err(|e| AiosError::InvalidConfig(format!("invalid gzip plugin archive: {e}")))?;
    if tar_bytes.len() > MAX_UNCOMPRESSED_ARCHIVE_BYTES {
        return Err(AiosError::InvalidConfig(format!(
            "plugin archive exceeds maximum uncompressed size of {} MiB",
            MAX_UNCOMPRESSED_ARCHIVE_BYTES / (1024 * 1024)
        )));
    }

    fs::create_dir_all(plugins_dir).map_err(|source| AiosError::IoPath {
        path: plugins_dir.to_path_buf(),
        source,
    })?;

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let staging_dir = plugins_dir.join(format!(".staging-{}-{nonce}", std::process::id()));
    if staging_dir.exists() {
        let _ = fs::remove_dir_all(&staging_dir);
    }
    fs::create_dir_all(&staging_dir).map_err(|source| AiosError::IoPath {
        path: staging_dir.clone(),
        source,
    })?;

    let extract_res = (|| -> Result<InstalledPlugin> {
        extract_tar_bytes_safe(&tar_bytes, &staging_dir)?;
        let bundle_root = locate_extracted_manifest_root(&staging_dir)?;
        let validated = load_plugin_from_dir(&bundle_root)?;
        let target_dir = plugins_dir.join(&validated.manifest.id);

        if target_dir.exists() {
            fs::remove_dir_all(&target_dir).map_err(|source| AiosError::IoPath {
                path: target_dir.clone(),
                source,
            })?;
        }

        if bundle_root == staging_dir {
            fs::rename(&staging_dir, &target_dir).map_err(|source| AiosError::IoPath {
                path: target_dir.clone(),
                source,
            })?;
        } else {
            fs::rename(&bundle_root, &target_dir).map_err(|source| AiosError::IoPath {
                path: target_dir.clone(),
                source,
            })?;
            let _ = fs::remove_dir_all(&staging_dir);
        }

        load_plugin_from_dir(&target_dir)
    })();

    if extract_res.is_err() && staging_dir.exists() {
        let _ = fs::remove_dir_all(&staging_dir);
    }

    Ok((extract_res?, actual_sha))
}

fn locate_extracted_manifest_root(staging_dir: &Path) -> Result<PathBuf> {
    if staging_dir.join("aios-plugin.toml").is_file() {
        return Ok(staging_dir.to_path_buf());
    }
    // Also tolerate a single top-level wrapper folder (common in hand-tarred archives).
    let mut subdirs = Vec::new();
    for entry in fs::read_dir(staging_dir).map_err(|source| AiosError::IoPath {
        path: staging_dir.to_path_buf(),
        source,
    })? {
        let path = entry
            .map_err(|source| AiosError::IoPath {
                path: staging_dir.to_path_buf(),
                source,
            })?
            .path();
        if path.is_dir() {
            subdirs.push(path);
        }
    }
    if subdirs.len() == 1 && subdirs[0].join("aios-plugin.toml").is_file() {
        return Ok(subdirs.remove(0));
    }
    Err(AiosError::InvalidConfig(
        "plugin archive is missing 'aios-plugin.toml' at bundle root".to_string(),
    ))
}

fn collect_packable_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(dir).map_err(|source| AiosError::IoPath {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| AiosError::IoPath {
            path: dir.to_path_buf(),
            source,
        })?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Skip source/build/VCS artifacts so archives contain only runtime bundle files.
        if matches!(
            name_str.as_ref(),
            "target" | ".git" | ".github" | "src" | "Cargo.toml" | "Cargo.lock" | ".gitignore"
        ) || name_str.ends_with(&format!(".{PLUGIN_ARCHIVE_EXT}"))
        {
            continue;
        }
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|source| AiosError::IoPath {
            path: path.clone(),
            source,
        })?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            collect_packable_files(root, &path, out)?;
        } else if meta.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| AiosError::InvalidConfig(e.to_string()))?;
            out.push(rel.to_path_buf());
        }
    }
    Ok(())
}

fn append_tar_regular_file(out: &mut Vec<u8>, rel_path: &str, data: &[u8]) -> Result<()> {
    let (prefix, name) = split_ustar_path(rel_path)?;
    let mut header = [0u8; TAR_BLOCK_SIZE];
    header[0..name.len()].copy_from_slice(name.as_bytes());
    write_octal(&mut header[100..108], 0o644);
    write_octal(&mut header[108..116], 0);
    write_octal(&mut header[116..124], 0);
    write_octal(&mut header[124..136], data.len() as u64);
    write_octal(&mut header[136..148], 0);
    header[148..156].fill(b' ');
    header[156] = b'0'; // Regular file
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());

    let checksum: u32 = header.iter().map(|&b| u32::from(b)).sum();
    let chk_str = format!("{checksum:06o}\0 ");
    header[148..156].copy_from_slice(chk_str.as_bytes());

    out.extend_from_slice(&header);
    out.extend_from_slice(data);
    let pad = (TAR_BLOCK_SIZE - (data.len() % TAR_BLOCK_SIZE)) % TAR_BLOCK_SIZE;
    out.resize(out.len() + pad, 0);
    Ok(())
}

fn split_ustar_path(rel_path: &str) -> Result<(&str, &str)> {
    if rel_path.len() <= 100 {
        return Ok(("", rel_path));
    }
    if let Some(pos) = rel_path.rfind('/') {
        let prefix = &rel_path[..pos];
        let name = &rel_path[pos + 1..];
        if prefix.len() <= 155 && name.len() <= 100 {
            return Ok((prefix, name));
        }
    }
    Err(AiosError::InvalidConfig(format!(
        "plugin bundle path exceeds POSIX ustar length limit: {rel_path}"
    )))
}

fn write_octal(slot: &mut [u8], value: u64) {
    let width = slot.len().saturating_sub(1);
    let formatted = format!("{value:0width$o}");
    let bytes = formatted.as_bytes();
    let copy_len = bytes.len().min(width);
    slot[..copy_len].copy_from_slice(&bytes[bytes.len() - copy_len..]);
    slot[slot.len() - 1] = 0;
}

fn extract_tar_bytes_safe(tar: &[u8], dest_dir: &Path) -> Result<()> {
    let mut offset = 0usize;
    let mut extracted_files = 0usize;

    while offset + TAR_BLOCK_SIZE <= tar.len() {
        let header = &tar[offset..offset + TAR_BLOCK_SIZE];
        offset += TAR_BLOCK_SIZE;
        if header.iter().all(|&b| b == 0) {
            break;
        }

        verify_tar_checksum(header)?;
        let raw_name = read_cstr(&header[0..100]);
        let raw_prefix = read_cstr(&header[345..500]);
        let full_name = if raw_prefix.is_empty() {
            raw_name.to_string()
        } else {
            format!("{raw_prefix}/{raw_name}")
        };
        let size = parse_tar_octal(&header[124..136])? as usize;
        let typeflag = header[156];

        let data_end = offset.checked_add(size).ok_or_else(|| {
            AiosError::InvalidConfig("corrupt tar entry size overflow".to_string())
        })?;
        if data_end > tar.len() {
            return Err(AiosError::InvalidConfig(
                "truncated tar entry inside plugin archive".to_string(),
            ));
        }
        let payload = &tar[offset..data_end];
        let padded_size = size.div_ceil(TAR_BLOCK_SIZE) * TAR_BLOCK_SIZE;
        offset = offset.saturating_add(padded_size);

        // Skip PAX extended header blocks gracefully if produced by GNU/BSD tar.
        if matches!(typeflag, b'x' | b'g') {
            continue;
        }
        if matches!(typeflag, b'1' | b'2' | b'3' | b'4' | b'6') {
            return Err(AiosError::InvalidConfig(format!(
                "unsafe link or special device entry rejected in plugin archive: '{full_name}'"
            )));
        }

        let safe_rel = sanitize_relative_tar_path(&full_name)?;
        if safe_rel.as_os_str().is_empty() {
            continue;
        }
        let dst = dest_dir.join(&safe_rel);
        if typeflag == b'5' || full_name.ends_with('/') {
            fs::create_dir_all(&dst).map_err(|source| AiosError::IoPath { path: dst, source })?;
        } else if typeflag == 0 || typeflag == b'0' {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|source| AiosError::IoPath {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            fs::write(&dst, payload).map_err(|source| AiosError::IoPath { path: dst, source })?;
            extracted_files += 1;
        }
    }

    if extracted_files == 0 {
        return Err(AiosError::InvalidConfig(
            "plugin archive contained no files".to_string(),
        ));
    }
    Ok(())
}

fn sanitize_relative_tar_path(raw: &str) -> Result<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(PathBuf::new());
    }
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err(AiosError::InvalidConfig(format!(
            "absolute path rejected in plugin archive: '{raw}'"
        )));
    }
    let mut clean = PathBuf::new();
    for comp in Path::new(trimmed).components() {
        match comp {
            Component::CurDir => {}
            Component::Normal(part) => clean.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AiosError::InvalidConfig(format!(
                    "path traversal rejected in plugin archive: '{raw}'"
                )));
            }
        }
    }
    Ok(clean)
}

fn read_cstr(slice: &[u8]) -> &str {
    let end = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
    std::str::from_utf8(&slice[..end]).unwrap_or("").trim()
}

fn parse_tar_octal(slice: &[u8]) -> Result<u64> {
    let s = read_cstr(slice).trim_matches([' ', '\0']);
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8)
        .map_err(|_| AiosError::InvalidConfig(format!("invalid tar octal field '{s}'")))
}

fn verify_tar_checksum(header: &[u8]) -> Result<()> {
    let expected = parse_tar_octal(&header[148..156])?;
    let mut computed: u64 = 0;
    for (idx, &b) in header.iter().enumerate() {
        if (148..156).contains(&idx) {
            computed += u64::from(b' ');
        } else {
            computed += u64::from(b);
        }
    }
    if computed == expected {
        Ok(())
    } else {
        Err(AiosError::InvalidConfig(
            "corrupt tar header checksum inside plugin archive".to_string(),
        ))
    }
}
