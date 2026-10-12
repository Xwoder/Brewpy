use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "brewpy",
    version,
    about = "切换 Homebrew 安装的 Python 版本",
    long_about = "brewpy 只负责发现、校验和配置 Homebrew 已安装的 Python；\n\
                  修改当前终端环境变量这件事由 shell 完成。\n\n\
                  首次使用：\n  \
                  echo 'eval \"$(brewpy init zsh)\"' >> ~/.zshrc\n\n\
                  常用命令：\n  \
                  brewpy list                     列出可用版本\n  \
                  brewpy current                  显示当前终端实际使用的 Python\n  \
                  brewpy default 3.14             设置持久化默认版本（影响新终端）\n  \
                  brewpy use 3.12                 只切换当前终端\n  \
                  brewpy reset                    撤销 brewpy 对本终端 PATH 的修改"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// 列出 Homebrew 已安装的 Python 版本
    List,

    /// 显示当前终端实际使用的 Python
    Current,

    /// 设置持久化默认版本（只改配置，不影响已打开的终端）
    Default {
        /// 版本号，形如 3.14
        version: Option<String>,

        /// 清除已设置的默认版本
        #[arg(long, conflicts_with = "version")]
        clear: bool,
    },

    /// 显示配置中的默认版本
    #[command(name = "default-show")]
    DefaultShow,

    /// 输出 shell 初始化代码：eval "$(brewpy init zsh)"
    Init {
        /// zsh 或 bash
        shell: String,
    },

    /// 输出切换当前终端所需的代码：eval "$(brewpy activate 3.12)"
    Activate {
        /// 版本号；与 --reset 二选一
        version: Option<String>,

        /// 撤销 brewpy 注入的 PATH
        #[arg(long)]
        reset: bool,
    },

    /// 撤销 brewpy 对当前终端 PATH 的修改：eval "$(brewpy reset)"
    Reset,

    /// 切换当前终端的版本（需要先完成 shell 集成）
    Use {
        /// 版本号，形如 3.12
        version: String,
    },
}
