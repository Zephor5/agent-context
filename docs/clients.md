# 接入 agent 工具

每个 agent 工具要做两件事：注册 MCP 服务，在全局指令里加入使用规则。每个工具只需配置一次，之后在这个工具的任意项目、任意会话里，都能写入和检索同一个 vault。

## 1. 注册 MCP 服务

| 设置项 | 取值 |
|---|---|
| 传输方式 | stdio |
| 命令 | 程序的绝对路径（`command -v agent-context` 的输出，通常是展开后的 `~/.cargo/bin/agent-context`）。agent 工具，尤其是图形界面应用，常常不继承 shell 的 PATH。 |
| 参数 | `serve` |
| 环境变量 | `AGENT_CONTEXT_ROOT=<vault 的绝对路径>`。不会展开 `~`。 |
| 作用范围 | 用户级 / 全局，不要按项目配置。目的就是在任何地方工作时都能写入。 |
| 审批 | 可选。这些工具只读取 vault、向 `raw/` 追加、写 `board/`，设成免审批是合理的。有些工具在非交互运行时会直接拒绝未审批的 MCP 调用。 |

## 2. 加上全局使用规则

将 [客户端指令模板](../templates/agent-context.md) 加入工具的用户级 / 全局指令；已有 `agent-context` 段落时替换该段，保留其他指令。

模板包含三类规则：

- **主动检索**：接手项目任务时先搜索，对相关结果读全文，并核对适用范围。
- **自动沉淀**：采用该模板即授权 agent 对有复用价值的已验证结论自动 capture；写入前查重，记录证据和适用范围。
- **黑板协作**：使用共享资源或接手跨工具工作前查看黑板，随状态变化更新，完成且资源释放后清理。

可按个人偏好调整模板。`templates/vault/AGENTS.md` 是维护员在 vault 内使用的规则，不是客户端全局指令。

## 3. 检查

在工具里开一个新会话，让它用 wiki 里的某个词调用一次 `wiki_search`，再调用一次 `board_list`。这两个都是只读的。

不要在真实 vault 上测试 `wiki_capture`、`board_put` 或 `board_edit`：raw 记录只追加不修改，测试记录会永久留下。要试写入，先用 `agent-context init <临时目录>` 建一个临时 vault，测试时把 `AGENT_CONTEXT_ROOT` 指向它。

## 参考配置

把 `/abs/agent-context` 换成程序路径，`/abs/vault` 换成 vault 路径。Codex CLI 和 Kiro CLI 已经完整实测过；Claude Code 和 Cursor 的配置是按各自官方文档写的，没有实测。

**Claude Code**：全局指令写在 `~/.claude/CLAUDE.md`

```sh
claude mcp add --transport stdio --scope user \
  --env AGENT_CONTEXT_ROOT=/abs/vault \
  agent-context -- /abs/agent-context serve
```

**Codex CLI**：配置文件 `~/.codex/config.toml`；全局指令写在 `~/.codex/AGENTS.md`

```toml
[mcp_servers.agent-context]
command = "/abs/agent-context"
args = ["serve"]
# 不加这行，每次调用都要人工确认，非交互的 codex exec 会直接拒绝调用。
default_tools_approval_mode = "approve"

[mcp_servers.agent-context.env]
AGENT_CONTEXT_ROOT = "/abs/vault"
```

**Kiro CLI**：全局指令写入 `~/.kiro/steering/agent-context.md`

```sh
kiro-cli mcp add --name agent-context --scope global \
  --command /abs/agent-context --args serve \
  --env AGENT_CONTEXT_ROOT=/abs/vault
```

设置了 `includeMcpJson: false` 的 agent 配置不会加载全局 MCP 配置，要在这个配置的 `mcpServers` 里再加一次。

**Cursor**：配置文件 `~/.cursor/mcp.json`；全局指令在 Cursor Settings → Rules → User Rules

```json
{
  "mcpServers": {
    "agent-context": {
      "command": "/abs/agent-context",
      "args": ["serve"],
      "env": { "AGENT_CONTEXT_ROOT": "/abs/vault" }
    }
  }
}
```

其他工具按第 1 步的设置项，对应到它自己的 MCP 配置即可。

## 交给 agent 来配

把下面这段填好占位符，粘贴到要接入的工具里：

```text
按照 <agent-context 仓库路径>/docs/clients.md，把 agent-context MCP 服务接入当前这个工具。
程序路径：<`command -v agent-context` 的输出>。vault：<vault 的绝对路径>。
在用户级/全局范围注册，将 templates/agent-context.md 的内容加入这个工具的全局指令；已有 agent-context 段落时替换该段，保留其他指令。然后告诉我改了哪些地方。
只用 wiki_search 和 board_list 做检查；不要对这个 vault 调用 wiki_capture、board_put 或 board_edit。
```

配置完重启工具（或开一个新会话），让它加载这个服务。
