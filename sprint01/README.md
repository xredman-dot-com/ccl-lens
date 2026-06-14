# sprint01 — 拦截代理 + Token/成本可观测

## 目标
让 Claude Code 的所有 API 请求经过 ccl-lens 本地代理，实时看到模型、延迟、token、成本与异常；
并支持多上游（SOCKS5 / HTTP / 直连）池化与按健康/速率切换。

## 范围（本 sprint 做）
- 反向代理：监听 `127.0.0.1:31415`，转发到 `api.anthropic.com`，零缓冲流式（SSE 不破）
- 接管机制：写 `~/.claude/settings.json` 的 `env.ANTHROPIC_BASE_URL`（启停可逆，首次改动前备份）
- 上游池：多节点 + 主动健康探测（延迟 EWMA / 成功率）+ 三种调度
  - `fixed` 固定 / `sticky` 粘性优先（默认）/ `auto` 自动最快
- Token & 成本：从 SSE `message_start` / `message_delta` 读真实用量，按定价表算成本
- UI：实时时间线、Token/成本仪表盘（按模型聚合）、请求详情检查器（messages / system / tool_use）
- 历史持久化：SQLite（`~/.ccl-lens/history.db`），可清空

## 不做（留给后续 sprint）
- 方案 A（私有 CA / 全主机抓取，含 OAuth 刷新与遥测）
- 配置可视化编辑（settings / hooks / MCP）
- Session 浏览器、跨项目搜索、菜单栏常驻
- 上下文窗口可视化、泄露检测、录制回放
- 通用二进制（Intel + M2）打包：当前机器是 Intel 且无 rustup，仅出 x86_64 原生版

## 架构
```
Claude Code → http://127.0.0.1:31415 (axum 监听, 绕开 8888 via NO_PROXY)
            → tee SSE 解析 token/成本/异常 → 实时 emit 前端
            → reqwest 经选中上游 (direct/socks5/http) → api.anthropic.com
```
故障切换边界：新请求/建连失败 → 自动换健康节点；SSE 流到一半断 → 该请求失败由 CC 自身重试。

## 文件索引
后端 `src-tauri/src/`
- `models.rs`     共享数据结构、id/时间戳
- `claude.rs`     settings.json 安全读写（启停接管、备份）
- `pricing.rs`    模型定价表与成本计算
- `store.rs`      SQLite 持久化与聚合统计
- `sse.rs`        SSE/JSON 增量解析，提取 usage / 文本 / 错误
- `upstream.rs`   上游池、reqwest 客户端、健康探测、调度选择
- `proxy.rs`      axum 监听 + 转发 + 流式 tee + 记录落库/emit
- `state.rs`      AppState、ccl 自身配置持久化（~/.ccl-lens/config.json）
- `commands.rs`   Tauri 命令 + 视图结构
- `lib.rs`        装配、命令注册、后台健康监测循环

前端 `src/`
- `types.ts` / `api.ts` / `format.ts`
- `App.tsx`       编排（事件订阅、tab、详情）
- `components/`    Header / Upstreams / Timeline / Stats / RequestDetail
- `styles.css`    深色主题（无 emoji）

## 运行
```bash
pnpm install
pnpm tauri dev        # 需要 Node 18（已锁 Vite 5）
```
启动后点「启动拦截」，在任意项目运行 `claude` 即可看到流量。
停止拦截会移除 `ANTHROPIC_BASE_URL`，不影响现有 `HTTPS_PROXY=8888` 配置。

## 自测清单（待在真实环境跑）
- [ ] 当前 CC（v2.1.177）是否吃 `settings.json.env.ANTHROPIC_BASE_URL` 的 `http://`
- [ ] SSE 流式不破、token 数与官方一致
- [ ] 某上游断开时 sticky 自动切换、恢复后切回
- [ ] 断网（换房间）→ 全节点 down → 请求快速失败 → 恢复后自动复活
