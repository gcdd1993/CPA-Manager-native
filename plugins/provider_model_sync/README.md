# Provider 模型同步插件

这是一个 CLIProxyAPI 动态库插件。它直接读取 CLIProxyAPI 启动参数中的 `--config` 文件，定时同步所有 `openai-compatibility` Provider 的模型，并把别名写回对应的 `models[].alias`。

插件不需要单独配置 Provider URL 或 API Key。Provider 名称、`base-url`、`api-key-entries`、`headers` 和启用状态全部复用主配置。

## 功能

- 自动读取全部 `openai-compatibility` 配置项；
- 调用每个启用 Provider 的 `<base-url>/models`；
- 支持多个 API Key，遇到 401/403 时尝试下一个；
- 按有序正则规则生成 `models[].alias`；
- 内置规则自动将模型名转小写、空格转为 `-`，并移除末尾日期后缀（如 `-0731`、`-20250731`、`-2025-07-31`）；
- 模型名含 `/` 时只保留最后一段，例如 `z-ai/glm-5.2` → `glm-5.2`；
- 可选写入 `force-mapping: true`；
- 默认保留 Provider 未返回的旧模型，避免接口暂时不完整造成配置丢失；
- 配置写入前检查并发修改，写入同目录备份并进行原子替换；
- 提供“Provider 模型同步”管理页面，展示 Provider 状态、同步错误和规则；
- 规则保存通过认证的 Management API 完成。
- 写回成功后在配置目录创建 `.provider-model-sync.restart` 请求标记；新版 CPA Manager Native 会检测该标记，并通过自身的进程生命周期管理安全重启 CLIProxyAPI。

## 构建

```bash
go test -race ./...
go vet ./...
go build -buildmode=c-shared -o bin/provider-model-sync.dll .
```

Linux 使用 `.so`，macOS 使用 `.dylib`。DLL basename 必须保持 `provider-model-sync`，否则宿主配置键无法匹配。

## 最小配置

主配置只需要启用插件：

```yaml
plugins:
  enabled: true
  configs:
    provider-model-sync:
      enabled: true
```

同步周期和别名规则可以在插件配置字段或管理页面中设置。插件会自动读取同一份 `config.yaml` 中的：

```yaml
openai-compatibility:
  - name: 薄荷 API
    base-url: https://example.com/v1
    api-key-entries:
      - api-key: ${PROVIDER_KEY}
    models:
      - name: deepseek-v4-flash
        alias: deepseek-v4-flash
```

## 别名规则

内置规则不需要配置。例如：

| Provider 模型 | 自动 alias |
| --- | --- |
| `GLM 5.2` | `glm-5.2` |
| `deepseek-v4-flash-0731` | `deepseek-v4-flash` |
| `DeepSeek V4 Flash 20250731` | `deepseek-v4-flash` |

已有非空 `alias` 会保留，不会被内置规则覆盖。

规则按顺序匹配，第一条命中后停止。字段如下：

| 字段 | 说明 |
| --- | --- |
| `enabled` | 是否启用，省略时默认启用 |
| `provider_pattern` | 匹配 `openai-compatibility[].name` 的 Go RE2 正则，空值匹配全部 |
| `model_pattern` | 匹配 Provider 返回的模型 ID |
| `alias_replacement` | 正则替换结果，支持 `$1`、`${name}` |
| `force_mapping` | 是否同时写入 `force-mapping: true` |

示例：

```yaml
plugins:
  configs:
    provider-model-sync:
      enabled: true
      sync_interval_seconds: 300
      request_timeout_seconds: 15
      include_disabled: false
      remove_missing_models: false
      alias_rules:
        - enabled: true
          provider_pattern: '^薄荷 API$'
          model_pattern: '^deepseek-(.+)$'
          alias_replacement: 'mint-deepseek-$1'
          force_mapping: true
        - provider_pattern: '.*公益.*'
          model_pattern: '^(.+)/(.+)$'
          alias_replacement: '$2'
          force_mapping: false
```

第一条规则会把 Provider 返回的 `deepseek-v4-flash` 写成：

```yaml
models:
  - name: deepseek-v4-flash
    alias: mint-deepseek-v4-flash
    force-mapping: true
```

## 管理页面

插件注册后，CLIProxyAPI 管理界面的插件菜单会出现“Provider 模型同步”。页面可以：

- 查看每个 Provider 的模型数、别名数、最近成功时间和错误；
- 增删改正则规则；
- 手动触发同步。

保存规则和手动同步需要 CLIProxyAPI Management Key。页面会同时发送 `Authorization: Bearer <key>` 和 `X-Management-Key` 请求头。

同步或保存规则后通常约 5 秒内会自动重启 CLIProxyAPI。若关闭了该组件的“随应用自动启动”，则只写入配置而不会自动拉起进程。

资源地址：

```text
/v0/resource/plugins/provider-model-sync/status
```

## 安全与失败处理

- API Key 只从主配置读取，不复制到插件配置或页面；
- 管理页面不会显示 API Key；
- 同步失败保留当前配置和旧模型；
- 配置文件被其他进程修改时，本轮写回会放弃，下一轮重试；
- 每次写回前生成 `config.yaml.provider-model-sync.bak`；
- 不同步 `disabled: true` 的 Provider，除非 `include_disabled: true`。
