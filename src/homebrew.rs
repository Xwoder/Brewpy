use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// 一个由 Homebrew 安装的 Python 解释器。
///
/// 只保存 `opt/` 下的稳定路径，绝不保存 `Cellar/` 路径：后者包含补丁版本号，
/// `brew upgrade` 之后目录名会变。
#[derive(Debug, Clone)]
pub struct Python {
    /// 形如 `3.12`
    pub version: String,
    /// 需要加入 PATH 的目录，形如 `<prefix>/libexec/bin`
    pub bin_dir: PathBuf,
    /// 是否安装了对应的 `python-tk@X.Y`
    pub tk: bool,
}

impl Python {
    pub fn python3(&self) -> PathBuf {
        self.bin_dir.join("python3")
    }

    /// 解析符号链接后的真实可执行文件，用于判断某个 `python3` 是否来自本公式。
    pub fn real_python3(&self) -> Option<PathBuf> {
        fs::canonicalize(self.python3()).ok()
    }
}

/// `brew --prefix`
pub fn brew_prefix() -> Result<PathBuf> {
    let out = Command::new("brew")
        .arg("--prefix")
        .output()
        .context("无法执行 `brew`，请确认 Homebrew 已安装且在当前 PATH 中")?;

    if !out.status.success() {
        bail!(
            "`brew --prefix` 执行失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    let text = String::from_utf8(out.stdout).context("`brew --prefix` 输出不是合法 UTF-8")?;
    let text = text.trim();
    if text.is_empty() {
        bail!("`brew --prefix` 返回空路径");
    }
    Ok(PathBuf::from(text))
}

/// 扫描 `<prefix>/opt` 的结果。
#[derive(Debug, Default)]
pub struct Scan {
    /// 有可用 `python3` 的解释器，按版本排序
    pub usable: Vec<Python>,
    /// 公式目录存在但缺少 `python3` 的版本。
    ///
    /// 实测：预发布版的 `python@3.15` 只提供 `python` 不提供 `python3`，
    /// 把它加进 PATH 会导致 `python3` 仍然指向别处，所以必须排除并告知用户。
    pub unusable: Vec<String>,
}

/// 所有可用的 Homebrew Python 解释器。
pub fn installed() -> Result<Vec<Python>> {
    Ok(scan()?.usable)
}

/// 扫描 `<prefix>/opt` 找出所有 Homebrew Python 解释器。
///
/// 判据是「`<prefix>/libexec/bin/python3` 是否存在且可执行」，而不是对公式名做
/// 黑名单过滤。这样 `python-tk@3.12`、`python-gdbm@3.15` 这类不含解释器的公式
/// 会被自动排除，将来 Homebrew 新增同类公式也不需要改代码。
pub fn scan() -> Result<Scan> {
    let prefix = brew_prefix()?;
    let opt_dir = prefix.join("opt");

    if !opt_dir.is_dir() {
        return Ok(Scan::default());
    }

    let mut usable = Vec::new();
    let mut unusable = Vec::new();

    for entry in fs::read_dir(&opt_dir)
        .with_context(|| format!("无法读取 Homebrew 的 opt 目录：{}", opt_dir.display()))?
    {
        let entry = entry.context("读取 opt 目录条目失败")?;
        let name = entry.file_name().to_string_lossy().into_owned();

        let Some(version) = parse_formula_version(&name) else {
            continue;
        };

        let formula_prefix = entry.path();
        let bin_dir = formula_prefix.join("libexec").join("bin");

        // 关键校验：必须真的有一个 python3 可执行文件。
        if !is_executable(&bin_dir.join("python3")) {
            unusable.push(version);
            continue;
        }

        let tk = opt_dir
            .join(format!("python-tk@{version}"))
            .join("libexec")
            .is_dir();

        usable.push(Python {
            version,
            bin_dir,
            tk,
        });
    }

    usable.sort_by_key(|p| version_key(&p.version));
    unusable.sort_by_key(|v| version_key(v));
    Ok(Scan { usable, unusable })
}

/// 按版本号查找，找不到就报错。
pub fn find(pythons: &[Python], version: &str) -> Result<Python> {
    let wanted = normalize_version(version)?;
    pythons
        .iter()
        .find(|p| p.version == wanted)
        .cloned()
        .with_context(|| {
            format!("Homebrew 未安装可用的 python@{wanted}，用 `brewpy list` 查看可用版本")
        })
}

/// 当前 PATH 下实际生效的 `python3`。
#[derive(Debug)]
pub struct Current {
    pub python3: Option<PathBuf>,
    pub version: Option<String>,
    /// 若该 `python3` 来自某个 Homebrew 公式，则为对应的解释器信息。
    pub python: Option<Python>,
}

pub fn current(pythons: &[Python]) -> Result<Current> {
    let python3 = which("python3");

    let version = python3
        .as_ref()
        .and_then(|p| query_version(p).ok());

    let python = python3
        .as_ref()
        .and_then(|p| fs::canonicalize(p).ok())
        .and_then(|real| {
            pythons
                .iter()
                .find(|p| p.real_python3().as_deref() == Some(real.as_path()))
                .cloned()
        });

    Ok(Current {
        python3,
        version,
        python,
    })
}

/// 在 PATH 中查找可执行文件（只依赖 PATH 顺序，不依赖任何 shell 内建命令）。
pub fn which(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn query_version(python3: &Path) -> Result<String> {
    let out = Command::new(python3)
        .arg("-c")
        .arg("import sys; print('%d.%d' % sys.version_info[:2])")
        .output()
        .with_context(|| format!("无法执行 {}", python3.display()))?;

    if !out.status.success() {
        bail!(
            "{} 返回非零退出码：{}",
            python3.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    let text = String::from_utf8(out.stdout)?;
    normalize_version(text.trim())
}

fn is_executable(path: &Path) -> bool {
    path.is_file()
        && path
            .metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

/// 从公式名中提取版本号。`python@3.12` -> `3.12`；其余返回 None。
fn parse_formula_version(name: &str) -> Option<String> {
    let rest = name.strip_prefix("python@")?;
    normalize_version(rest).ok()
}

/// 规范化版本号：接受 `3.12` 或 `3.12.15`，统一输出 `MAJOR.MINOR`。
pub fn normalize_version(input: &str) -> Result<String> {
    let text = input.trim();
    let segments: Vec<&str> = text.split('.').collect();

    if segments.len() < 2 {
        bail!("版本号格式无效：{text:?}，期望形如 3.12");
    }
    for segment in &segments {
        if segment.is_empty() || !segment.chars().all(|c| c.is_ascii_digit()) {
            bail!("版本号格式无效：{text:?}，期望形如 3.12");
        }
    }

    Ok(format!("{}.{}", segments[0], segments[1]))
}

fn version_key(version: &str) -> (u64, u64) {
    let mut parts = version.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_major_minor() {
        assert_eq!(normalize_version("3.12").unwrap(), "3.12");
        assert_eq!(normalize_version(" 3.14 ").unwrap(), "3.14");
    }

    #[test]
    fn normalize_trims_patch_level() {
        assert_eq!(normalize_version("3.12.15").unwrap(), "3.12");
    }

    #[test]
    fn normalize_rejects_garbage() {
        assert!(normalize_version("3").is_err());
        assert!(normalize_version("3.").is_err());
        assert!(normalize_version("3.x").is_err());
        assert!(normalize_version("").is_err());
        assert!(normalize_version("python@3.12").is_err());
    }

    #[test]
    fn only_python_formulae_are_recognized() {
        assert_eq!(parse_formula_version("python@3.12").unwrap(), "3.12");
        // 这些公式不含解释器，靠名字无法区分，要靠 libexec/bin/python3 是否存在。
        assert_eq!(parse_formula_version("python-tk@3.12"), None);
        assert_eq!(parse_formula_version("python-gdbm@3.15"), None);
        assert_eq!(parse_formula_version("openssl@3"), None);
        assert_eq!(parse_formula_version("python"), None);
    }

    #[test]
    fn versions_sort_numerically() {
        let mut versions = vec!["3.9".to_string(), "3.14".to_string(), "3.2".to_string()];
        versions.sort_by_key(|v| version_key(v));
        assert_eq!(versions, vec!["3.2", "3.9", "3.14"]);
    }

    #[test]
    fn find_requires_installed_version() {
        let installed = vec![Python {
            version: "3.12".into(),
            bin_dir: PathBuf::from("/opt/homebrew/opt/python@3.12/libexec/bin"),
            tk: false,
        }];

        assert_eq!(find(&installed, "3.12").unwrap().version, "3.12");
        // 补丁版本号也应能命中
        assert_eq!(find(&installed, "3.12.1").unwrap().version, "3.12");
        assert!(find(&installed, "3.11").is_err());
    }
}
