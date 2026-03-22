<p align="center">
  <img src="https://img.shields.io/badge/Rust-🦀-orange?style=for-the-badge" alt="Built with Rust"/>
  <img src="https://img.shields.io/badge/TLS_1.3-安全-blue?style=for-the-badge" alt="TLS 1.3"/>
  <img src="https://img.shields.io/badge/单文件-便携-green?style=for-the-badge" alt="Portable"/>
</p>

<h1 align="center">TT-Sync</h1>

<p align="center">
  <strong>TauriTavern 远程同步服务器</strong><br/>
  <em>我要给我的角色一个更大的家。</em>
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

**认识一下 TT-Sync** — 一个独立的命令行服务器，把你的 TauriTavern 数据安全地带到公网。

使用 **Rust** 🦀 构建，TT-Sync 具有：
- **端到端加密** — TLS 1.3 + SPKI 证书固定，无需 CA
- **单一可执行文件** — 丢到任何 VPS、NAS 或家庭服务器，零运行时依赖
- **Ed25519 设备身份** — 每个配对设备都经过密码学验证
- **双向兼容** — 同时支持 TauriTavern 和原版 SillyTavern

---

## 安装

### 从源码编译（推荐）

```bash
git clone https://github.com/Darkatse/TT-Sync.git
cd TT-Sync
cargo build --release

# 把编译好的文件拷到你喜欢的地方！
cp target/release/tt-sync ~/.local/bin/
```

### 直接下载二进制

从 [Releases](https://github.com/Darkatse/TT-Sync/releases) 下载预编译的二进制文件，放到 `$PATH` 里即可。搞定！

---

## 快速上手

### 1. 初始化

```bash
tt-sync init \
  --data-root /path/to/sillytavern/data/ \
  --public-url https://my-vps.example.com:8443
```

这会创建：
- `config.toml` — 服务器配置
- `identity.json` — 唯一设备 UUID + Ed25519 密钥对
- `tls/key.pem` + `tls/cert.pem` — 自签名 TLS 证书

### 2. 启动服务器

```bash
tt-sync serve
```

你会看到这样的启动信息：

```
  ▶ TT-Sync server running

  Listen       0.0.0.0:8443
  Public URL   https://my-vps.example.com:8443
  TLS          self-managed (SPKI pin)
  SPKI SHA-256 dGVzdC1zcGtp...

  Press Ctrl+C to stop.
```

### 3. 配对客户端

```bash
# 只读配对（默认）
tt-sync pair open

# 读写配对，1 小时有效期
tt-sync pair open --rw --expires 1h

# 机器可读输出
tt-sync pair open --json
```

把 `tauritavern://tt-sync/pair?...` URI 粘贴到你的 TauriTavern 客户端即可。

### 4. 管理配对设备

```bash
# 列出所有已配对设备
tt-sync peers list

# 撤销一台设备（可用 ID 前缀或名称）
tt-sync peers revoke "My Phone"
```

---

## 功能一览

| 命令 | 功能 |
|------|------|
| `init` | 初始化服务器：配置、身份、TLS 证书 |
| `serve` | 启动 HTTPS 同步服务器 |
| `pair open` | 生成一次性配对令牌 + URI |
| `peers list` | 以表格显示所有已配对设备 |
| `peers revoke` | 移除已配对设备 |
| `profile list` | 显示每个作用域配置文件包含的目录 |
| `doctor` | 验证配置、TLS、数据目录、身份 |
| `cert show` | 显示 SPKI SHA-256 指纹 |
| `cert rotate-leaf` | 重签 TLS 证书（密钥不变，SPKI pin 不变） |

### 全局参数

| 参数 | 效果 |
|------|------|
| `--no-color` | 禁用 ANSI 彩色输出 |
| `--quiet` | 抑制非必要输出（适合脚本使用） |
| `--state-dir <path>` | 覆盖默认状态目录 |

---

## 作用域配置（Scope Profile）

TT-Sync 内置两个同步作用域配置：

| 配置名 | 描述 |
|--------|------|
| `default` | 完整的 TauriTavern 用户内容 — 角色卡、聊天、设置、主题、扩展等 |
| `compatible-minimal` | 与 v1 LAN Sync 白名单完全一致，适用于 SillyTavern 兼容场景 |

使用 `tt-sync profile list` 查看每个配置包含的具体目录和文件。

---

## 安全模型

```
┌──────────────────────────────────────────────────────────┐
│  第一层：传输安全                                         │
│  TLS 1.3（自签名）+ SPKI 证书固定                        │
│  → 每个客户端在配对时固定服务器公钥                       │
├──────────────────────────────────────────────────────────┤
│  第二层：设备身份                                         │
│  Ed25519 每设备密钥对，规范请求签名                       │
│  → 签名验证后颁发会话令牌                                │
├──────────────────────────────────────────────────────────┤
│  第三层：授权控制                                         │
│  每设备 ACL：read / write / mirror-delete                │
│  作用域配置限制可见目录范围                               │
└──────────────────────────────────────────────────────────┘
```

---

## 项目架构

```
TT-Sync/crates/
├── ttsync-contract   # 协议类型与线上格式（领域层）
├── ttsync-core       # 用例编排与 trait 定义（应用层）
├── ttsync-fs         # 文件系统适配器 — 扫描、原子写入、Peer 存储
├── ttsync-http       # HTTPS 服务器 (axum) 与客户端 (reqwest)，含 SPKI 固定
└── ttsync-cli        # CLI 二进制 — 你直接交互的入口
```

基于 Clean Architecture 构建：依赖向内流动。CLI 依赖 HTTP 和 FS 适配器，适配器依赖 Core，Core 依赖 Contract。Contract 对其他一切一无所知。

---

## 开发者打包指南

### 为你的平台编译

```bash
cargo build --release
# 二进制文件：target/release/tt-sync（Windows 上是 tt-sync.exe）
```

### 运行测试

```bash
cargo test
```

### GitHub Actions

每次推送到 `main` 分支都会通过 GitHub Actions 自动构建以下平台：

| 平台 | 架构 | 产物 |
|------|------|------|
| Linux | x86_64 | `tt-sync-linux-x64` |
| Linux | ARM64 | `tt-sync-linux-arm64` |
| Windows | x86_64 | `tt-sync-windows-x64.exe` |
| macOS | x86_64 (Intel) | `tt-sync-macos-x64` |
| macOS | ARM64 (Apple Silicon) | `tt-sync-macos-arm64` |

二进制文件发布在 [Releases](https://github.com/Darkatse/TT-Sync/releases) 页面的 nightly 预发布中。

---

## 贡献

发现 bug？想要新功能？欢迎 PR！

```bash
cargo test
cargo build
```

---

## 许可证

MIT 许可证 — 随便同步，不过角色跑丢了别怪我们 XD

---

<p align="center">
  <em>用 ❤️ 为 TauriTavern 社区打造。</em><br/>
  <strong>祝同步愉快！</strong>
</p>
