# ccl-lens

> Claude Code 流量透镜 —— 本地拦截代理 + Token / 成本实时可观测

ccl-lens 是一个跨平台桌面应用（Tauri + React + Rust）。它在本地起一个反向代理，把 [Claude Code](https://docs.anthropic.com/en/docs/claude-code) 的所有 API 请求接管过来，让你实时看到**每一次请求的模型、延迟、token 用量、成本和异常**，并支持多条出口（直连 / SOCKS5 / HTTP 代理）的健康探测与自动切换。

整个项目由 [Claude Code](https://www.anthropic.com/claude-code) 完整生成。

---

## 它能做什么

- **零侵入接管**：写入 `~/.claude/settings.json` 的 `env.ANTHROPIC_BASE_URL`，把 Claude Code 指向本地代理。改动前自动备份，退出时自动还原。
- **实时时间线**：每个请求的模型、状态码、首字节延迟（TTFB）、总耗时一目了然。
- **Token / 成本仪表盘**：从 SSE 流里读真实 `usage`（含 cache 读写），按定价表算出每次调用与按模型聚合的花费。
- **请求详情检查器**：查看 messages / system / tool_use 等请求体与响应文本。
- **多上游池 + 故障切换**：配置多条出口，主动健康探测（延迟 EWMA / 成功率），三种调度策略：
  - `fixed` 固定使用指定出口
  - `sticky` 粘性优先（默认，挂了切走、恢复切回）
  - `auto` 始终走最快的健康出口
- **三种接管模式**：`config`（改主配置，可逆）/ `env`（只给出 export 命令，不改文件）/ `test`（仅测隧道连通）。
- **SOCKS 账密快捷解析**：粘贴 `host:port:user:pass` 自动转成 `socks5://user:pass@host:port` 并填表。
- **出口信息展示**：经选中隧道查询出口 IP + 地理位置（密码永不显示）。
- **历史持久化**：本地 SQLite（`~/.ccl-lens/history.db`），可随时清空。

> 数据全部留在本机，不上传任何第三方。

---

## 工作原理

```
Claude Code
   │  ANTHROPIC_BASE_URL = http://127.0.0.1:31415
   ▼
ccl-lens 本地代理 (127.0.0.1:31415)
   │  · 零缓冲流式转发（不破坏 SSE）
   │  · tee 一份 SSE 解析 token / 成本 / 异常 → 实时推送前端
   ▼
选中的出口 (direct / socks5 / http)
   ▼
api.anthropic.com
```

故障切换边界：新请求或建连失败 → 自动换健康节点；若 SSE 流已开始后中断 → 该请求记为失败，交由 Claude Code 自身重试。

---

## 快速开始（开发模式）

环境要求：**Node 18+**、**pnpm 10+**、**Rust（stable）**。

```bash
pnpm install
pnpm tauri dev
```

启动后：

1. 在「连接」面板点 **启动拦截**。
2. 在任意项目目录运行 `claude`，流量就会出现在时间线里。
3. 点 **停止拦截** 会移除 `ANTHROPIC_BASE_URL`，还原你的原配置。

---

## 打包

### 本机打包（macOS）

```bash
./build.sh            # 打 .app（默认）
./build.sh --dmg      # 同时出 .dmg
./build.sh --windows  # 交叉编译 Windows 安装包（需 rustup + cargo-xwin）
```

产物在 `src-tauri/target/release/bundle/`。详见 `./build.sh --help`。

### GitHub Actions 三平台出包（推荐）

仓库内置 `.github/workflows/release.yml`，矩阵构建：

| 平台 | 产物 |
|------|------|
| Apple Silicon (M 芯片) | `.dmg` |
| Intel Mac | `.dmg` |
| Windows x64 | `.exe` (NSIS) + `.msi` (WiX) |

- 推 tag（如 `v0.1.0`）→ 自动汇总到一个 Draft Release。
- 手动 Run workflow → 产物在该次运行的 Artifacts 里下载。

> 默认未做代码签名：macOS 首次打开需右键 → 打开（或 `xattr -dr com.apple.quarantine <app>`）；Windows 会弹 SmartScreen，点「更多信息 → 仍要运行」。

---

## 配置与数据位置

| 路径 | 用途 |
|------|------|
| `~/.ccl-lens/config.json` | ccl-lens 自身配置（端口、上游池、调度模式等） |
| `~/.ccl-lens/history.db` | 请求历史（SQLite） |
| `~/.claude/settings.json` | 接管时写入 `ANTHROPIC_BASE_URL`，停止时还原 |
| `~/.claude/settings.json.ccl-lens.bak` | 首次改动前的备份 |

默认代理端口：`31415`。

---

## 技术栈

- **前端**：React 19 + TypeScript + Vite 5
- **后端**：Rust + Tauri 2（`tokio` / `hyper` / `reqwest` rustls / `rusqlite` bundled / `rcgen` + `rustls`）
- **代理**：本地反向代理，零缓冲流式转发，SSE 增量解析

后端模块（`src-tauri/src/`）与前端结构详见 [CLAUDE.md](./CLAUDE.md)。

---

## 安全说明

- ccl-lens 仅作用于 **Claude Code 与 Anthropic API 之间** 的流量，监听 `127.0.0.1`，不对外暴露。
- 出口 URL 若含账号密码，**展示时一律隐去**。
- 接管会修改 `~/.claude/settings.json`，但改动前备份、退出时还原；即使异常退出，下次启动也会尝试恢复残留改动。

---

## License

[MIT](./LICENSE)
