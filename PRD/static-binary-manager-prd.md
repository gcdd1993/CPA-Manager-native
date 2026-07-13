# CPA Manager Native - 静态 Binary 管理器 PRD

> 文档状态：Draft v1.0  
> 更新时间：2026-07-13  
> 目标版本：MVP  
> 参考项目：[LinJun](https://github.com/wangdabaoqq/LinJun)、[CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)、[CPA-Manager-Plus](https://github.com/seakee/CPA-Manager-Plus)

## 0. 技术架构约束

本产品采用 **Tauri + Rust** 构建跨平台桌面应用：

- **桌面容器**：Tauri 2；
- **核心后端**：Rust，负责 Release 检查、下载校验、解压安装、进程管理、健康检查、日志和更新回滚；
- **前端界面**：运行于 Tauri WebView，仅负责状态展示和用户交互，不直接操作文件、网络下载或子进程；
- **通信方式**：前端通过类型明确的 Tauri Commands 发起操作，Rust 通过 Events 或 Channels 推送进度、日志和状态变化；
- **异步运行时**：使用 Tauri 内置的 Tokio 运行环境执行网络请求、文件操作、健康检查和进程监督，禁止阻塞 UI 主线程；
- **状态管理**：Rust 端持有唯一的应用状态和组件状态机，前端状态是后端状态的投影，不能作为进程是否运行或更新是否完成的事实来源。

MVP 不引入独立本地服务或额外运行时。发布包必须包含运行桌面应用所需的全部资源，终端用户不需要安装 Node.js、Go 或 Rust。

## 1. 产品概述

CPA Manager Native 是一个跨平台桌面管理器，用于下载、更新、启动和监督以下两个静态 binary：

1. **CLIProxyAPI（CPA Core）**：提供 OpenAI、Gemini、Claude、Codex、Grok 等兼容 API 的本地网关。
2. **CPA-Manager-Plus（CPAMP）**：提供 CLIProxyAPI 的管理、监控与可视化界面。

应用本身不重复实现上游的账号管理、配置编辑、配额监控等业务功能，而是提供稳定的本地运行环境和统一入口，解决用户手动识别 Release 资产、下载解压、校验文件、管理进程和升级回滚的成本。

## 2. 背景与问题

两个上游项目均通过 GitHub Releases 发布多平台静态产物，但普通用户需要自行完成：

- 判断操作系统和 CPU 架构并选择正确资产；
- 下载压缩包、校验 checksum、解压和设置执行权限；
- 记忆启动参数、配置目录、端口和管理页面地址；
- 判断进程是否真实可用，而非仅确认进程存在；
- 更新前停止服务，更新后恢复服务；
- 在新版本启动失败时手动退回旧版本；
- 分辨两个组件的版本和运行状态。

LinJun 已验证一种可行模式：桌面应用内置或托管 CLIProxyAPI binary，使用独立数据目录和配置文件，通过子进程启动、端口预检、日志采集、健康检查和 GitHub Release 更新实现一键管理。本产品将这一模式扩展为双 binary 管理，并强化校验、原子更新与回滚能力。

## 3. 产品目标

### 3.1 核心目标

- 用户首次打开应用后，可在 3 次操作内完成两个组件的安装和启动。
- 用户可在一个界面确认组件的安装版本、最新版本、进程状态和服务健康状态。
- 更新过程不破坏用户配置、认证文件、数据库或日志。
- 更新失败时自动恢复上一可运行版本。
- 应用退出、崩溃或系统重启后，不遗留无法识别和管理的孤儿进程。

### 3.2 成功指标

- 支持平台上的资产自动匹配准确率为 100%。
- 正常网络下，安装或更新成功率不低于 99%。
- 已安装版本的启动成功率不低于 99.5%。
- 更新失败后的自动回滚成功率不低于 99%。
- 从点击启动到健康状态确认的 P95 小于 10 秒。

## 4. 非目标

MVP 不包含：

- 重做 CPA-Manager-Plus 已提供的管理面板功能；
- 编辑 CLIProxyAPI 的完整 YAML 配置；
- 管理 OAuth 账号、Provider、API Key、配额与请求日志；
- Docker、systemd、launchd 或 Windows Service 的安装管理；
- 多实例或集群管理；
- 自动穿透、防火墙配置、反向代理或公网暴露；
- 修改或重新打包上游 binary；
- 应用自身的自动更新。应用自更新可在后续版本独立规划。

## 5. 目标用户与场景

### 5.1 目标用户

- 希望在个人电脑上快速运行 CLIProxyAPI 与 CPA-Manager-Plus 的用户；
- 不熟悉命令行、GitHub Releases 和进程管理的用户；
- 需要频繁跟随上游版本更新的高级用户或多账号池维护者。

### 5.2 核心场景

1. 新用户首次安装两个组件并一键启动。
2. 日常打开应用查看服务是否健康并进入管理面板。
3. 检测到新版后完成无损更新并恢复原运行状态。
4. 新版本无法启动时自动回滚到旧版本。
5. 用户只运行其中一个组件，另一个保持未安装或停止。
6. GitHub 无法访问时继续使用已安装版本。

## 6. 产品原则

- **组件独立**：两个 binary 可分别安装、启动、停止、更新和回滚。
- **配置与程序分离**：版本替换只操作程序目录，永不覆盖数据目录。
- **健康优先**：进程存在不等于运行成功，状态必须结合进程和健康检查判断。
- **更新可恢复**：下载、校验、解压和启动验证完成前，不删除上一版本。
- **默认本地安全**：默认仅监听本机地址，不主动开放远程访问。
- **失败可理解**：面向用户展示可执行的错误原因，不只展示退出码。

## 7. 信息架构

### 7.1 主界面

主界面展示两个组件行或面板，每个组件包含：

- 组件名称与用途；
- 安装状态；
- 当前版本与最新版本；
- 运行状态与健康状态；
- 主操作：安装、启动、停止、重启或更新；
- 次操作：打开页面、查看日志、打开数据目录、版本管理；
- 最近错误摘要。

顶部提供整体状态和组合操作：

- 全部启动；
- 全部停止；
- 检查更新；
- 设置。

### 7.2 状态定义

| 状态 | 含义 | 可用主操作 |
|---|---|---|
| 未安装 | 本机不存在可用版本 | 安装 |
| 下载中 | 正在下载资产 | 取消 |
| 安装中 | 正在校验、解压或激活 | 等待 |
| 已停止 | 已安装但进程未运行 | 启动 |
| 启动中 | 进程已创建，等待健康检查 | 停止 |
| 运行中 | 进程存活且健康检查通过 | 停止、重启、打开页面 |
| 异常 | 进程退出或健康检查连续失败 | 重试、查看日志 |
| 有更新 | 已安装且存在更新 | 更新 |
| 更新中 | 正在执行更新事务 | 取消（仅下载阶段） |
| 回滚中 | 新版本验证失败，恢复旧版本 | 等待 |

## 8. 功能需求

### FR-01 平台与资产识别

应用必须识别当前 OS 和架构，并映射到上游 Release 资产。

| 系统 | 架构 | CLIProxyAPI 资产模式 | CPA-Manager-Plus 资产模式 |
|---|---|---|---|
| Windows | amd64 | `CLIProxyAPI_{version}_windows_amd64.zip` | `cpa-manager-plus_v{version}_windows_amd64.zip` |
| Windows | arm64 | `CLIProxyAPI_{version}_windows_aarch64.zip` | `cpa-manager-plus_v{version}_windows_arm64.zip` |
| macOS | Intel | `CLIProxyAPI_{version}_darwin_amd64.tar.gz` | `cpa-manager-plus_v{version}_darwin_amd64.tar.gz` |
| macOS | Apple Silicon | `CLIProxyAPI_{version}_darwin_aarch64.tar.gz` | `cpa-manager-plus_v{version}_darwin_arm64.tar.gz` |
| Linux | amd64 | `CLIProxyAPI_{version}_linux_amd64.tar.gz` | `cpa-manager-plus_v{version}_linux_amd64.tar.gz` |
| Linux | arm64 | `CLIProxyAPI_{version}_linux_aarch64.tar.gz` | `cpa-manager-plus_v{version}_linux_arm64.tar.gz` |

要求：

- 资产匹配必须使用规则和白名单，不能简单选择首个压缩包；
- CLIProxyAPI 默认选择带插件版本，不选择 `no-plugin` 资产；
- 不支持的平台必须明确提示，不允许尝试下载相近资产；
- 上游命名变化导致无匹配时，保留现有版本并提示“未找到适配资产”。

### FR-02 Release 检查

- 通过 GitHub Releases API 分别检查两个仓库的最新正式 Release；
- 默认忽略 draft 和 prerelease；设置中可开启接收 prerelease；
- 启动应用后延迟检查一次，之后最多每 6 小时自动检查一次；
- 支持手动检查；
- 使用语义化版本比较，兼容版本号前缀 `v`；
- GitHub API 不可用或达到限流时，不影响本地服务启动；
- 缓存最近一次成功获取的 Release 元数据和时间。

### FR-03 下载与进度

- 展示文件名、总大小、已下载大小、百分比和当前速度；
- 支持取消下载；
- 下载写入临时文件，完成前不得覆盖已安装文件；
- 支持 HTTP 重定向和合理的连接、读取超时；
- 网络中断后允许重试；MVP 可从头下载，断点续传列为增强项；
- 同一组件同一时间只允许存在一个安装或更新任务。

### FR-04 完整性校验

- 优先下载同一 Release 中的 `checksums.txt` 并执行 SHA-256 校验；
- 若 GitHub Release Asset API 提供可信 digest，可作为补充校验来源；
- checksum 缺失时默认阻止自动更新，并允许用户在明确风险提示后手动继续；
- 校验失败必须删除临时文件，不得激活新版本；
- 在本地 manifest 中保存版本、资产名、下载地址、SHA-256、安装时间和来源仓库。

### FR-05 解压与安装

- 支持 `.zip` 和 `.tar.gz`；
- 防止压缩包路径穿越，禁止文件写出目标临时目录；
- 解压后验证预期可执行文件存在；
- macOS/Linux 设置可执行权限；
- 每个版本安装到独立目录，完成后原子切换 `current` 指针或 manifest；
- 程序目录与数据目录严格分离；
- 默认保留当前版本和上一可用版本，其他旧版本按设置清理。

建议目录：

```text
<app-data>/
  components/
    cliproxyapi/
      versions/<version>/
      current.json
      component.json
    cpa-manager-plus/
      versions/<version>/
      current.json
      component.json
  data/
    cliproxyapi/
    cpa-manager-plus/
  logs/
  downloads/
  state.json
```

### FR-06 启动前检查

启动组件前必须检查：

- 已安装且可执行文件存在；
- 文件哈希与 manifest 一致，可配置为仅安装后首次检查；
- 数据目录和日志目录可写；
- 配置文件存在或可安全生成默认配置；
- 目标端口未被其他进程占用；
- 依赖组件状态满足要求。

CPA-Manager-Plus 对 CLIProxyAPI 的依赖采用“软依赖”：允许独立启动，但若配置的 CPA 地址不可达，应展示警告并提供启动 CLIProxyAPI 的快捷操作。

### FR-07 进程启动

- 使用子进程方式启动，不依赖用户打开终端；
- 指定稳定的工作目录、数据目录、配置路径和环境变量；
- 捕获 stdout、stderr 和退出码；
- 将启动时间、PID、版本、参数摘要写入运行状态；
- 启动参数由组件适配器维护，禁止 UI 自由拼接命令行；
- 启动成功以健康检查通过为准；若上游缺少稳定健康端点，可使用 TCP 端口检查和启动稳定窗口组合判定；
- 启动超时默认 15 秒，失败后终止新进程并返回分类错误。

### FR-08 停止与重启

- 优先发送优雅终止信号并等待最多 5 秒；
- 超时后强制终止进程树；
- Windows 必须处理子进程树，不能只结束父 PID；
- 重启等价于“优雅停止成功后，使用同一版本和配置重新启动”；
- 重复点击启动、停止和重启不得产生并发竞态；
- 应用不得结束仅凭进程名匹配到的外部进程。

### FR-09 进程身份与孤儿进程处理

- 状态文件至少记录 PID、进程启动时间、可执行文件绝对路径和版本；
- 应用重启后必须校验 PID 对应进程的路径和启动时间，避免 PID 复用误判；
- 若确认是本应用启动且仍在运行，应重新接管并继续健康检查；
- 若状态文件存在但进程不存在，应清理过期状态；
- 用户可选择“关闭窗口时最小化到托盘”或“退出应用并停止托管组件”；
- 默认行为：关闭窗口最小化到托盘，显式退出时询问是否停止正在运行的组件，并记住选择。

### FR-10 健康检查与状态监督

- 每个组件由适配器定义健康检查地址、端口或探测方式；
- 运行中默认每 5 秒检查一次；
- 连续 3 次失败后标记为“不健康”，但若进程仍存在，不立即误判为已停止；
- 进程退出必须立即变为“异常”或“已停止”；用户主动停止则不记为异常；
- 记录最近成功健康时间和连续失败次数；
- MVP 不自动无限重启。可提供最多 3 次、指数退避的可选崩溃恢复策略。

### FR-11 日志管理

- UI 实时展示两个组件的 stdout/stderr，支持按组件和级别筛选；
- 日志落盘并按大小轮转，默认单文件 10 MB、保留 5 份；
- 启动失败时展示最后 100 行相关日志；
- 提供复制诊断信息和打开日志目录；
- 诊断导出必须对密钥、Token、Authorization Header 和管理密码做脱敏。

### FR-12 更新事务

更新流程：

1. 获取并锁定目标 Release；
2. 下载资产和 checksum 到临时目录；
3. 校验并解压到新版本目录；
4. 验证可执行文件和 manifest；
5. 记录组件更新前是否运行；
6. 若正在运行，优雅停止；
7. 原子切换当前版本；
8. 若更新前正在运行，则启动新版本并等待健康检查；
9. 验证成功后标记为当前稳定版本；
10. 清理临时文件和超额旧版本。

更新过程中禁止再次启动、停止或更新同一组件。两个组件可以独立更新，但“全部更新”默认按 CLIProxyAPI、CPA-Manager-Plus 的顺序串行执行，降低同时不可用风险。

### FR-13 自动回滚

出现以下情况时自动回滚：

- 新版本进程创建失败；
- 新版本在启动超时内退出；
- 新版本健康检查未通过；
- 激活后的 manifest 或可执行文件校验失败。

回滚流程：停止新版本、切回上一稳定版本、按更新前运行状态重新启动、验证健康状态，并保留失败版本的日志和元数据供诊断。若旧版本也无法启动，停止自动重试并提示用户手动处理。

### FR-14 版本管理

- 查看已安装版本、安装时间、文件大小和稳定状态；
- 手动切换到保留的旧版本；
- 删除非当前、非运行中的旧版本；
- 当前版本和最后一个稳定版本不可同时被删除；
- MVP 默认最多保留 2 个版本；可在设置中调整为 2 至 5 个。

### FR-15 组合启动与管理入口

- “全部启动”按 CLIProxyAPI、CPA-Manager-Plus 顺序启动；
- “全部停止”按 CPA-Manager-Plus、CLIProxyAPI 顺序停止；
- CPA-Manager-Plus 健康后提供“打开管理页面”；
- URL 由组件配置和实际端口生成，不硬编码公网地址；
- 若管理页面不可访问，保留进程状态并展示健康诊断。

### FR-16 设置

MVP 设置项：

- 开机启动桌面管理器；
- 应用启动后自动启动选定组件；
- 关闭窗口时最小化到托盘；
- 稳定版或预发布版更新通道；
- 自动检查更新；
- 是否自动下载更新；默认关闭；
- 代理服务器，用于访问 GitHub；
- CLIProxyAPI 与 CPA-Manager-Plus 的端口、数据目录和配置路径；
- 保留版本数量；
- 崩溃后自动重启开关。

端口或路径修改只允许在对应组件停止时进行。

## 9. 组件适配器

为避免将上游差异散落在 UI 和通用流程中，每个组件实现统一适配器接口：

```text
ComponentAdapter
  id / displayName / repository
  resolveAsset(platform, arch, release)
  resolveChecksum(release)
  locateExecutable(extractedDirectory)
  buildLaunchSpec(settings, dataDirectory)
  getVersion(executable, manifest)
  preflight(settings)
  healthCheck(runtime)
  getManagementUrl(settings)
  redactLog(line)
```

通用下载器、版本仓库、进程监督器和更新事务不得包含具体组件文件名或启动参数。

### 9.1 Rust 模块划分

建议 Rust 后端按以下职责划分：

```text
src-tauri/src/
  commands/             # 暴露给前端的 Tauri Commands
  components/
    mod.rs              # ComponentAdapter trait 与公共模型
    cliproxyapi.rs       # CLIProxyAPI 资产、启动和健康检查适配
    manager_plus.rs      # CPA-Manager-Plus 适配
  releases/             # GitHub Release 查询、缓存、版本和资产解析
  downloads/            # 下载任务、进度、取消和 SHA-256 校验
  archives/             # zip/tar.gz 安全解压
  installations/        # 版本目录、manifest、原子激活与清理
  processes/            # 子进程启动、停止、进程树和身份校验
  health/               # TCP/HTTP 健康检查与监督循环
  updates/              # 更新事务和自动回滚状态机
  logs/                 # stdout/stderr 采集、轮转和脱敏
  storage/              # 设置、状态文件和原子持久化
  events.rs             # 前端事件模型
  error.rs              # 结构化错误类型和用户错误码
  state.rs              # AppState 与并发任务注册表
  lib.rs
  main.rs
```

模块之间通过 Rust 类型和 trait 协作。组件适配器只描述上游差异，更新流程、下载器和进程监督器保持通用。

### 9.2 Tauri Command 边界

前端可调用的 Command 至少包括：

```text
get_app_snapshot
check_updates
install_component
cancel_component_task
start_component
stop_component
restart_component
update_component
rollback_component
list_installed_versions
activate_version
delete_version
open_management_page
open_data_directory
open_log_directory
read_settings
update_settings
export_diagnostics
```

要求：

- Command 返回结构化结果和稳定错误码，不向前端暴露任意 Rust 错误文本；
- 长任务启动后立即返回任务 ID，进度通过事件推送；
- 前端不得传入任意可执行文件路径或任意命令行参数；
- Rust 端必须再次校验组件 ID、版本、路径、端口和状态转换；
- 所有会修改组件状态的 Command 必须经过同一组件锁，防止并发启停或更新。

### 9.3 事件模型

Rust 后端至少发布以下事件：

```text
component://state-changed
component://task-progress
component://log-appended
component://health-changed
component://update-available
component://error
```

每条事件应包含组件 ID、时间戳和与事件类型对应的结构化 payload。前端首次加载或事件流中断后，应重新调用 `get_app_snapshot` 获取完整状态，不能仅依赖事件重放。

### 9.4 并发与状态机

- `AppState` 由 Tauri Managed State 持有；
- 每个组件使用独立异步互斥锁，两个组件可并行读取状态，但同一组件的安装、更新、启动和停止必须串行；
- 下载和健康检查使用 Tokio Task，任务必须支持取消和应用退出时的受控收尾；
- 不在持有互斥锁时执行长时间网络下载；更新事务通过任务所有权和显式状态转换防止竞态；
- 组件状态转换集中定义，不允许各 Command 任意修改状态；
- Rust panic 不得跨越 Tauri Command 边界，所有预期错误转换为统一 `AppError`。

### 9.5 进程管理实现

- 使用 Rust 子进程 API直接传递可执行文件和参数数组，禁止通过 `cmd.exe`、PowerShell 或 `/bin/sh` 拼接命令；
- stdout 和 stderr 使用管道异步读取，避免缓冲区写满导致子进程阻塞；
- Windows 应使用 Job Object 或等效机制约束并终止完整进程树；
- Unix 平台应使用独立进程组并向进程组发送终止信号；
- PID、启动时间、可执行路径和当前版本共同构成进程身份；
- Tauri 应用退出时根据用户设置执行停止或脱离策略，不允许因 Rust Drop 顺序不确定而隐式决定子进程生命周期。

### 9.6 文件与更新事务实现

- 所有临时文件和新版本目录必须位于与最终安装目录相同的文件系统，以支持原子重命名；
- manifest 和状态文件采用“写临时文件、刷新、原子替换”的持久化方式；
- 更新状态机由 Rust 后端持久化阶段，至少包含 downloading、verifying、extracting、staged、switching、validating、rolling_back 和 completed；
- 应用启动时扫描未完成事务，依据阶段安全清理或回滚；
- Tauri resource 目录只存放随应用发布的只读资源，不作为可更新 binary 的写入位置；
- 可更新 binary、版本 manifest 和运行数据统一存储在 Tauri `app_data_dir` 下，并继续保持程序目录与用户数据目录分离。

### 9.7 Tauri 安全配置

- 仅启用产品实际需要的 Tauri capabilities；
- shell、文件系统、HTTP 和 opener 权限采用最小范围白名单；
- 不向前端开放通用 shell 执行能力；
- GitHub 下载由 Rust 后端执行，不允许前端 WebView 直接下载并写入 binary；
- 管理页面使用系统默认浏览器打开，外部 URL 必须由 Rust 根据已验证配置生成；
- 前端 Content Security Policy 禁止不必要的远程脚本和动态代码执行。

## 10. 数据与配置保护

- binary 更新不能覆盖 `config.yaml`、认证目录、SQLite 数据库、插件目录和用户日志；
- 首次创建默认配置前必须确认文件不存在；
- 若必须执行配置迁移，先备份并采用可重复执行的迁移逻辑；MVP 原则上不主动改写上游配置；
- 所有状态和 manifest 写入采用临时文件加原子重命名；
- 应用崩溃后重新启动时，应能识别并清理未完成的下载或安装事务；
- 路径展示和诊断信息应避免泄露用户名等非必要隐私。

## 11. 异常与错误文案

至少覆盖以下错误类别：

- GitHub 不可访问或 API 限流；
- 未找到当前平台适配资产；
- 下载超时、取消或磁盘空间不足；
- checksum 缺失或校验失败；
- 压缩包损坏或内容不符合预期；
- 安装目录无权限；
- binary 不存在或不可执行；
- 配置文件无效；
- 端口被占用或无绑定权限；
- 启动超时、进程提前退出或健康检查失败；
- 停止超时或无法结束进程树；
- 更新失败但回滚成功；
- 更新和回滚均失败。

错误提示应包含：发生了什么、当前是否仍可使用旧版本、建议操作、查看日志入口。

## 12. 非功能需求

### 12.1 安全

- 默认仅允许本地监听；
- Release 下载只接受 HTTPS GitHub 官方地址及其合法重定向；
- 必须校验资产完整性；
- 解压必须防止 Zip Slip 和符号链接逃逸；
- 启动参数不得经过 shell 字符串拼接；
- 日志和诊断包必须脱敏；
- 不收集遥测、账号或请求数据，除非未来单独取得用户授权。

### 12.2 性能

- 空闲状态 CPU 平均占用低于 1%；
- 空闲状态内存目标低于 150 MB，具体阈值随桌面技术栈调整；
- 日志视图不得因高频输出阻塞进程读取；
- Release 检查和下载不得阻塞 UI 主线程。

### 12.3 兼容性

- Windows 10/11 amd64；Windows arm64 作为同版本目标，但需真机验证；
- macOS 12+ Intel 与 Apple Silicon；
- 主流 Linux 桌面发行版 amd64/arm64；
- 文件路径必须支持空格、中文和长路径；
- Windows 不依赖 WSL，macOS/Linux 不依赖系统预装 Node.js 或 Go。

### 12.4 可维护性

- 上游资产规则、仓库地址、启动参数和健康检查均集中在适配器；
- 所有更新阶段具备结构化日志和可恢复状态；
- GitHub API 响应和资产选择规则应有固定样例测试，避免上游变更静默选错文件。
- Rust 核心模块应可脱离 Tauri Command 层进行单元测试；Command 层仅负责参数校验、调用服务和结果映射。
- 前后端共享的数据模型应由单一契约维护，并在 CI 中检查 TypeScript 与 Rust 字段一致性。

### 12.5 构建与发布

- 使用 Tauri 官方构建流程生成 Windows、macOS 和 Linux 安装包；
- CI 按目标 OS/架构构建，不进行未经验证的跨平台二进制拼装；
- Rust 编译启用锁定依赖，提交并校验 `Cargo.lock`；
- 发布前运行 Rust 格式化、Clippy、单元测试、前端 lint/typecheck/test 和 Tauri 构建；
- 应用包不固定内置两个上游 binary。MVP 默认首次运行按平台下载，以避免桌面应用与上游 binary 版本强绑定；
- 如后续提供离线包，应将预置版本导入同一版本仓库和 manifest 流程，不建立第二套安装逻辑。

## 13. MVP 用户流程

### 13.1 首次使用

1. 应用识别系统与架构。
2. 展示两个组件及其用途、最新版本和下载大小。
3. 用户点击“安装并启动全部”。
4. 应用依次下载、校验并安装 CLIProxyAPI 和 CPA-Manager-Plus。
5. 应用生成或引导选择必要配置，但不覆盖已有配置。
6. 依次启动并完成健康检查。
7. 展示“运行中”，用户点击打开管理页面。

### 13.2 更新

1. 应用提示某组件存在新版本。
2. 用户查看版本号和 Release 页面后点击更新。
3. 后台完成下载和校验，最后才短暂停止服务并切换版本。
4. 新版本健康检查通过后恢复运行状态。
5. 若失败，自动回滚并提示旧版本已恢复。

## 14. 验收标准

### AC-01 安装

- 在每个支持的 OS/架构组合上，能够选择唯一正确的 Release 资产；
- checksum 正确时完成安装，错误时拒绝安装；
- 解压后的可执行文件位于版本目录，用户数据位于独立数据目录；
- 安装中断后重新打开应用，不会将半成品识别为已安装版本。

### AC-02 启停

- 点击启动后只能产生一个受管进程实例；
- 端口占用时不启动进程，并明确显示占用错误；
- 健康检查通过后才显示运行中；
- 用户停止后进程树被完整结束；
- 应用重启后能正确识别并接管仍在运行的受管进程。

### AC-03 更新与回滚

- 更新不改变用户配置与数据文件内容；
- 更新前运行的组件在成功更新后自动恢复运行；
- 更新前停止的组件在成功更新后仍保持停止；
- 模拟新版本启动失败时，应用自动恢复旧版本及其原运行状态；
- 更新任一阶段失败时，不得留下指向不完整版本的 current 状态。

### AC-04 离线与错误恢复

- 无网络时可以正常启动、停止和使用已安装版本；
- GitHub 限流时展示最近检查时间，不清空已缓存版本信息；
- 日志可以定位到下载、校验、解压、启动、健康检查或回滚的具体失败阶段；
- 诊断导出不包含明文密钥和 Token。

## 15. 测试范围

- 资产匹配：全部 OS/架构、`aarch64`/`arm64` 差异、`no-plugin` 排除、无匹配；
- 版本比较：`v` 前缀、正式版、预发布版、降级；
- checksum 解析：常见格式、空格、星号、缺失、错误哈希；
- 解压安全：路径穿越、绝对路径、符号链接逃逸、损坏压缩包；
- 更新状态机：每一阶段失败、取消、应用崩溃后恢复；
- 进程管理：重复启动、早退、停止超时、PID 复用、孤儿进程接管；
- 健康检查：慢启动、间歇失败、端口可连但服务异常；
- 数据保护：更新、回滚和切换版本前后配置及数据库哈希不变；
- 路径兼容：中文、空格、只读目录和磁盘空间不足；
- UI：长版本号、长错误信息、窄窗口和高频日志。
- Rust 状态机：所有合法和非法状态转换、并发 Command、任务取消和应用重启恢复；
- Tauri 边界：Command 参数校验、错误码序列化、事件 payload 和 capabilities 权限；
- 平台进程行为：Windows Job Object、Unix 进程组、父进程退出和子进程树清理。

## 16. 版本规划

### MVP

- 双组件安装、启动、停止、重启；
- GitHub Release 检查与正确资产匹配；
- SHA-256 校验、解压、独立版本目录；
- 健康检查、日志、托盘和管理页面入口；
- 手动更新、自动恢复原运行状态、失败回滚；
- 基础设置和版本保留。

### V1.1

- 自动下载更新和计划更新时间；
- 断点续传、镜像源或自定义下载源；
- 更完善的崩溃自动恢复；
- 诊断包导出；
- GitHub Token 配置以提高 API 限额。

### V1.2

- 桌面应用自身更新；
- systemd、launchd、Windows Service 可选集成；
- 多实例和高级启动参数；
- 版本更新说明内嵌展示。

## 17. 风险与待验证项

1. **上游启动契约可能变化**：两个 binary 的可执行文件名、参数、默认端口和健康端点需在实施前针对固定 Release 做集成验证。
2. **CPA-Manager-Plus 运行模型**：其 native package 可能同时包含 Web 资源和 Manager Server，需确认包内目录结构、默认数据目录及与 CLIProxyAPI 的连接配置方式。
3. **checksum 格式差异**：需以两个仓库的实际 `checksums.txt` 建立解析样例测试。
4. **macOS 安全限制**：未签名 binary 可能触发 Gatekeeper；产品需提供准确提示，不能静默修改系统安全设置。
5. **Windows arm64 与 Linux arm64**：虽然上游提供资产，仍需真机或 CI 环境验证解压、权限和进程树终止行为。
6. **应用退出语义**：后台服务是否随桌面应用退出应提供明确设置，避免用户误以为关闭窗口等于停止服务。

## 18. 产品决策摘要

- 采用“桌面壳 + 两个独立受管 binary”的产品边界。
- 参考 LinJun 的子进程、配置目录、端口预检、日志和健康检查模式。
- 相比简单覆盖 binary，采用版本目录、原子切换和健康验证后的稳定标记。
- CLIProxyAPI 与 CPA-Manager-Plus 分别管理，组合操作只负责编排顺序。
- 默认不自动安装更新；自动检查、手动确认，以降低上游快速迭代带来的运行风险。
- 任何更新都必须以不覆盖用户数据和可自动回滚为前提。
