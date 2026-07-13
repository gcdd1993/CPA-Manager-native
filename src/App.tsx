import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Activity,
  ArrowUpRight,
  Boxes,
  Check,
  CheckCircle2,
  CircleAlert,
  CircleStop,
  Copy,
  Download,
  ExternalLink,
  FolderOpen,
  GitFork,
  LoaderCircle,
  Moon,
  Play,
  RefreshCw,
  RotateCcw,
  ServerCog,
  Settings,
  SquareTerminal,
  Sun,
} from "lucide-react";
import { getSnapshot, isTauri, runAppCommand } from "./api";
import type {
  AppSnapshot,
  ComponentId,
  ComponentSnapshot,
  LifecycleState,
} from "./types";

const lifecycleLabels: Record<LifecycleState, string> = {
  not_installed: "未安装",
  stopped: "已停止",
  starting: "启动中",
  running: "运行中",
  stopping: "停止中",
  installing: "安装中",
  updating: "更新中",
  error: "异常",
};

function formatTime(value: string | null) {
  if (!value) return "尚未检查";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function ComponentIcon({ id }: { id: ComponentId }) {
  return id === "cliproxyapi" ? <ServerCog /> : <Boxes />;
}

function StatusPill({ component }: { component: ComponentSnapshot }) {
  const tone = component.lifecycle === "error"
    ? "danger"
    : component.lifecycle === "running" && component.healthy
      ? "success"
      : component.busy
        ? "progress"
        : "neutral";

  return (
    <span className={`status-pill ${tone}`}>
      <span className="status-dot" />
      {component.lifecycle === "running" && !component.healthy
        ? "运行中 · 未就绪"
        : lifecycleLabels[component.lifecycle]}
    </span>
  );
}

function ComponentCard({
  component,
  onAction,
}: {
  component: ComponentSnapshot;
  onAction: (command: string, id: ComponentId) => Promise<void>;
}) {
  const canOpen = component.lifecycle === "running" && component.management_url;
  const primaryCommand = component.installed_version
    ? component.update_available
      ? "install_component"
      : component.lifecycle === "running"
        ? "stop_component"
        : "start_component"
    : "install_component";
  const primaryLabel = component.installed_version
    ? component.update_available
      ? "更新"
      : component.lifecycle === "running"
        ? "停止"
        : "启动"
    : "安装";

  return (
    <article className="component-card">
      <div className="component-head">
        <div className={`component-icon ${component.id}`}>
          <ComponentIcon id={component.id} />
        </div>
        <div className="component-title">
          <div className="eyebrow">{component.short_name}</div>
          <h2>{component.name}</h2>
          <p>{component.description}</p>
        </div>
        <StatusPill component={component} />
      </div>

      <div className="component-metrics">
        <div>
          <span>当前版本</span>
          <strong>{component.installed_version ?? "未安装"}</strong>
        </div>
        <div>
          <span>最新版本</span>
          <strong>{component.latest_version ?? "等待检查"}</strong>
        </div>
        <div>
          <span>本地端口</span>
          <strong>{component.port}</strong>
        </div>
        <div>
          <span>进程</span>
          <strong>{component.pid ? `PID ${component.pid}` : "无"}</strong>
        </div>
      </div>

      {component.busy && (
        <div className="progress-block">
          <div className="progress-label">
            <span>{component.progress_label ?? "正在处理"}</span>
            <strong>{component.progress_percent ?? 0}%</strong>
          </div>
          <div className="progress-track">
            <span style={{ width: `${component.progress_percent ?? 2}%` }} />
          </div>
        </div>
      )}

      {component.last_error && (
        <div className="inline-error">
          <CircleAlert />
          <span>{component.last_error}</span>
        </div>
      )}

      <div className="component-actions">
        <button
          className="primary-button"
          disabled={component.busy}
          onClick={() => onAction(primaryCommand, component.id)}
        >
          {component.busy ? (
            <LoaderCircle className="spin" />
          ) : primaryCommand === "stop_component" ? (
            <CircleStop />
          ) : component.installed_version ? (
            <Play />
          ) : (
            <Download />
          )}
          {primaryLabel}
        </button>
        {component.installed_version && component.lifecycle !== "running" && (
          <button
            className="icon-button"
            title="重新安装当前最新版本"
            disabled={component.busy}
            onClick={() => onAction("install_component", component.id)}
          >
            <RotateCcw />
          </button>
        )}
        <button
          className="secondary-button"
          disabled={!canOpen}
          onClick={() => onAction("open_management_page", component.id)}
        >
          <ExternalLink />
          打开页面
        </button>
        <button
          className="icon-button"
          title="查看 GitHub 仓库"
          aria-label={`查看 ${component.name} GitHub 仓库`}
          onClick={() => onAction("open_repository", component.id)}
        >
          <GitFork />
        </button>
      </div>
    </article>
  );
}

export default function App() {
  const [theme, setTheme] = useState<"dark" | "light">(() => {
    const saved = localStorage.getItem("cpa-manager-theme");
    if (saved === "dark" || saved === "light") return saved;
    return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  });
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [activeView, setActiveView] = useState<"overview" | "logs" | "versions" | "settings">("overview");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const [activeLog, setActiveLog] = useState<"all" | ComponentId>("all");
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  const loadSnapshot = useCallback(async () => {
    try {
      setSnapshot(await getSnapshot());
      setError(null);
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.style.colorScheme = theme;
    localStorage.setItem("cpa-manager-theme", theme);
  }, [theme]);

  useEffect(() => {
    void loadSnapshot();

    if (!isTauri()) return;
    const unlisten = listen<AppSnapshot>("app://snapshot", (event) => {
      setSnapshot(event.payload);
    });
    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, [loadSnapshot]);

  const run = useCallback(
    async (command: string, componentId?: ComponentId) => {
      const key = componentId ? `${command}:${componentId}` : command;
      setPending(key);
      setError(null);
      try {
        setSnapshot(await runAppCommand(command, componentId));
      } catch (cause) {
        setError(String(cause));
        await loadSnapshot();
      } finally {
        setPending(null);
      }
    },
    [loadSnapshot],
  );

  const filteredLogs = useMemo(() => {
    if (!snapshot) return [];
    return snapshot.logs.filter(
      (entry) => activeLog === "all" || entry.component_id === activeLog,
    );
  }, [activeLog, snapshot]);

  const copyManagementKey = useCallback(async (key: string) => {
    try {
      await navigator.clipboard.writeText(key);
      setCopiedKey(key);
      window.setTimeout(() => setCopiedKey((current) => current === key ? null : current), 1800);
    } catch (cause) {
      setError(`复制失败：${String(cause)}`);
    }
  }, []);

  if (!snapshot) {
    return (
      <main className="loading-screen">
        <LoaderCircle className="spin" />
        <span>正在加载本地组件状态</span>
      </main>
    );
  }

  const runningCount = snapshot.components.filter(
    (component) => component.lifecycle === "running" && component.healthy,
  ).length;
  const installedCount = snapshot.components.filter(
    (component) => component.installed_version,
  ).length;

  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content">跳到主要内容</a>
      <aside className="sidebar">
        <div className="brand-mark" title="CPA Manager Native">CM</div>
        <nav aria-label="主导航">
          <button
            className={`nav-button ${activeView === "overview" ? "active" : ""}`}
            title="组件概览"
            aria-label="组件概览"
            aria-current={activeView === "overview" ? "page" : undefined}
            onClick={() => setActiveView("overview")}
          ><Activity /></button>
          <button
            className={`nav-button ${activeView === "logs" ? "active" : ""}`}
            title="运行日志"
            aria-label="运行日志"
            aria-current={activeView === "logs" ? "page" : undefined}
            onClick={() => setActiveView("logs")}
          ><SquareTerminal /></button>
          <button
            className={`nav-button ${activeView === "versions" ? "active" : ""}`}
            title="安装版本"
            aria-label="安装版本"
            aria-current={activeView === "versions" ? "page" : undefined}
            onClick={() => setActiveView("versions")}
          ><Boxes /></button>
        </nav>
        <button
          className={`nav-button settings-button ${activeView === "settings" ? "active" : ""}`}
          title="设置"
          aria-label="设置"
          aria-current={activeView === "settings" ? "page" : undefined}
          onClick={() => setActiveView("settings")}
        ><Settings /></button>
      </aside>

      <main className="main-content" id="main-content" tabIndex={-1}>
        <header className="topbar">
          <div>
            <div className="eyebrow">LOCAL CONTROL PLANE</div>
            <h1>CPA Manager Native</h1>
          </div>
          <div className="topbar-actions">
            <button
              className="theme-toggle"
              type="button"
              aria-label={theme === "dark" ? "切换到浅色主题" : "切换到深色主题"}
              aria-pressed={theme === "light"}
              title={theme === "dark" ? "切换到浅色主题" : "切换到深色主题"}
              onClick={() => setTheme((current) => current === "dark" ? "light" : "dark")}
            >
              {theme === "dark" ? <Sun /> : <Moon />}
            </button>
            <button
              className="secondary-button"
              disabled={Boolean(pending)}
              onClick={() => run("check_updates")}
            >
              <RefreshCw className={pending === "check_updates" ? "spin" : ""} />
              检查更新
            </button>
            <button
              className="primary-button"
              disabled={Boolean(pending)}
              onClick={() => run("start_all")}
            >
              <Play />
              全部启动
            </button>
          </div>
        </header>

        {error && (
          <div className="global-error" role="alert">
            <CircleAlert />
            <span>{error}</span>
            <button onClick={() => setError(null)} aria-label="关闭错误">×</button>
          </div>
        )}

        {activeView === "overview" && <>
        <section className="overview-band" aria-label="整体状态">
          <div className="overview-copy">
            <span className="section-kicker">SYSTEM STATUS</span>
            <h2>{runningCount === 2 ? "所有本地服务运行正常" : "本地服务等待操作"}</h2>
            <p>
              {installedCount}/2 已安装 · {runningCount}/2 健康运行 · {snapshot.platform} {snapshot.architecture}
            </p>
          </div>
          <div className="overview-stat">
            <CheckCircle2 />
            <span>健康组件</span>
            <strong>{runningCount}<small>/2</small></strong>
          </div>
          <div className="overview-stat">
            <RefreshCw />
            <span>上次检查</span>
            <strong className="date-value">{formatTime(snapshot.last_update_check)}</strong>
          </div>
          <button
            className="stop-all"
            disabled={Boolean(pending) || runningCount === 0}
            onClick={() => run("stop_all")}
          >
            <CircleStop />
            全部停止
          </button>
        </section>

        <section className="component-grid" aria-label="组件管理">
          {snapshot.components.map((component) => (
            <ComponentCard key={component.id} component={component} onAction={run} />
          ))}
        </section>
        </>}

        {activeView === "versions" && (
          <section className="view-section versions-section" aria-labelledby="versions-title">
            <div className="view-heading">
              <span className="section-kicker">VERSION REPOSITORY</span>
              <h2 id="versions-title">安装版本</h2>
              <p>每个组件独立安装和更新，当前版本切换不会覆盖用户数据目录。</p>
            </div>
            <div className="version-list">
              {snapshot.components.map((component) => (
                <article className="version-row" key={component.id}>
                  <div className={`component-icon ${component.id}`}><ComponentIcon id={component.id} /></div>
                  <div className="version-name">
                    <strong>{component.name}</strong>
                    <span>{component.repository}</span>
                  </div>
                  <div className="version-cell">
                    <span>当前版本</span>
                    <strong>{component.installed_version ?? "未安装"}</strong>
                  </div>
                  <div className="version-cell">
                    <span>最新版本</span>
                    <strong>{component.latest_version ?? "等待检查"}</strong>
                  </div>
                  <StatusPill component={component} />
                  <button
                    className="primary-button"
                    disabled={component.busy}
                    onClick={() => run("install_component", component.id)}
                  >
                    {component.busy ? <LoaderCircle className="spin" /> : <Download />}
                    {component.installed_version ? "重新安装 / 更新" : "安装"}
                  </button>
                  {component.id === "cliproxyapi" && component.management_key && (
                    <div className="version-secret">
                      <div>
                        <span>Remote Management Secret Key</span>
                        <code>{component.management_key}</code>
                      </div>
                      <button
                        className="secondary-button"
                        type="button"
                        aria-label="复制 CLIProxyAPI Secret Key"
                        onClick={() => copyManagementKey(component.management_key!)}
                      >
                        {copiedKey === component.management_key ? <Check /> : <Copy />}
                        {copiedKey === component.management_key ? "已复制" : "复制"}
                      </button>
                    </div>
                  )}
                  {component.id === "cliproxyapi" && component.installed_version && !component.management_key && (
                    <div className="version-secret unavailable">
                      <div>
                        <span>Remote Management Secret Key</span>
                        <p>现有 key 已由上游哈希，原文不可恢复。清空 config.yaml 中的 secret-key 后重新启动，可自动生成并保存可复制的 key。</p>
                      </div>
                    </div>
                  )}
                </article>
              ))}
            </div>
          </section>
        )}

        {activeView === "settings" && (
          <section className="view-section settings-view" aria-labelledby="settings-title">
            <div className="view-heading">
              <span className="section-kicker">PREFERENCES</span>
              <h2 id="settings-title">设置</h2>
              <p>桌面外观和本地运行目录。</p>
            </div>
            <div className="settings-grid">
              <div className="setting-row">
                <div><strong>界面主题</strong><span>偏好会保存在本机</span></div>
                <div className="segmented-control" aria-label="界面主题">
                  <button className={theme === "light" ? "active" : ""} onClick={() => setTheme("light")}><Sun />浅色</button>
                  <button className={theme === "dark" ? "active" : ""} onClick={() => setTheme("dark")}><Moon />深色</button>
                </div>
              </div>
              <div className="setting-row">
                <div><strong>应用数据目录</strong><span>组件程序、配置和日志分开存储</span></div>
                <code>{snapshot.data_directory}</code>
              </div>
              {snapshot.components.map((component) => (
                <div className="setting-row" key={component.id}>
                  <div><strong>{component.name}</strong><span>仅监听本机地址</span></div>
                  <code>127.0.0.1:{component.port}</code>
                </div>
              ))}
            </div>
          </section>
        )}

        {(activeView === "overview" || activeView === "logs") && (
        <section className={`log-section ${activeView === "logs" ? "standalone" : ""}`}>
          <div className="section-heading">
            <div>
              <span className="section-kicker">LIVE OUTPUT</span>
              <h2>{activeView === "logs" ? "组件运行日志" : "运行日志"}</h2>
            </div>
            <div className="segmented-control" aria-label="日志筛选">
              {(["all", "cliproxyapi", "cpa-manager-plus"] as const).map((value) => (
                <button
                  key={value}
                  className={activeLog === value ? "active" : ""}
                  onClick={() => setActiveLog(value)}
                >
                  {value === "all" ? "全部" : value === "cliproxyapi" ? "Core" : "Manager"}
                </button>
              ))}
            </div>
          </div>
          <div className="terminal">
            <div className="terminal-head">
              <div className="terminal-lights"><i /><i /><i /></div>
              <span>managed-process.log</span>
              <button title="打开日志目录" onClick={() => run("open_log_directory")}>
                <FolderOpen />
              </button>
            </div>
            <div className="terminal-body">
              {filteredLogs.length === 0 ? (
                <div className="empty-log">等待组件输出...</div>
              ) : (
                filteredLogs.slice(-100).map((entry, index) => (
                  <div className={`log-line ${entry.level}`} key={`${entry.timestamp}-${index}`}>
                    <time>{new Date(entry.timestamp).toLocaleTimeString("zh-CN", { hour12: false })}</time>
                    <span className="log-source">[{entry.component_id}]</span>
                    <span>{entry.message}</span>
                  </div>
                ))
              )}
            </div>
          </div>
        </section>
        )}

        <footer>
          <span>数据目录：{snapshot.data_directory}</span>
          <a href="https://github.com" target="_blank" rel="noreferrer">
            GitHub Releases <ArrowUpRight />
          </a>
        </footer>
      </main>
    </div>
  );
}
