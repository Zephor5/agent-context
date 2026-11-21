# Agent Context · 个人知识库

`@zephor/dsh-agent-context` 让 DSH 会话通过本机 MCP 服务检索 wiki、追加记录和共享黑板。安装包随附预编译的 `agent-context`，覆盖 macOS（Apple Silicon、Intel）、Linux（x64、ARM64）和 Windows x64。用户无需安装 Rust 或手动启动 CLI。

## 在 DSH 中使用

在「插件」中安装并启用组合包。可在「发现」中选择 **Agent Context · 个人知识库**，或在「添加插件」中输入 `@zephor/dsh-agent-context`。按宿主提示重启或开新会话，然后试一句「列出本机黑板条目」。新知识库的列表可能为空。会话可通过 MCP 工具读写所选知识库。

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

CLI 和插件独立发版。插件清单中的 `agentContext.cliVersion` 是随包分发的精确 CLI 版本。普通代码提交不会发布；在同一次推送包含多个提交时，工作流比较推送前后的版本。

| 合入 main 的修改 | 自动执行 |
| --- | --- |
| 提高根目录 `Cargo.toml` 的 CLI 版本（同步 `Cargo.lock`） | 构建并测试五个平台，将可执行文件发布到本仓库的 `cli-v<版本>` GitHub Release。 |
| 提高 `integrations/dsh/package.json` 的插件版本 | 下载并校验 `agentContext.cliVersion` 指定的五个可执行文件，测试插件、打包并发布到公共 npm。 |
| 同时提高两个版本 | 等 CLI Release 成功后，插件按自己的版本引用取包。若要采用新 CLI，请一并更新 `agentContext.cliVersion`。 |

CLI `0.1.0` 的五个平台产物已随插件 `0.2.1` 首发。该版本直接从 SHA-512 已固定的原始 npm 归档提取；以后的 CLI 版本从 GitHub Release 下载并核对各资产的 SHA-256。下载失败或校验不通过会中止插件发版。

插件通过 OIDC 发布。在 npm 将本包的 Trusted Publisher 设为 GitHub 用户 `Zephor5`、仓库 `agent-context`、工作流 `package-dsh.yml`，允许 `npm publish`，环境名留空。正式版本使用 `latest`，预发布使用 `next`。发布后，`refresh-index` 作业从公共 npm 匿名下载该版本，核对 SHA-512 与已测试的归档一致。正式版可下载且 `latest` 已更新后，作业主动触发聚合仓库的 `plugin-index.yml`。预发布只验证下载。若 npm 可见性暂时滞后或通知失败，可在 Actions 中重跑失败作业。

跨仓通知使用本仓库的 Actions secret `DSH_INDEX_DISPATCH_TOKEN`。令牌限定目标仓库 `Zephor5/dsh-plugins`，授予 Actions 的读写权限，到期前需轮换。也可以在聚合仓库手动刷新索引。

本地打通正式打包流程可在本目录依次执行 `npm run fetch:native`、`npm test` 和 `npm pack`。手动运行 GitHub 工作流也会产生可下载的 `dsh-agent-context-npm` 归档。两种方式都复用指定 CLI 版本。

本平台开发时，在仓库根目录执行 `cargo build --locked --release`，将 `target/release/agent-context`（Windows 为 `agent-context.exe`）放到本插件的 `native/<platform>-<arch>/`。进入插件目录运行 `npm test`，运行 `npm run build:web` 后可用 `npm pack --ignore-scripts` 生成仅用于本机试装的单平台包。

[MIT 许可证](LICENSE)
