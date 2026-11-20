# dsh-agent-context

把 agent-context 接入 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)（DSH）的 Host 插件：向宿主的 `mcpManager` 注册一个 stdio MCP 服务（`agent-context serve`），让 DSH 会话和本机其他 agent 工具共用同一份个人 wiki vault 和黑板。

插件本身不实现任何工具，工具来自 agent-context 自己的 MCP 服务：`wiki_capture` / `wiki_search` / `wiki_read` 和 `board_*`（见仓库根 README）。

## 前置条件

- DSH Desktop **0.2.2-rc.3 或更新**（`mcpManager` 服务、`dsh.bundle.patch` 装载方式），Node **>= 24**。
- 已安装 `agent-context`（`cargo install --locked --path .`，默认装到 `~/.cargo/bin`）。
- 已有一个 vault（`agent-context init <目录>`）。插件不会替你创建。

## 配置项

| 配置项 | 默认值 | 说明 |
|---|---|---|
| `serverName` | `agent-context` | 注册到宿主的 MCP 服务名，必须匹配 `^[A-Za-z0-9_-]{1,32}$`。 |
| `vaultRoot` | 空 | vault 的**绝对路径**，作为 `AGENT_CONTEXT_ROOT` 传给服务。不展开 `~`。 |
| `binaryPath` | 空 | `agent-context` 可执行文件的**绝对路径**。留空则自动查找。 |

解析规则，三条都是“失败就不生效”，不猜：

- **`binaryPath` 显式配置时**：必须是绝对路径且是本进程可执行的普通文件，否则报错，**不回退**到自动查找。你指定了哪个文件，就只用那个文件。
- **`binaryPath` 留空时**：依次查找 `PATH` 中的**绝对**条目（相对条目和空条目会跳过，它们依赖宿主的工作目录），再查找 `~/.cargo/bin`。查找只用 `fs` 探测，不启动 shell，所以不受登录 shell 的 profile 脚本影响。找不到就只记一条告警，不注册。
- **`vaultRoot` 必须显式配置**：绝对路径、已存在的目录，且含 `raw/` 和 `wiki/` 两个子目录——与 `src/vault.rs` 里 `Vault::open` 的判定一致。留空时插件什么都不做（只记一条 info），配错时直接报错，让它在插件页面上可见。

## 本地安装

插件尚未发布。可以从仓库打包，再通过 DSH 的 **插件 → 添加插件** 安装生成的 `.tgz`（也可用插件管理工具安装该文件的绝对路径）：

```sh
cd <agent-context 仓库>/integrations/dsh
npm pack
```

用打包文件安装会解析 Host 入口所需的依赖；直接链接源码目录时，需要在目录中先准备好依赖。启用 bundle 后，在当前 profile 的用户 `cordis.patch.yml` 中按 id 覆盖配置（不要再插入同一个插件）：

```yaml
- id: mcp-agent-context
  config:
    vaultRoot: /绝对路径/my-context
    binaryPath: /绝对路径/agent-context # 可省略：自动查 PATH 和 ~/.cargo/bin
```

如果配置没有实时生效，重启 DSH。确认 MCP 已连接后开新会话，试 `wiki_search` 等只读工具。不要拿真实 vault 测试写入工具。

DSH 会读取 MCP 服务自带的检索和协作指令；插件不会注入自动记录授权。只有用户明确授权后才应添加这类全局指令。

## 复用

- **换一台机器**：把 tarball 或 `integrations/dsh/` 目录拷过去，重复上面的安装步骤。包里没有写死任何本机路径，所有路径都来自配置项。
- **多个 profile**（如 `desktop` 和 `web`）：每个 profile 有各自的 `node_modules`，逐个安装，各自配置。
- **多个 vault**：装一次即可，用不同 `serverName` + 不同 `vaultRoot` 插入多个实例（`id` 也要不同）。

## 边界

插件刻意不做这些事：

- 不创建 vault，不执行 `agent-context init`，不编译、不下载、不安装任何东西。
- 不向 vault 写入任何内容，不主动发起提示或询问。
- 不启动 shell 做路径解析，不读写 DSH profile 以外的配置，不修改你的 profile。
- 除 `AGENT_CONTEXT_ROOT` 外不向子进程注入环境变量（其余环境由宿主提供）。

注意一点：接上之后 vault 的内容可以进入会话上下文，也能被会话追加记录。vault 里放什么、给哪个会话用，由你决定。

## 测试

```sh
cd <agent-context 仓库>/integrations/dsh
npm test
```

用 `node:test`，不需要安装依赖：被测的 `lib/resolve.js` 和 `lib/mount.js` 只依赖 `node:*`，宿主交互在测试里用假的 cordis context（记录 `mcpManager.register` 的入参、`ctx.effect` 收到的 disposer）。`lib/index.js` 只放 loader 元数据和 schemastery 的 `Config`，需要在装好依赖的 profile 里才能导入。

以 root 身份运行时，`access(X_OK)` 对任意文件都返回成功，相关用例会自动跳过；Windows 上执行位无意义的用例同样跳过。

## 公开发布清单

发布前要确认的事，按顺序：

1. 本包采用 [MIT 许可证](LICENSE)。首次正式发布到 npm 前，确认包名、作用域及发布权限，再移除 `package.json` 的 `private` 标记。
2. 补充英文 README；中英文展示信息见 `locale/`。
3. 如需进入 DSH 插件市场，按目标市场的要求准备元数据。
4. 核对 `engines.dsh` 声明的兼容范围。
5. 在干净的 profile 中测试启用、停用、重载和修改配置，并在 CI 中运行 `npm test`。
6. 发布说明应说明会话可以读取和写入指定 vault。

## 验证范围

已对照 DSH Desktop 0.2.2-rc.3 的 `mcpManager.register()` 接口；未在实际 DSH profile 里安装或加载。单元测试与临时 vault 的 CLI stdio 只读测试不代表 DSH 内端到端已验证。
