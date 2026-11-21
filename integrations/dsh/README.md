# Agent Context · 个人知识库

`@zephor/dsh-agent-context` 让 DSH 会话通过本机 MCP 服务检索 wiki、追加记录和共享黑板。安装包随附预编译的 `agent-context`，覆盖 macOS（Apple Silicon、Intel）、Linux（x64、ARM64）和 Windows x64。用户无需安装 Rust 或手动启动 CLI。

## 在 DSH 中使用

在「插件」中安装并启用组合包。首发前可通过「添加插件」选择构建产出的本地 `.tgz`；发布后可直接按包名安装。按宿主提示重启或开新会话，然后试一句「列出本机黑板条目」。新知识库的列表可能为空。会话可通过 MCP 工具读写所选知识库。

初次使用会自动初始化本人的知识库。macOS 和 Linux 默认路径是 `~/.local/share/agent-context/vault`，Windows 默认路径是 `%LOCALAPPDATA%/agent-context/vault`；设置了绝对路径 `XDG_DATA_HOME` 时使用其下的 `agent-context/vault`。重启、升级或从其他 DSH profile 启用时都会复用该目录。缺少 Git 时，内置 CLI 仍可初始化 wiki 和黑板；需要提交历史时再安装 Git 并初始化仓库。

## 在 DSH 里打开工作台

点击 DSH 左侧的 **Agent Context**，在主区域查看或编辑当前知识库。若这个 Host 启用了多个 agent-context 实例，可在页面上选择服务名。黑板、Wiki、原始记录、Markdown 预览和单页 Git 提交与独立 Web 使用同一套页面和接口。离开侧栏页面再回来时，当前 DSH 客户端会保留该库的编辑草稿；刷新整个 DSH 窗口会清除内存草稿。

工作台在首次请求时通过包内 CLI 的 `web --stdio` 通道读取当前 vault，由 DSH Host 校验访问者。它不占用本机 Web 端口，也不改动独立 Web 的根目录配置。关掉插件会关闭该通道。已有 CLI 可以继续提供 MCP 服务。单页 Git 提交需要本机有 Git 且知识库已经初始化 Git 仓库。

## 已有安装与配置

- **可执行文件**依次查找显式 `binaryPath`、PATH 中的绝对目录、Cargo bin 目录、包内当前平台的程序。自定义路径须可执行。
- **知识库**依次采用显式 `vaultRoot`、环境变量 `AGENT_CONTEXT_ROOT`、`agent-context web` 记住的目录、已有的 `~/agent-context` vault、上述默认目录。旧库会原地打开。其他位置的旧库可以用绝对路径指定。
- 显式配置或 Web 记录指向无效目录时，修正路径后重新启用。已有的非 vault 目录会受到保护。
- 旧本地包 `dsh-agent-context` 用户先记录自定义路径，再停用旧组合包、启用新版。保留已有条目 ID `mcp-agent-context` 和服务名 `agent-context`。若 profile 覆盖项指定了旧模块 `name`，把该字段改为 `"@zephor/dsh-agent-context"`。已有同名手工 MCP 注册时先移除重复注册，保留原 vault。
- 多个 vault 可以用不同条目 ID、`serverName` 和 `vaultRoot` 挂载。卸载插件会撤回其 MCP 注册，知识库保留在原处。

需要自定义时，在当前 profile 的 `cordis.patch.yml` 覆盖已启用组合包的配置。例如：

```yaml
- id: mcp-agent-context
  config:
    vaultRoot: /absolute/path/to/my-vault
    binaryPath: /absolute/path/to/agent-context
```

两个路径都可留空。要查看工具，打开 DSH 的 MCP 设置检查 `agent-context` 的连接状态。接口和维护流程见[项目说明](https://github.com/Zephor5/agent-context#readme)。

## 开发与打包

运行 `npm test` 检查配置选择、注册和卸载。仓库中的 **Build DSH plugin** 工作流在各平台从同一份 `Cargo.lock` 编译 release 程序，验证首次启动和 MCP 握手，再将五个程序放进同一个 npm 安装包。下载 `dsh-agent-context-npm` artifact，解压即可获得 DSH 可安装的 `.tgz`。工作流仅构建测试包。正式打包要求五个平台的文件齐全。

本平台开发时，在仓库根目录执行 `cargo build --locked --release`，将 `target/release/agent-context`（Windows 为 `agent-context.exe`）放到本插件的 `native/<platform>-<arch>/`。进入插件目录运行 `npm test`，运行 `npm run build:web` 后可用 `npm pack --ignore-scripts` 生成仅用于本机试装的单平台包。

[MIT 许可证](LICENSE)
