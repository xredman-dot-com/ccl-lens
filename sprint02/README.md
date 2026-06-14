# sprint02 — 接管模式 + SOCKS 账密解析 + 连接信息

## 目标
让拦截更可控、更直观：接管方式可选、SOCKS 账密一键粘贴、运行时显示真实出口信息。

## 范围（本 sprint 做）
1. **三种接管模式**（运行中不可改，停止后可切）
   - `config` 改主配置：写 `~/.claude/settings.json` 的 `env.ANTHROPIC_BASE_URL`（默认，可逆）
   - `env` 环境变量：不改配置，面板给出 `export ANTHROPIC_BASE_URL=...` 让用户自己导出
   - `test` 仅测隧道：不改配置，只绑定端口并验证上游隧道是否通
2. **SOCKS 账密 + 快捷解析**：粘贴 `host:port:user:pass`
   （如 `207.251.13.173:5782:7nfnpt54:gQXVaTPSz6av`）自动解析成
   `socks5://user:pass@host:port` 并填表；也支持 `user:pass@host:port` / 裸 URL
3. **连接管理面板**（运行时展示，对齐目标设计）
   - 状态点：运行中 / 已停止
   - 端口 / 状态（ProxyReady）
   - 隧道：正常 (延迟ms) / 异常
   - 上游：`SOCKS5 host:port`（**密码不显示**）
   - 出口 IP：通过隧道查 ipinfo.io 得到 IP + 地理位置（City, CC）

## 不做
- 出口 IP 的离线 GeoIP 库（暂用 ipinfo.io 在线查询）
- 多隧道并发/分流（仍是单选上游 + 健康切换）

## 关键实现
- `models.rs` `TakeoverMode` / `TunnelStatus`；`upstream.rs` `probe_exit_ip`（HTTPS 走选中 client）、`endpoint_of`（隐藏账密，含单测）
- `commands.rs` `update_tunnel`（select 上游 → 查出口 IP → emit `tunnel`）；start/stop 按模式决定是否改 settings.json
- 健康监测循环顺带刷新隧道信息；前端 `Connection.tsx` + `parse.ts`

## 安全
- 上游 URL 含密码，**展示一律用 `endpoint_of` 去除账密**（`endpoint_hides_auth` 单测锁定）
- 出口 IP 查询走 HTTPS（ipinfo.io）

## 自测清单（真实环境）
- [ ] 粘贴 `host:port:user:pass` → 节点可用、出口 IP 正确
- [ ] config / env / test 三模式行为符合预期（env/test 不动 settings.json）
- [ ] 面板出口 IP + 地理 + 隧道延迟显示正常，密码不泄露
