//! Targeted, comment-preserving config edits with atomic replacement.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result, bail};
use toml_edit::{DocumentMut, Value, value};

use super::Config;

pub(crate) fn save_accent_color(
    path: &Path,
    color: Option<[u8; 3]>,
    custom_color: Option<[u8; 3]>,
) -> Result<()> {
    // Follow existing symlinks so saving never replaces a user's config link.
    let path = match fs::symlink_metadata(path) {
        Ok(_) => fs::canonicalize(path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => path.to_path_buf(),
        Err(error) => return Err(error.into()),
    };
    let original = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let updated = edit_accent_color(&original, color, custom_color)?;
    if updated == original {
        return Ok(());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let temporary = parent.join(format!(
        ".enzo-config-{}-{}.tmp",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed),
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = (|| -> Result<()> {
        if let Ok(metadata) = fs::metadata(&path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(updated.as_bytes())?;
        file.sync_all()?;
        drop(file);
        // Avoid overwriting an edit made while preparing this save.
        match fs::read_to_string(&path) {
            Ok(current) if current != original => bail!("config changed during save; try again"),
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        fs::rename(&temporary, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.with_context(|| format!("failed to save config to {}", path.display()))
}

fn edit_accent_color(
    contents: &str,
    color: Option<[u8; 3]>,
    custom_color: Option<[u8; 3]>,
) -> Result<String> {
    // Never replace malformed config with the defaults used for playback.
    Config::from_str(contents).context("fix the invalid config before saving settings")?;
    let mut document = contents.parse::<DocumentMut>()?;
    if let Some(color) = color {
        set_color(&mut document, "accent_color", color);
    } else {
        document.remove("accent_color");
    }
    if let Some(color) = custom_color {
        set_color(&mut document, "custom_accent_color", color);
    }
    Ok(document.to_string())
}

fn set_color(document: &mut DocumentMut, key: &str, [r, g, b]: [u8; 3]) {
    let hex = format!("#{r:02x}{g:02x}{b:02x}");
    if let Some(existing) = document.get_mut(key).and_then(|item| item.as_value_mut()) {
        let decor = existing.decor().clone();
        *existing = Value::from(hex);
        *existing.decor_mut() = decor;
    } else {
        document[key] = value(hex);
    }
}

#[cfg(test)]
#[path = "tests/config_saving.rs"]
mod tests;
