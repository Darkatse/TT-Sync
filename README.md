<p align="center">
  <img src="https://img.shields.io/badge/Rust-🦀-orange?style=for-the-badge" alt="Built with Rust"/>
  <img src="https://img.shields.io/badge/TLS_1.3-安全-blue?style=for-the-badge" alt="TLS 1.3"/>
  <img src="https://img.shields.io/badge/单文件-便携-green?style=for-the-badge" alt="Portable"/>
</p>

<p align="center">
  <img src="./image/tt-sync-logo.png" alt="TT-Sync" width="1100"/>
</p>

<p align="center">
  <strong>TauriTavern 远程同步服务器</strong><br/>
  <em>我要给我的角色们一个更大的家！</em>
</p>

<p align="center">
  <img src="./image/demo.gif" alt="TT-Sync TUI demo" width="1100"/>
</p>

<p align="center">
  <a href="./README_EN.md">English</a>
</p>

---

## 为什么需要 TT-Sync？

你是否经常：
- 想在家里和 VPS 之间同步 TauriTavern 数据，但 LAN Sync 只能在局域网用？
- 希望把 NAS 当作权威副本，随时随地 pull？
- 顺便还想和原版 SillyTavern 同步？

**TT-Sync** 就是为这个问题准备的独立远程同步服务端：把 TauriTavern 的同步端点从局域网扩展到 VPS、NAS、家庭服务器，仍然保持安全、可控、可自部署。

使用 **Rust** 🦀 构建，TT-Sync 提供：
- **端到端加密**：TLS 1.3 + SPKI 证书固定，无需公网 CA
- **单一可执行文件**：适合 VPS、NAS、容器和家庭服务器
- **Ed25519 设备身份**：每个配对设备都经过密码学验证
- **双向兼容**：同时支持 TauriTavern 和原版 SillyTavern

---

## 安装

### 直接下载二进制

从 [Releases](https://github.com/Darkatse/TT-Sync/releases) 下载预编译二进制，放到你的 `$PATH` 中即可。

### 从源码编译

```bash
git clone https://github.com/Darkatse/TT-Sync.git
cd TT-Sync
cargo build --release
```

编译产物位于：

```bash
target/release/tt-sync
```

Windows 下为 `tt-sync.exe`。

---

## 推荐使用方式：TUI

### 首次部署

首次部署建议直接运行：

```bash
tt-sync onboard
```

引导流程会按顺序带你完成：
- 选择语言
- 设置监听端口与 `Public URL`
- 选择 `layout mode`
- 选择服务器上的数据文件夹
- 可选立即进入配对界面
- 选择服务运行方式：前台运行，或用户态 service

已支持的用户态 service：
- Linux：`systemd --user`
- macOS：`LaunchAgent`
- Windows：`Task Scheduler`（beta）

### 日常管理

完成初始化后，日常使用建议直接进入主界面：

```bash
tt-sync
```

主菜单当前覆盖：
- `Onboard`：重新走一遍引导配置
- `Pair`：生成二维码 / 配对链接，等待 TauriTavern 接入
- `Peers`：查看、改名、调整权限、撤销已配对设备
- `Serve`：启动/停止前台服务，或管理用户态 service
- `Doctor`：检查配置、证书、数据目录和配对状态
- `Help`：查看按键与部署提示

### 基本按键

- `↑ ↓`：移动焦点
- `Enter`：确认
- `Esc`：返回上一级
- `q`：退出

配对页面还支持：
- `r`：刷新二维码 / 重新生成配对令牌

### 配对流程

推荐路径很简单：
1. 在服务器上运行 `tt-sync onboard` 完成初始化，或进入 `tt-sync` 主菜单选择 `Pair`。
2. 在配对界面生成二维码或配对链接。
3. 在 TauriTavern 中扫描二维码或粘贴 `tauritavern://tt-sync/pair?...` 链接完成配对。
4. 之后通过 `Peers` 页面管理权限，通过 `Serve` 页面管理服务状态。

---

## Layout Mode

TT-Sync v2 使用固定的**全量同步数据集**。需要选择的是你本地服务器本地文件夹的形态。

| 选项 | 适用对象 | 全局扩展映射 |
|------|----------|--------------|
| `tauritavern` | TauriTavern `data/` | `extensions/third-party` → `data/extensions/third-party` |
| `sillytavern` | SillyTavern 仓库布局 | `extensions/third-party` → `public/scripts/extensions/third-party` |
| `sillytavern-docker` | SillyTavern Docker 目录布局 | `extensions/third-party` → `./extensions` |

如果你部署的是：
- TauriTavern 独立数据目录，选 `tauritavern`
- 普通 SillyTavern 仓库，选 `sillytavern`
- Docker 卷挂载，选 `sillytavern-docker`

---

## 安全模型

```
┌──────────────────────────────────────────────────────────┐
│  第一层：传输安全                                           │
│  TLS 1.3（自签名）+ SPKI 证书固定                           │
│  → 客户端在配对时固定服务器公钥                               │
├──────────────────────────────────────────────────────────┤
│  第二层：设备身份                                           │
│  Ed25519 每设备密钥对 + 规范请求签名                         │
│  → 签名验证通过后才签发短期 Session Token                    │
├──────────────────────────────────────────────────────────┤
│  第三层：授权控制                                           │
│  每设备 ACL：read / write / mirror-delete                 │
│  固定 allowlist 限制可见路径范围                            │
└──────────────────────────────────────────────────────────┘
```

---

## 开发者文档

如果想进行开发，欢迎查看文档哦~

- [CLI 参考](./docs/CLI.md)
- [系统架构](./docs/Architecture.md)
- [当前实现状态](./docs/CurrentState.md)
- [上游协议契约](./docs/UpstreamContract.md)
- [技术栈](./docs/TechStack.md)

---

## 贡献

发现 bug？想要新功能？欢迎 PR！

```bash
cargo test
cargo build --release
```

---

## 许可证

MIT 许可证 — 随便同步，不过角色跑丢了别怪我们 XD

---

<p align="center">
  <em>用 ❤️ 为 TauriTavern 社区打造。</em><br/>
  <strong>祝同步愉快！</strong>
</p>
