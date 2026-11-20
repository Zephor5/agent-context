# 运行维护员

维护员不属于 agent-context 本身，它是任何一个加载了内置流程的 agent。agent-context 提供这套流程（`agent-context prompt maintainer`），并把其中固定不变的步骤做成命令；宿主由你自己选择和配置，包括用哪个 agent 工具、用哪个调度器、汇报发到哪里。维护员是可选的：没有它，记录会一直留在 `raw/`，`wiki_search` 仍然能搜到，但不会被整理成页面，黑板上过时的条目也没人清理。

## agent 需要什么

- 对 vault 目录的读写权限。
- 能执行的 shell 命令：`agent-context`、`git`，以及用来核实黑板条目是否仍然有效的只读命令，比如 `ls`、`git worktree list`、`lsof`。
- PATH 中有 `agent-context`，或者在指令里写明它的绝对路径。
- 能找到 vault：要么把 vault 设为 agent 的工作目录，要么每条 `agent-context` 命令都带 `--root <vault>`、`git` 命令都用 `git -C <vault>`。

agent-context 本身不需要联网，agent 只需要能连上它自己的模型。

决定维护员行为的文本有两份，宿主必须都加载：内置提示词（通用，随程序发布，重新安装后更新），以及 vault 里的 `AGENTS.md`（你的个人规则，冲突时以它为准）。

## 定时整理

默认每天一次就够了。每次运行应当：

- 从**全新的上下文**开始，前后两次运行之间不带任何状态；
- 在 vault 内执行上述命令时**不弹审批**。无人值守的运行一旦停下来等授权，就会一直等到被强制结束；
- 设一个**时限**（比如 30 分钟），卡住的运行到时自动结束；
- 把汇报发到你看得到的地方，没有改动时不出声。

每次运行的指令：

```text
你是 vault <vault> 的 agent-context 维护员。
每条 agent-context 命令都带 --root <vault>，git 命令都用 git -C <vault>。
1. 执行 `agent-context prompt maintainer` 并照做，然后读 <vault>/AGENTS.md。
2. 按其中的 Compile 一节执行一次整理。
3. 如果整理和黑板清理都没有任何改动，直接结束，不输出任何内容。
4. 否则给一段简短汇报：处理了哪些记录、改了哪些页面、删了哪些黑板条目，以及需要用户处理的事项。
```

每周体检是同一个任务，只把第 2 步换成「按其中的 Health check 一节执行一次体检」。

常见的宿主：你已经在用的 agent 平台自带的调度器；或者操作系统的调度器（cron、launchd、systemd timer、任务计划程序），以非交互模式启动一个命令行 agent，比如 `claude -p`、`codex exec`、`kiro-cli chat --no-interactive`。各工具预先放行文件编辑和 shell 命令的方式不一样，请查对应工具的文档。

## 对话

跟维护员对话可以修正页面、提出以后都要遵守的规则（它会写进 `AGENTS.md`），也可以让它随时做一次整理或体检。任何能保留对话的 agent 工具都可以：把 vault 设为工作目录打开它，再把下面这段作为常驻指令（或第一条消息）：

```text
你是 vault <vault> 的 agent-context 维护员。
每次对话开始时，执行 `agent-context prompt maintainer` 并照做，然后读 <vault>/AGENTS.md。
每条 agent-context 命令都带 --root <vault>。
```

维护员每次提交都会带上 vault 里所有未提交的改动，所以不要在对话正在改页面的时候跑定时整理。

## 检查配置

手动触发一次定时运行。没有待整理的记录时，它应该在一两分钟内跑完，并且不产生任何改动。看一下这次运行的输出里有没有被拒绝的命令或审批提示，然后检查：

```sh
agent-context --root <vault> pending --json   # 整理后应为 []
agent-context --root <vault> lint
git -C <vault> log --oneline -3
```

不要往真实 vault 里写测试记录来验证整理：raw 记录只追加不修改。要测试整理，用 `agent-context init <临时目录>` 建一个临时 vault。

## 交给 agent 来配

把下面这段填好占位符，粘贴到要担任维护员宿主的 agent 工具里：

```text
按照 <agent-context 仓库路径>/docs/maintainer.md 配置 agent-context 维护员。
vault：<vault 的绝对路径>。程序路径：<`command -v agent-context` 的输出>。
用 <调度器，或「你自己的调度器」> 建一个每天 <时间和时区> 运行的定时整理，汇报发到 <发送位置>。
确保这个运行在 vault 里使用 agent-context 和 git 时不弹审批，并且设有时限。
<如果需要，再配置维护员对话 / 每周体检。>
然后手动触发一次，把输出给我看，并告诉我你改了哪些地方。
```
