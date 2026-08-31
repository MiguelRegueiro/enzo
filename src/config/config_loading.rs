use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};

use super::config_definition::Config;

impl Config {
    pub(crate) fn load(override_path: Option<&Path>) -> Result<Self> {
        let is_override = override_path.is_some();
        let Some(path) = override_path.map(Path::to_path_buf).or_else(config_path) else {
            return Ok(Self::default());
        };
        load_from_path(&path, is_override)
    }
}

fn load_from_path(path: &Path, is_override: bool) -> Result<Config> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if !is_override && error.kind() == io::ErrorKind::NotFound => {
            return Ok(Config::default());
        }
        Err(error) if is_override => {
            bail!("failed to read config from {}: {error}", path.display());
        }
        Err(error) => {
            eprintln!(
                "enzo: failed to read config from {}: {error}",
                path.display()
            );
            return Ok(Config::default());
        }
    };

    Ok(match Config::from_str(&contents) {
        Ok(config) => config,
        Err(error) => {
            eprintln!(
                "enzo: failed to load config from {}: {error}",
                path.display()
            );
            Config::default()
        }
    })
}

fn config_home() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        platform_config_home(
            env::var_os("XDG_CONFIG_HOME").as_deref().map(Path::new),
            dirs::home_dir().as_deref(),
        )
    }

    #[cfg(windows)]
    {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(dirs::config_dir)
    }

    #[cfg(not(any(unix, windows)))]
    {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(dirs::config_dir)
    }
}

#[cfg(unix)]
fn platform_config_home(xdg_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(xdg_home) = xdg_home {
        return Some(xdg_home.to_path_buf());
    }
    let home = home?;

    #[cfg(target_os = "macos")]
    {
        let xdg_home = home.join(".config");
        if xdg_home.join("enzo/config.toml").is_file() {
            return Some(xdg_home);
        }
        return Some(home.join("Library/Application Support"));
    }

    #[cfg(not(target_os = "macos"))]
    Some(home.join(".config"))
}

pub(crate) fn config_dir() -> Option<PathBuf> {
    config_home().map(|home| home.join("enzo"))
}

fn config_path() -> Option<PathBuf> {
    config_dir().map(|directory| directory.join("config.toml"))
}

#[cfg(test)]
#[path = "tests/config_loading.rs"]
mod tests;
