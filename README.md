# agent-context

在不同 agent 工具、不同会话之间共享上下文：一份由 agent 写入、由维护员整理的个人知识 wiki，加上一块本机黑板，记录进行中的工作。全部是纯 Markdown，任何支持 MCP 的 agent 工具都能使用。

```
 Claude Code / Codex / Kiro / Cursor …        维护员 agent（可选，宿主任意）
          │  MCP: wiki_capture                    │  agent-context pending / lint / commit
          ▼                                       ▼
      raw/（只追加的记录） ──整理──▶  wiki/（主题页面） ◀── 你直接编辑
```

- **raw/**：用户要求记录，或 agent 按全局规则自动沉淀的小条目。每条带结论、依据和适用范围，写入后不再修改。
- **wiki/**：维护员从 raw 整理出的主题页面：合并重复、补上链接、标出矛盾、列明出处。
- **board/**：本机黑板，存放短期的工作状态，各 agent 工具按 key 覆盖写入，比如跨工具的交接、某个端口被谁用来做什么、某个环境挂了。不进 git，不会自动过期。
- **你**有三种介入方式：让 agent 写入记录、跟维护员对话、直接编辑页面。每次整理都是一个单独的 git 提交，任何改动都可以审阅或撤回。

## 环境要求

- 编译需要 Rust **1.89 或更新版本**（`rustup update stable`），PATH 中要有 `git`。
- 一个支持 MCP（stdio 传输）的 agent 工具，用于写入和检索。
- 可选：一个能编辑文件、能执行 shell 命令的 agent，用来担任维护员。
- 已在 macOS 上测试。Linux 预计可用，但未测试；Windows 未测试。

## 安装

agent-context 没有发布到 crates.io，需要从源码编译：

```sh
git clone https://github.com/Zephor5/agent-context.git
cd agent-context
cargo install --locked --path .        # 安装到 ~/.cargo/bin/agent-context
agent-context init "$HOME/my-context"  # 创建 vault 并初始化它的 git 仓库
```

所有命令按以下顺序确定 vault：`--root <dir>`，其次是环境变量 `AGENT_CONTEXT_ROOT`，再次是当前目录（如果它是一个 vault）。agent-context 不展开 `~`，配置文件里请写绝对路径。

## 配置

1. **接入 agent 工具**：注册 MCP 服务，并将 [客户端指令模板](templates/agent-context.md) 加入每个工具的全局指令，启用主动检索、自动沉淀和黑板协作。见 [docs/clients.md](docs/clients.md)。
2. **运行维护员（可选）**：定时整理，和/或跟一个加载了内置提示词的 agent 对话。见 [docs/maintainer.md](docs/maintainer.md)。没有维护员时，记录会一直留在 `raw/`，`wiki_search` 仍然能搜到，但不会被整理成页面。

这两份说明都可以直接交给 agent 工具，让它替你完成配置，末尾各附了一段可直接粘贴的提示词。

## MCP 工具

| 工具 | 作用 |
|---|---|
| `wiki_capture(title, conclusion, evidence, scope, type, tags?, repo?, ref?)` | 向 `raw/` 追加一条记录（`type` 取值：decision、finding、howto、pitfall、reference）。多个写入方同时写也安全。 |
| `wiki_search(terms, limit?)` | 在 `wiki/` 和尚未整理的 `raw/` 中按关键词检索，支持中文。每一项都是一个不区分大小写的子串，可以是短语；命中任意一项就返回，命中项多的排在前面，每条结果用 `matched` 列出命中了哪几项。同义词请放进同一次调用。 |
| `wiki_read(path)` | 读取一个页面或一条记录（仅限 `wiki/` 和 `raw/`）。 |
| `board_list(query?)` | 列出黑板条目：key、第一行摘要、`updated_at`，最新的在前。`query` 中的关键词必须全部出现在 key 或内容里。 |
| `board_read(key)` | 读取一条条目的全文及其 `version`。 |
| `board_put(key, value, expect_version?)` | 新建或整条覆盖。key 自由命名，用 `:` 分段。传 `expect_version` 时，如果条目在你读取之后被别人改过，写入会被拒绝；传 0 表示条目必须不存在。 |
| `board_edit(key, old_text, new_text, expect_version?)` | 局部修改条目，不用重发全文：把恰好出现一次的 `old_text` 替换为 `new_text`（为空即删除）。`old_text` 找不到或不唯一时拒绝。替换在黑板锁内基于当前内容进行，条目其他部分的修改不会丢。 |
| `board_delete(key)` | 删除一条条目。 |

刻意不提供编辑页面的工具。黑板只是说明，不是锁：端口或 worktree 能不能用，以实际去用的结果为准。

## 命令行

| 命令 | 作用 |
|---|---|
| `agent-context init <dir>` | 用模板创建 vault 并初始化 git 仓库 |
| `agent-context serve` | 通过 stdio 提供 MCP 服务（由 agent 工具启动） |
| `agent-context web` | 本机 Web 界面（见下文） |
| `agent-context prompt maintainer` | 输出内置的维护员流程 |
| `agent-context pending --json` | 列出尚未整理的记录，并把它们记为当前批次 |
| `agent-context commit --compile -m "…"` | 把当前批次标记为已整理，追加 `log.md`，提交一次 |
| `agent-context commit -m "…"` | 只提交修改，不标记整理（raw 记录仍待整理） |
| `agent-context lint [--json]` | 检查失效的 `[[链接]]`、孤立页面、索引缺漏、格式不对的记录（有问题时退出码为 1） |
| `agent-context board list\|read\|put\|edit\|delete` | 在 shell 里操作黑板 |

## 本机 Web 界面

```sh
agent-context web --root "$HOME/my-context"   # 首次启动，会记住这个 vault
agent-context web                             # 之后在任意目录启动
```

浏览器会打开 `http://127.0.0.1:7420`。命令需要保持运行，按 Ctrl+C 停止。`--no-open` 只打印地址，`--port 0` 自动选一个空闲端口。只监听 IPv4 回环地址，不需要登录，不联网，所有静态资源都内置在程序里。界面目前只有中文。

- **黑板**：按 key 前缀分组的卡片，支持全文搜索、新建、编辑、删除。更新和删除时会校验你读到的版本。
- **Wiki**：按主题和标签浏览，可跳转到出处，可编辑完整的 Markdown（包括头部字段），可预览和保存。raw 记录只读。
- **实时刷新**：每三秒检查一次本地文件。阅读视图会自动刷新；拖选文字或保留文本选区时暂停自动刷新，取消选区后恢复，未变化的列表不会重建。编辑视图会保留你的草稿，并提供与最新内容对比、合并的选项；有未保存的修改时离开页面会先确认。保存时校验内容哈希。其他编辑器和维护员不共用 Web 界面的写锁，所以不要在两个地方同时保存同一个文件。
- **Git**：保存只写文件。提交按钮会先预览，然后只提交当前这一页，不会带上其他已暂存的文件或待整理的 raw 记录，也不会往 `log.md` 追加内容。

没有 `--root` / `AGENT_CONTEXT_ROOT` 时，Web 界面先用当前目录（如果它是 vault），否则用上一次打开的 vault。这个位置保存在 `$XDG_CONFIG_HOME/agent-context/web.json`（默认 `~/.config/agent-context/web.json`）。

## vault 目录结构

```
AGENTS.md                     你给维护员的规则（优先于内置提示词）
raw/YYYY/MM/*.md              写入的记录（只追加）
wiki/index.md, wiki/*.md      整理后的页面
board/<a>/<b>.md              key 为 a:b 的黑板条目（git 忽略）
log.md                        每次提交一行
.agent-context/compiled.txt   已整理过的记录（纳入版本管理）
.agent-context/pending.json   当前整理批次（git 忽略）
```

## 开发

```sh
cargo test
node --test tests/web-selection.test.cjs
cargo clippy --all-targets -- -D warnings
```

维护员提示词（`prompts/maintainer.md`）、vault 模板（`templates/vault/`）和 MCP 提示文本（`src/mcp.rs`）是编译进程序的，改了之后要重新安装，并重新连接 MCP 服务。客户端指令模板（`templates/agent-context.md`）需要同步到各工具的全局指令文件。

## DeepSeek Harness 插件

[DSH 插件](integrations/dsh/README.md) 将本机 agent-context 接入 DSH，会话可以使用同一份 wiki 和黑板。安装和配置方法见插件文档。

## 许可证

[MIT](LICENSE)
