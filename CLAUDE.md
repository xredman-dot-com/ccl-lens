# CLAUDE.md

本文件给 [Claude Code](https://docs.anthropic.com/en/docs/claude-code) 在本仓库工作时提供上下文。

## 项目简介

ccl-lens 是一个 Tauri 2 桌面应用，作为 **Claude Code 与 Anthropic API 之间的本地反向代理**，实时观测每次请求的模型、延迟、token 用量、成本与异常，并管理多条出口（直连 / SOCKS5 / HTTP）的健康探测与故障切换。

接管原理：写入 `~/.claude/settings.json` 的 `env.ANTHROPIC_BASE_URL = http://127.0.0.1:31415`，Claude Code 即把流量发到本地代理，代理转发到 `api.anthropic.com`，途中 tee 一份 SSE 解析用量。

## 技术栈

- 前端：React 19 + TypeScript + Vite 5（`src/`）
- 后端：Rust + Tauri 2（`src-tauri/src/`）
- 关键依赖：`tokio`、`hyper`/`hyper-util`、`reqwest`（rustls-tls + socks）、`rusqlite`（bundled）、`rcgen` + `rustls`（MITM TLS）、`tokio-socks`

## 常用命令

```bash
pnpm install          # 装前端依赖
pnpm tauri dev        # 开发模式（前端 Vite + Tauri 窗口）
pnpm build            # 仅构建前端（tsc && vite build）
./build.sh            # 打 macOS .app
./build.sh --dmg      # 出 .dmg
./build.sh --windows  # 交叉编译 Windows（需 rustup + cargo-xwin）
```

三平台正式出包走 GitHub Actions：`.github/workflows/release.yml`（推 `v*` tag 触发）。

## 后端模块（`src-tauri/src/`）

| 文件 | 职责 |
|------|------|
| `lib.rs` | 应用装配、命令注册、后台健康监测循环、退出时还原 settings |
| `main.rs` | 二进制入口，调用 `lib::run()` |
| `models.rs` | 共享数据结构（`Upstream` / `Health` / `RequestRecord` / 枚举）、id 与时间戳 |
| `state.rs` | `AppState`、ccl 自身配置持久化（`~/.ccl-lens/config.json`） |
| `claude.rs` | `~/.claude/settings.json` 安全读写：启停接管、备份、残留恢复 |
| `proxy.rs` | 本地监听（`127.0.0.1:31415`）+ 转发 + 流式 tee + 落库 + emit |
| `upstream.rs` | 上游池、reqwest 客户端、健康探测、调度选择、出口 IP 查询、`endpoint_of`（隐藏账密） |
| `sse.rs` | SSE / JSON 增量解析，提取 usage / 文本 / 错误 |
| `pricing.rs` | 模型定价表与成本计算 |
| `store.rs` | SQLite 持久化与聚合统计（`~/.ccl-lens/history.db`） |
| `commands.rs` | Tauri 命令与视图结构、隧道刷新、出口 IP（ipinfo.io） |
| `ca.rs` / `mitm.rs` | 私有 CA 与 MITM TLS 支持 |
| `tray.rs` | 托盘菜单 |

## 前端结构（`src/`）

- `App.tsx` —— 编排：事件订阅、tab、详情面板
- `components/` —— `Header` / `Connection` / `Upstreams` / `Timeline` / `Stats` / `RequestDetail` / `Settings`
- `api.ts` / `types.ts` / `format.ts` / `parse.ts` —— Tauri invoke 封装、类型、格式化、SOCKS 账密解析
- `styles.css` / `App.css` —— 深色主题

## 关键约定与注意点

- **接管可逆且自愈**：改 `settings.json` 前先备份到 `settings.json.ccl-lens.bak`；任意退出路径（托盘 Quit / Cmd+Q / 进程结束）都会 `shutdown()` 还原；启动时 `recover_stale()` 兜底恢复上次未清理的改动。
- **账密绝不外显**：展示上游一律走 `endpoint_of` 去除 user:pass（有 `endpoint_hides_auth` 单测锁定）。
- **不破坏 SSE**：代理零缓冲流式转发，解析只在旁路 tee。
- **调度策略**：`fixed` / `sticky`（默认）/ `auto`；健康探测用延迟 EWMA + 成功率，任一启用节点 down 时探测间隔自适应收紧到 ≤5s。
- **接管模式**：`config`（改主配置）/ `env`（只给 export 命令）/ `test`（仅测隧道）——运行中不可改，停止后可切。
- 默认端口 `31415`；UI 文案与原型**不使用 emoji**，图标用 SVG。

## 迭代记录

- `sprint01/README.md` —— 拦截代理 + token/成本可观测（基础）
- `sprint02/README.md` —— 三种接管模式 + SOCKS 账密解析 + 连接信息面板

## 不要提交的内容

`node_modules` / `dist` / `src-tauri/target` / `src-tauri/gen/schemas` / `.omc` 均已在 `.gitignore`。
