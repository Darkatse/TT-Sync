---
name: tt-sync
description: 帮助用户安装、配置和使用 TT-Sync，根据需要选择手动 TUI、自动化 CLI 或 Docker，完成启动、配对和运行验证。面向用户侧部署与日常操作。
---

# TT-Sync 安装与使用

协助用户在选定的设备上运行 TT-Sync，并连接 TauriTavern 客户端。使用中文说明操作及结果，沿用用户已经选定的版本、目录、部署方式和权限。

## 选择操作方式

从用户说明或现有部署中确定运行设备、系统、数据来源与目录，以及客户端能访问的 HTTPS 地址。只补问影响当前操作的缺失信息。

| 用户的需要 | 采用的方式 |
|---|---|
| 用户自己操作，希望逐步完成配置 | TUI：准备程序，引导用户使用全屏界面 |
| 希望 AI 完成配置和启动，或在无交互终端的服务器上部署 | CLI：使用明确的配置路径和命令 |
| 已使用 Docker 管理服务，或明确选择容器部署 | Docker：使用项目提供的 Compose 文件 |

已有可用部署时先复用。升级沿用原配置和状态目录；状态目录保存设备身份与配对关系，应与同步数据分开。

## 安装与资料

TUI 和 CLI 使用同一个可执行程序。需要安装时，从 [官方 Releases](https://github.com/Darkatse/TT-Sync/releases) 确认版本；用户未指定时选择稳定版。

- Linux/macOS 使用 [install.sh](https://raw.githubusercontent.com/Darkatse/TT-Sync/main/scripts/install.sh)，通过 `--version` 指定选定版本，`--dir` 指定安装位置。
- Windows 使用 [install.ps1](https://raw.githubusercontent.com/Darkatse/TT-Sync/main/scripts/install.ps1)，对应参数为 `-Version` 和 `-InstallDir`。
- 两份安装脚本会选择系统对应的二进制并校验下载文件。安装后检查实际程序的 `--version` 输出。

Docker 路线直接使用容器镜像。已有仓库时读取本地示例；单独加载本技能时，可使用下面的官方链接。为旧版本处理配置时，读取对应版本的资料，并以已安装程序的 `--help` 为准。

## 手动 TUI

1. 将程序放在当前用户可写的目录。TUI 读取并保存可执行文件旁的 `config.toml`，`--config-file` 只对 CLI 命令生效。
2. 请用户在可交互的终端运行 `tt-sync onboard`。按当前界面说明数据目录、访问地址和运行方式的含义，由用户完成选择。
3. 引导用户把配对链接导入 TauriTavern 的同步设置，核对设备和权限。
4. 日常管理运行 `tt-sync`。需要后台运行时，引导用户选择界面提供的用户服务；前台运行则保持该终端会话。

这条路线以用户的实际操作进度为准。若任务改为由 AI 代为配置，使用 CLI 路线。

## 自动化 CLI

先阅读 [命令说明](https://github.com/Darkatse/TT-Sync/blob/main/docs/CLI.md) 和 [配置示例](https://raw.githubusercontent.com/Darkatse/TT-Sync/main/config.toml.example)。固定本次部署的配置文件与状态目录，后续命令使用同一组 `--config-file`、`--state-dir`。

新部署用 `tt-sync init --help` 查看初始化参数，再用 `init` 创建配置和身份；已有配置只修改任务所需的字段。设置目录结构时按数据来源选择 `layout`，填写路径时优先使用展开后的绝对路径。

例如，下面两条命令演示初始化和检查。路径、目录结构与域名需替换为本次部署的值：

```bash
tt-sync --state-dir /srv/tt-sync --config-file /srv/tt-sync/config.toml \
  init --path /srv/tavern/data --layout tauri-tavern \
  --public-url https://sync.example.com:8443
tt-sync --state-dir /srv/tt-sync --config-file /srv/tt-sync/config.toml doctor
```

通过同一配置启动 `serve`。常驻运行交给目标机的服务管理器，并检查服务状态和日志。需要生成配对链接时，另行执行同一路径参数下的 `pair open --json`；设备管理使用 `peers list --json`、`peers revoke`。

## Docker

按 [Docker 部署说明](https://github.com/Darkatse/TT-Sync/blob/main/docs/Docker.md) 准备 [docker-compose.yaml](https://raw.githubusercontent.com/Darkatse/TT-Sync/main/docker-compose.yaml)、[.env.example](https://raw.githubusercontent.com/Darkatse/TT-Sync/main/.env.example) 和配置示例。已有 Compose 项目时在其现有部署中调整。

`.env` 中填写宿主机目录和映射端口；`config.toml` 中填写容器内的目录。默认 `/state` 持久化配置和身份，`/data` 挂载同步数据。外部证书需要额外挂载，配置中也使用容器内路径。

用户指定版本时，用 `TT_SYNC_IMAGE` 选择对应镜像标签，例如 `ghcr.io/darkatse/tt-sync:2.4.0`。

先用 `docker compose config` 核对挂载、镜像和端口，再启动与检查：

```bash
docker compose up -d
docker compose ps
docker compose logs --tail 50 tt-sync
docker compose run --rm tt-sync doctor
docker compose run --rm tt-sync pair open --json
```

管理命令通过镜像入口使用同一份 `/state/config.toml`。更新镜像或重建容器时保留原状态和数据挂载。

## 配置、配对与验证

- `public_url` 是客户端实际使用的 HTTPS 地址，监听地址与对外地址分别核对。经反代或 CDN 访问时，使用该入口的地址和证书公钥指纹。
- 外部证书及 `public_spki_sha256` 配置需要 2.4.0 或以上版本。显式指纹优先用于配对；本机监听仍使用配置的证书与私钥，并通过 HTTPS 提供服务。填写方法见配置示例。
- 权限按用户用途选择：仅下载且不做镜像删除可用 `--ro`；需要上传时使用默认读写权限；需要镜像删除时使用 `--mirror`，它不能与 `--ro` 同时使用。
- 核对 `doctor` 的实际检查结果，尤其是数据、扩展目录和 TLS。当前 `doctor` 会把失败项写入输出，不能只凭退出码 0 判断通过。服务进程状态、客户端可达性和配对结果也需分别确认。
- 实际文件同步由 TauriTavern 客户端发起，使用用户选定的方向和数据范围。安装与配对可以通过运行状态和设备列表验证，无需为此同步真实数据。

完成后简要说明安装与配置位置、服务运行方式和已验证的结果。用户手动操作的步骤，按已确认的进度说明。
