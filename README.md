# brewpy

用 Rust 写的 Homebrew Python 版本切换器：在 macOS 上，把 `python3` 指向你通过 `brew install python@X.Y` 装的那一个版本，而不是系统自带的那一个。

它只做一件事——**管理 PATH 里的那一个 `<prefix>/opt/python@X.Y/libexec/bin` 目录**。不创建虚拟环境，不接管 `pip`，不修改 Homebrew 的链接。

---

## 为什么需要它

Homebrew 把 `python@3.14` 的可执行文件放在 `libexec/bin` 下，并且默认**不**把它写进 PATH（这是 Homebrew 有意为之，避免覆盖系统 Python）。结果是：

- `brew install python@3.14` 之后敲 `python3`，拿到的可能还是 3.9 或者别的版本；
- 手动往 `~/.zshrc` 里加 `export PATH=...` 容易越加越多，切换版本时还得自己记得删掉旧的那条；
- `python-tk@3.12`、`python-gdbm@3.15` 这些公式名字里也有 `python`，用公式名做黑名单过滤迟早出错；
- 预发布版（比如 `python@3.15`）只提供 `python` 不提供 `python3`，把它加进 PATH 也不会生效——需要明确告诉你，而不是静默失败。

brewpy 就是为这几件事准备的。

---

## 安装

### 从源码

```bash
git clone https://github.com/Xwoder/Brewpy.git
cd Brewpy
cargo build --release
install -m 755 target/release/brewpy /usr/local/bin/brewpy
```

需要 Rust（edition 2024）和 Homebrew。

### 接入 shell

在 `~/.zshrc`（bash 用户用 `~/.bashrc`）末尾加一行：

```bash
eval "$(brewpy init zsh)"
```

重开一个终端即生效。这一步会定义三个 shell 函数，并把配置的默认版本注入 PATH——**不会**动你 `.zshrc` 里的其它内容。

---

## 用法

```bash
brewpy list              # 列出 Homebrew 已装的可用版本
brewpy current           # 当前终端实际生效的是哪个 python3
brewpy default 3.14      # 设置持久化默认版本（影响新开的终端）
brewpy use 3.12          # 只切换当前终端
brewpy reset             # 撤销 brewpy 对本终端 PATH 的修改
```

`brewpy list` 的真实输出：

```
Homebrew prefix: /opt/homebrew

  3.11   /opt/homebrew/opt/python@3.11/libexec/bin
  3.12   /opt/homebrew/opt/python@3.12/libexec/bin   [tk]
* 3.13   /opt/homebrew/opt/python@3.13/libexec/bin   [default, tk]
  3.14   /opt/homebrew/opt/python@3.14/libexec/bin   [tk]

* = 默认版本；用 `brewpy use 3.12` 切换当前终端，`brewpy default 3.12` 设置持久化默认。

已忽略：3.15（缺少 libexec/bin/python3，加入 PATH 也不会生效）
```

`brewpy current`：

```
python3:  /opt/homebrew/opt/python@3.13/libexec/bin/python3
version:  3.13
source:   Homebrew python@3.13
default:  3.13
active:   3.13
```

### 命令一览

| 命令 | 作用 |
| --- | --- |
| `brewpy list` | 列出所有**可用**的解释器，标注 `default` / `tk`，并单独列出被忽略的版本 |
| `brewpy current` | 当前终端的 `python3` 路径、版本、来源（是否为 Homebrew）、配置默认、本次激活版本 |
| `brewpy default <X.Y>` | 写入持久化默认版本。**不影响已经打开的终端** |
| `brewpy default --clear` | 清除默认版本，新终端不再被注入 PATH |
| `brewpy default-show` | 打印配置里的默认版本及配置文件路径 |
| `brewpy use <X.Y>` | 只切换当前终端（需要 shell 集成） |
| `brewpy reset` | 移除 brewpy 注入过的所有路径，清空 `BREWPY_ACTIVE_DIR` |
| `brewpy init <zsh\|bash>` | 输出 shell 初始化代码，供 `eval` 使用 |
| `brewpy activate <X.Y>` | 输出切换代码，供 `eval` 使用（shell 集成内部调用） |

版本号接受 `3.14` 和 `3.14.2` 两种写法，一律规范化为 `MAJOR.MINOR`。

---

## 工作原理

子进程无法修改父 shell 的环境变量，所以 brewpy 把职责拆开：

- **Rust 侧**负责发现、校验、配置：跑 `brew --prefix`，扫描 `<prefix>/opt`，校验 `libexec/bin/python3` 是否真的存在且可执行，读写配置文件，然后把一段 shell 代码打到 stdout；
- **shell 侧**负责改环境：`eval` 这段代码，把目标目录前置插入 `PATH`，`export BREWPY_ACTIVE_DIR`，`hash -r` 清掉命令缓存。

```
终端启动
  └─ zshrc 执行 eval "$(brewpy init zsh)"
       ├─ 读 ~/.config/brewpy/config.toml → default = 3.14
       ├─ brew --prefix → /opt/homebrew
       ├─ 扫描 opt，校验 libexec/bin/python3 可执行
       │     └─ 找不到或不合法 → stderr 警告，跳过激活，不注入 PATH
       └─ 打印 activate 代码 → 定义辅助函数 → PATH 前置插入 → export BREWPY_ACTIVE_DIR → hash -r
```

`brewpy()` 这个包装函数会把 `use` / `reset` 转发成 `eval "$(brewpy activate ...)"`，其余子命令直接交给真正的可执行文件。如果没做 shell 集成就敲 `brewpy use`，会直接报错并提示你该执行什么。

### 几个设计取舍

- **只保存 `opt/` 路径，绝不保存 `Cellar/` 路径。** `Cellar` 目录名带补丁版本号，`brew upgrade` 之后就失效了；`opt/python@X.Y` 是稳定符号链接。
- **判据是「有没有可执行的 `python3`」，不是公式名黑名单。** 这样 `python-tk@3.12`、`python-gdbm@3.15` 会被自动排除，将来 Homebrew 新增同类公式也不需要改代码。检测不到 `python3` 的版本会被列进 `已忽略` 并说明原因。
- **激活前先清理。** PATH 前置插入之前会先剔除所有已注入的 `*/opt/python@*/libexec/bin`，切换版本不会留下历史残留，`brew upgrade` 也不用担心旧路径堆积。
- **`activate` 的输出是自包含的。** 即使没跑过 `init`，`eval "$(brewpy activate 3.12)"` 也能工作；辅助函数运行时自探测 shell（zsh 用 `path` 数组，bash 用 `IFS=:` 切分），不依赖可能不可靠的 `$SHELL`。
- **配置文件原子写入**：先写临时文件（权限 `0600`）再 `rename`，中断时不会留下半截 TOML。

---

## 配置

配置文件在 `~/.config/brewpy/config.toml`（遵循 `XDG_CONFIG_HOME`），内容就一行：

```toml
default = "3.13"
```

也可以直接用 `brewpy default <版本>` / `brewpy default --clear` 来改，不用手动编辑。

---

## 开发

```bash
cargo build          # 构建
cargo test           # 单元测试（版本号规范化、扫描判据、shell 代码片段等）
cargo run -- list    # 直接跑
```

代码结构：

```
src/
  main.rs      子命令实现与输出
  cli.rs       clap 命令行定义
  config.rs    config.toml 的读写（原子写入）
  homebrew.rs  brew --prefix、opt 扫描、版本规范化、当前 python3 探测
  shell.rs     生成 zsh/bash 的辅助函数与包装函数
```

会话内状态只有一个环境变量：`BREWPY_ACTIVE_DIR`，记录当前终端被注入的目录。

---

## 已知限制

- 只支持 zsh 和 bash（fish 等非 POSIX shell 暂不支持）。
- 只处理 Homebrew 安装的 Python：pyenv、conda、系统 Python 不在管理范围内，`brewpy current` 会把它们标成「非 Homebrew」。
- 只切 `python3` 的 PATH，不创建或管理虚拟环境——`python -m venv` 该怎么做还是怎么做。
