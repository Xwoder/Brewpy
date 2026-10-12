mod cli;
mod config;
mod homebrew;
mod shell;

use anyhow::{Result, bail};
use clap::Parser;

use cli::Command;
use homebrew::Python;

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("brewpy: {err:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = cli::Cli::parse();

    match cli.command {
        Command::List => cmd_list(),
        Command::Current => cmd_current(),
        Command::Default { version, clear } => cmd_default(version.as_deref(), clear),
        Command::DefaultShow => cmd_default_show(),
        Command::Init { shell } => cmd_init(&shell),
        Command::Activate { version, reset } => cmd_activate(version.as_deref(), reset),
        Command::Reset => cmd_reset(),
        Command::Use { version } => cmd_use(&version),
    }
}

fn cmd_list() -> Result<()> {
    let prefix = homebrew::brew_prefix()?;
    let scan = homebrew::scan()?;
    let pythons = &scan.usable;

    if pythons.is_empty() {
        println!("未发现 Homebrew 安装的 Python。");
        println!("可用 brew install python@3.12 安装一个。");
        return Ok(());
    }

    let default = configured_default()?;

    println!("Homebrew prefix: {}", prefix.display());
    println!();

    for python in pythons {
        let is_default = default.as_deref() == Some(python.version.as_str());
        let mut tags = Vec::new();
        if is_default {
            tags.push("default");
        }
        if python.tk {
            tags.push("tk");
        }

        let tag = if tags.is_empty() {
            String::new()
        } else {
            format!("   [{}]", tags.join(", "))
        };

        println!(
            "{} {:<5}  {}{}",
            if is_default { "*" } else { " " },
            python.version,
            python.bin_dir.display(),
            tag
        );
    }

    println!();
    println!("* = 默认版本；用 `brewpy use 3.12` 切换当前终端，`brewpy default 3.12` 设置持久化默认。");

    if !scan.unusable.is_empty() {
        println!();
        println!(
            "已忽略：{}（缺少 libexec/bin/python3，加入 PATH 也不会生效）",
            scan.unusable.join("、")
        );
    }

    Ok(())
}

fn cmd_current() -> Result<()> {
    let pythons = homebrew::installed()?;
    let current = homebrew::current(&pythons)?;

    match (&current.python3, &current.version) {
        (Some(path), Some(version)) => {
            println!("python3:  {}", path.display());
            println!("version:  {version}");

            let source = match &current.python {
                Some(python) => format!("Homebrew python@{}", python.version),
                None => "非 Homebrew（系统 Python 或其它来源）".to_string(),
            };
            println!("source:   {source}");
        }
        _ => {
            println!("python3:  （未在 PATH 中找到）");
        }
    }

    let default = configured_default()?;
    println!(
        "default:  {}",
        default.as_deref().unwrap_or("（未设置）")
    );

    println!("active:   {}", active_in_shell(&pythons));

    Ok(())
}

fn cmd_default(version: Option<&str>, clear: bool) -> Result<()> {
    if clear {
        let mut cfg = config::load()?;
        cfg.default = None;
        config::save(&cfg)?;
        println!("已清除默认版本，新终端将不再被 brewpy 注入 PATH。");
        return Ok(());
    }

    let version = match version {
        Some(v) => v,
        None => bail!("请指定版本（`brewpy default 3.14`）或用 `--clear` 清除"),
    };

    let pythons = homebrew::installed()?;
    let python = homebrew::find(&pythons, version)?;

    if !python.python3().exists() {
        bail!(
            "python@{} 的 python3 不存在：{}",
            python.version,
            python.python3().display()
        );
    }

    let mut cfg = config::load().unwrap_or_default();
    cfg.default = Some(python.version.clone());
    config::save(&cfg)?;

    println!("默认版本已设为 {}（{}）", python.version, python.bin_dir.display());
    println!("当前终端不受影响；新开的终端会自动生效，或在当前终端执行 `brewpy use {}`。", python.version);
    Ok(())
}

fn cmd_default_show() -> Result<()> {
    let path = config::config_path()?;
    match configured_default()? {
        Some(version) => println!("{version}   ({})", path.display()),
        None => {
            println!("（未设置默认版本）  ({})", path.display());
            println!("用 `brewpy default 3.14` 设置一个。");
        }
    }
    Ok(())
}

fn cmd_init(shell_name: &str) -> Result<()> {
    let shell = shell::Shell::parse(shell_name)?;
    let pythons = homebrew::installed()?;

    let default_dir = match configured_default()? {
        Some(version) => match homebrew::find(&pythons, &version) {
            Ok(python) if python.python3().exists() => Some(python.bin_dir.clone()),
            Ok(_) => {
                eprintln!(
                    "brewpy: 默认版本 {version} 的 python3 已不存在，本次 init 跳过激活"
                );
                None
            }
            Err(err) => {
                eprintln!("brewpy: {err:#}；本次 init 跳过激活");
                None
            }
        },
        None => None,
    };

    print!("{}", shell::init_snippet(shell, default_dir.as_deref()));
    Ok(())
}

fn cmd_activate(version: Option<&str>, reset: bool) -> Result<()> {
    if reset {
        print!("{}", shell::reset_snippet());
        return Ok(());
    }

    let version = match version {
        Some(v) => v,
        None => bail!("请指定版本（`brewpy activate 3.12`）或使用 `--reset`"),
    };

    let pythons = homebrew::installed()?;
    let python = homebrew::find(&pythons, version)?;

    print!("{}", shell::activate_snippet(&python.bin_dir));
    Ok(())
}

fn cmd_reset() -> Result<()> {
    print!("{}", shell::reset_snippet());
    Ok(())
}

/// 只有在 shell 集成缺失时才会走到这里——正常情况下 `brewpy()` 包装函数
/// 会拦截 `use` 并转发给 `activate`。
fn cmd_use(version: &str) -> Result<()> {
    bail!(
        "`brewpy use` 需要 shell 集成才能修改当前终端的 PATH（子进程无法改父 shell）。\n\
         请先执行：eval \"$(brewpy init zsh)\"\n\
         或直接：  eval \"$(brewpy activate {version})\""
    )
}

/// 配置中的默认版本（已规范化）。
fn configured_default() -> Result<Option<String>> {
    let cfg = config::load()?;
    cfg.default
        .as_deref()
        .map(homebrew::normalize_version)
        .transpose()
}

/// 本终端里 brewpy 实际激活的版本，来自 `BREWPY_ACTIVE_DIR`。
fn active_in_shell(pythons: &[Python]) -> String {
    let Some(active) = std::env::var_os("BREWPY_ACTIVE_DIR") else {
        return "（未激活）".to_string();
    };

    let active = std::path::PathBuf::from(active);
    match pythons.iter().find(|p| p.bin_dir == active) {
        Some(python) => python.version.clone(),
        None => format!("{}（不再是已安装的 Homebrew Python）", active.display()),
    }
}

