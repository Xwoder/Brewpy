use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// 持久化的默认版本，形如 `3.14`
    pub default: Option<String>,
}

/// `~/.config/brewpy/config.toml`（尊重 `XDG_CONFIG_HOME`）。
pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("无法获取配置目录（HOME 未设置？）")?;
    Ok(base.join("brewpy").join("config.toml"))
}

pub fn load() -> Result<Config> {
    let path = config_path()?;

    if !path.exists() {
        return Ok(Config::default());
    }

    let content =
        fs::read_to_string(&path).with_context(|| format!("无法读取 {}", path.display()))?;

    toml::from_str(&content).with_context(|| format!("配置文件格式错误：{}", path.display()))
}

/// 原子写入：先写临时文件再 rename，避免中断时留下半截配置。
pub fn save(config: &Config) -> Result<()> {
    let path = config_path()?;
    let parent = path.parent().context("配置文件没有父目录")?;
    fs::create_dir_all(parent).with_context(|| format!("无法创建目录 {}", parent.display()))?;

    let tmp = path.with_extension("toml.tmp");
    let content = toml::to_string_pretty(config).context("序列化配置失败")?;

    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .with_context(|| format!("无法创建临时文件 {}", tmp.display()))?;

        file.write_all(content.as_bytes())
            .with_context(|| format!("无法写入 {}", tmp.display()))?;
        file.flush()?;
    }

    fs::rename(&tmp, &path).with_context(|| format!("无法保存 {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_round_trips() {
        let text = toml::to_string_pretty(&Config::default()).unwrap();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert!(parsed.default.is_none());
    }

    #[test]
    fn default_version_round_trips() {
        let cfg = Config {
            default: Some("3.14".into()),
        };
        let text = toml::to_string_pretty(&cfg).unwrap();
        assert!(text.contains("3.14"));
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed.default.as_deref(), Some("3.14"));
    }

    #[test]
    fn config_path_lives_under_config_dir() {
        let path = config_path().unwrap();
        assert!(path.ends_with("brewpy/config.toml"));
    }
}
