import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { check as checkAppUpdate, type DownloadEvent } from "@tauri-apps/plugin-updater";
import {
  Activity,
  ArrowUpRight,
  Boxes,
  Check,
  CheckCircle2,
  CircleAlert,
  CircleStop,
  CloudDownload,
  CloudUpload,
  Copy,
  Download,
  ExternalLink,
  FolderOpen,
  GitFork,
  HardDrive,
  LoaderCircle,
  Monitor,
  Moon,
  Network,
  Play,
  Power,
  RefreshCw,
  RotateCcw,
  ServerCog,
  ShieldCheck,
  Settings,
  SquareTerminal,
  Sun,
} from "lucide-react";
import {
  changeDataDirectory,
  getSnapshot,
  isTauri,
  runAppCommand,
  selectDataDirectory,
  setLanAccess,
  setComponentAutoStart,
  setComponentPort,
  setLaunchAtStartup,
  saveWebDavSettings,
  syncWebDav,
  testWebDavConnection,
} from "./api";
import type {
  AppSnapshot,
  ComponentId,
  ComponentSnapshot,
  LifecycleState,
  WebDavSettings,
} from "./types";

type ThemePreference = "system" | "light" | "dark";
type ResolvedTheme = Exclude<ThemePreference, "system">;

function getSystemTheme(): ResolvedTheme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

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
  if (id === "cliproxyapi") return <ServerCog />;
  if (id === "octopus") return <Network />;
  return <Boxes />;
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
  const [theme, setTheme] = useState<ThemePreference>(() => {
    const saved = localStorage.getItem("cpa-manager-theme");
    if (saved === "system" || saved === "dark" || saved === "light") return saved;
    return "system";
  });
  const [systemTheme, setSystemTheme] = useState<ResolvedTheme>(getSystemTheme);
  const resolvedTheme = theme === "system" ? systemTheme : theme;
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [activeView, setActiveView] = useState<"overview" | "logs" | "versions" | "webdav" | "settings">("overview");
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const [activeLog, setActiveLog] = useState<"all" | ComponentId>("all");
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [selectedDataDirectory, setSelectedDataDirectory] = useState<string | null>(null);
  const [webdavDraft, setWebdavDraft] = useState<WebDavSettings | null>(null);
  const [webdavNotice, setWebdavNotice] = useState<string | null>(null);
  const [updateProgress, setUpdateProgress] = useState<string | null>(null);
  const [portDrafts, setPortDrafts] = useState<Partial<Record<ComponentId, string>>>({});

  const loadSnapshot = useCallback(async () => {
    try {
      setSnapshot(await getSnapshot());
      setError(null);
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const syncSystemTheme = (event: MediaQueryListEvent) => {
      setSystemTheme(event.matches ? "dark" : "light");
    };

    setSystemTheme(media.matches ? "dark" : "light");
    media.addEventListener("change", syncSystemTheme);
    return () => media.removeEventListener("change", syncSystemTheme);
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = resolvedTheme;
    document.documentElement.style.colorScheme = resolvedTheme;
    localStorage.setItem("cpa-manager-theme", theme);
  }, [resolvedTheme, theme]);

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

  useEffect(() => {
    if (snapshot && !webdavDraft) setWebdavDraft(snapshot.webdav);
  }, [snapshot, webdavDraft]);

  useEffect(() => {
    if (!snapshot) return;
    setPortDrafts((current) => {
      const next = { ...current };
      for (const component of snapshot.components) {
        if (next[component.id] === undefined) next[component.id] = String(component.port);
      }
      return next;
    });
  }, [snapshot]);

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

  const checkAllUpdates = useCallback(async () => {
    setPending("check_updates");
    setError(null);
    setUpdateProgress(null);
    let stoppedComponents: ComponentId[] = [];
    try {
      const next = await runAppCommand("check_updates");
      setSnapshot(next);
      if (!isTauri() || next.platform !== "Windows") return;

      const update = await checkAppUpdate({ timeout: 15_000 });
      if (!update) {
        window.alert("CPA Manager Native 已是最新版本，组件版本检查也已完成。");
        return;
      }

      const confirmed = window.confirm(
        `发现 CPA Manager Native ${update.version}。\n\n是否下载并原地更新？更新过程会自动停止组件、覆盖当前版本并重新启动，不会卸载或删除应用数据。`,
      );
      if (!confirmed) {
        await update.close();
        return;
      }

      let downloaded = 0;
      let total: number | undefined;
      setPending("download_app_update");
      setUpdateProgress("正在下载应用更新");
      await update.download((event: DownloadEvent) => {
        if (event.event === "Started") {
          total = event.data.contentLength;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          setUpdateProgress(total
            ? `正在下载应用更新 ${Math.min(100, Math.round(downloaded / total * 100))}%`
            : "正在下载应用更新");
        } else {
          setUpdateProgress("下载完成，准备安装");
        }
      });

      stoppedComponents = next.components
        .filter((component) => component.pid !== null)
        .map((component) => component.id);
      if (stoppedComponents.length > 0) {
        setSnapshot(await runAppCommand("stop_all"));
      }
      setPending("install_app_update");
      setUpdateProgress("正在原地安装并重新启动");
      await update.install();
    } catch (cause) {
      if (stoppedComponents.length > 0) {
        try {
          let restored = await getSnapshot();
          for (const componentId of stoppedComponents) {
            restored = await runAppCommand("start_component", componentId);
          }
          setSnapshot(restored);
        } catch {
          await loadSnapshot();
        }
      }
      setError(`检查或安装应用更新失败：${String(cause)}`);
    } finally {
      setPending(null);
      setUpdateProgress(null);
    }
  }, [loadSnapshot]);

  const chooseDataDirectory = useCallback(async () => {
    setPending("select_data_directory");
    setError(null);
    try {
      const selected = await selectDataDirectory();
      if (selected) setSelectedDataDirectory(selected);
    } catch (cause) {
      setError(String(cause));
      await loadSnapshot();
    } finally {
      setPending(null);
    }
  }, [loadSnapshot]);

  const migrateDataDirectory = useCallback(async () => {
    if (!selectedDataDirectory) return;
    setPending("change_data_directory");
    setError(null);
    try {
      setSnapshot(await changeDataDirectory(selectedDataDirectory));
      setSelectedDataDirectory(null);
    } catch (cause) {
      setError(String(cause));
      await loadSnapshot();
    } finally {
      setPending(null);
    }
  }, [loadSnapshot, selectedDataDirectory]);

  const toggleLaunchAtStartup = useCallback(async (enabled: boolean) => {
    setPending("set_launch_at_startup");
    setError(null);
    try {
      setSnapshot(await setLaunchAtStartup(enabled));
    } catch (cause) {
      setError(String(cause));
      await loadSnapshot();
    } finally {
      setPending(null);
    }
  }, [loadSnapshot]);

  const toggleLanAccess = useCallback(async (enabled: boolean) => {
    setPending("set_lan_access");
    setError(null);
    try {
      setSnapshot(await setLanAccess(enabled));
    } catch (cause) {
      setError(String(cause));
      await loadSnapshot();
    } finally {
      setPending(null);
    }
  }, [loadSnapshot]);

  const toggleComponentAutoStart = useCallback(async (componentId: ComponentId, enabled: boolean) => {
    setPending(`set_component_auto_start:${componentId}`);
    setError(null);
    try {
      setSnapshot(await setComponentAutoStart(componentId, enabled));
    } catch (cause) {
      setError(String(cause));
      await loadSnapshot();
    } finally {
      setPending(null);
    }
  }, [loadSnapshot]);

  const saveComponentPort = useCallback(async (componentId: ComponentId) => {
    const value = Number(portDrafts[componentId]);
    if (!Number.isInteger(value) || value < 1 || value > 65535) {
      setError("端口必须是 1 到 65535 之间的整数");
      return;
    }
    setPending(`set_component_port:${componentId}`);
    setError(null);
    try {
      const next = await setComponentPort(componentId, value);
      setSnapshot(next);
      setPortDrafts((current) => ({ ...current, [componentId]: String(value) }));
    } catch (cause) {
      setError(String(cause));
      await loadSnapshot();
    } finally {
      setPending(null);
    }
  }, [loadSnapshot, portDrafts]);

  const runWebDav = useCallback(async (action: "save" | "test" | "upload" | "download") => {
    if (!webdavDraft) return;
    if (action === "download" && !window.confirm("将用 WebDAV 中的配置覆盖本机配置。程序文件不会受影响，原配置会先备份。是否继续？")) return;
    setPending(`webdav_${action}`);
    setError(null);
    setWebdavNotice(null);
    try {
      if (action === "test") {
        await testWebDavConnection(webdavDraft);
        setWebdavNotice("连接成功，WebDAV 目录可访问。");
      } else {
        const saved = await saveWebDavSettings(webdavDraft);
        if (action === "save") {
          setSnapshot(saved);
          setWebdavDraft(saved.webdav);
          setWebdavNotice("WebDAV 设置已保存。");
        } else {
          const next = await syncWebDav(action);
          setSnapshot(next);
          setWebdavDraft(next.webdav);
          setWebdavNotice(action === "upload" ? "配置已上传到 WebDAV。" : "配置已从 WebDAV 恢复，本地原文件已备份。");
        }
      }
    } catch (cause) {
      setError(String(cause));
      await loadSnapshot();
    } finally {
      setPending(null);
    }
  }, [loadSnapshot, webdavDraft]);

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
      <div className="sr-only" aria-live="polite">
        {pending ? "正在处理设置或组件命令" : "空闲"}
      </div>
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
          <button
            className={`nav-button ${activeView === "webdav" ? "active" : ""}`}
            title="WebDAV 同步"
            aria-label="WebDAV 同步"
            aria-current={activeView === "webdav" ? "page" : undefined}
            onClick={() => setActiveView("webdav")}
          ><CloudUpload /></button>
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
              aria-label={theme === "system"
                ? `当前跟随系统（${resolvedTheme === "dark" ? "深色" : "浅色"}），点击切换到${resolvedTheme === "dark" ? "浅色" : "深色"}主题`
                : `切换到${resolvedTheme === "dark" ? "浅色" : "深色"}主题`}
              title={theme === "system"
                ? `跟随系统 · 当前${resolvedTheme === "dark" ? "深色" : "浅色"}`
                : `切换到${resolvedTheme === "dark" ? "浅色" : "深色"}主题`}
              onClick={() => setTheme(resolvedTheme === "dark" ? "light" : "dark")}
            >
              {theme === "system" ? <Monitor /> : resolvedTheme === "dark" ? <Sun /> : <Moon />}
            </button>
            <button
              className="secondary-button"
              disabled={Boolean(pending)}
              onClick={checkAllUpdates}
            >
              <RefreshCw className={pending?.includes("update") ? "spin" : ""} />
              {updateProgress ?? "检查更新"}
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

        {activeView === "webdav" && webdavDraft && (
          <section className="view-section webdav-view" aria-labelledby="webdav-title">
            <div className="view-heading">
              <span className="section-kicker">CONFIGURATION SYNC</span>
              <h2 id="webdav-title">WebDAV 同步</h2>
              <p>在设备之间手动上传和恢复 CPA 相关配置。</p>
            </div>
              <section className="webdav-panel" aria-label="WebDAV 配置">
                <div className="webdav-heading">
                  <div className="setting-icon"><CloudUpload /></div>
                  <div>
                    <h3>同步设置</h3>
                    <p>仅同步本程序及各托管组件的配置文件，不包含程序文件或数据库。</p>
                  </div>
                </div>
                <div className="webdav-form">
                  <label><span>WebDAV 地址</span><input type="url" placeholder="https://dav.example.com/dav/" value={webdavDraft.base_url} onChange={(event) => setWebdavDraft({ ...webdavDraft, base_url: event.target.value })} /></label>
                  <label><span>用户名</span><input autoComplete="username" value={webdavDraft.username} onChange={(event) => setWebdavDraft({ ...webdavDraft, username: event.target.value })} /></label>
                  <label><span>密码 / 应用密码</span><input type="password" autoComplete="current-password" value={webdavDraft.password} onChange={(event) => setWebdavDraft({ ...webdavDraft, password: event.target.value })} /></label>
                  <label><span>远端文件路径</span><input value={webdavDraft.remote_path} onChange={(event) => setWebdavDraft({ ...webdavDraft, remote_path: event.target.value })} /></label>
                </div>
                <div className="webdav-meta">
                  <ShieldCheck />
                  <span>下载前需停止所有组件；覆盖前会备份到固定配置目录的 webdav-backups 文件夹。</span>
                  <span>上次同步：{formatTime(snapshot.webdav.last_sync_at)}</span>
                </div>
                {snapshot.webdav.last_error && <div className="inline-error webdav-error"><CircleAlert /><span>{snapshot.webdav.last_error}</span></div>}
                {webdavNotice && <div className="webdav-notice" role="status"><CheckCircle2 />{webdavNotice}</div>}
                <div className="webdav-actions">
                  <button className="secondary-button" disabled={Boolean(pending)} onClick={() => runWebDav("test")}>{pending === "webdav_test" ? <LoaderCircle className="spin" /> : <Activity />}测试连接</button>
                  <button className="secondary-button" disabled={Boolean(pending)} onClick={() => runWebDav("save")}>{pending === "webdav_save" ? <LoaderCircle className="spin" /> : <Check />}保存设置</button>
                  <button className="primary-button" disabled={Boolean(pending)} onClick={() => runWebDav("upload")}>{pending === "webdav_upload" ? <LoaderCircle className="spin" /> : <CloudUpload />}上传配置</button>
                  <button className="secondary-button danger-outline" disabled={Boolean(pending)} onClick={() => runWebDav("download")}>{pending === "webdav_download" ? <LoaderCircle className="spin" /> : <CloudDownload />}下载并覆盖</button>
                </div>
              </section>
          </section>
        )}

        {activeView === "settings" && (
          <section className="view-section settings-view" aria-labelledby="settings-title">
            <div className="view-heading">
              <span className="section-kicker">PREFERENCES</span>
              <h2 id="settings-title">设置</h2>
              <p>桌面外观、本地运行与数据目录。</p>
            </div>
            <div className="settings-grid">
              <div className="setting-row">
                <div><strong>界面主题</strong><span>偏好会保存在本机</span></div>
                <div className="segmented-control" aria-label="界面主题">
                  <button aria-pressed={theme === "system"} className={theme === "system" ? "active" : ""} onClick={() => setTheme("system")}><Monitor />跟随系统</button>
                  <button aria-pressed={theme === "light"} className={theme === "light" ? "active" : ""} onClick={() => setTheme("light")}><Sun />浅色</button>
                  <button aria-pressed={theme === "dark"} className={theme === "dark" ? "active" : ""} onClick={() => setTheme("dark")}><Moon />深色</button>
                </div>
              </div>
              <div className="setting-row">
                <div><strong>开机自启</strong><span>{snapshot.launch_at_startup ? "已随系统启动" : "仅手动启动"}</span></div>
                <button
                  className={`switch-control ${snapshot.launch_at_startup ? "on" : ""}`}
                  type="button"
                  role="switch"
                  aria-checked={snapshot.launch_at_startup}
                  disabled={pending === "set_launch_at_startup"}
                  onClick={() => toggleLaunchAtStartup(!snapshot.launch_at_startup)}
                >
                  <span className="switch-track" aria-hidden="true"><span /></span>
                  <Power />
                  {snapshot.launch_at_startup ? "已开启" : "已关闭"}
                </button>
              </div>
              <div className="setting-row">
                <div>
                  <strong>允许局域网访问</strong>
                  <span>{snapshot.lan_access_enabled ? "API 与管理页面允许局域网连接" : "API 与管理页面仅允许本机访问"}</span>
                </div>
                <button
                  className={`switch-control ${snapshot.lan_access_enabled ? "on" : ""}`}
                  type="button"
                  role="switch"
                  aria-label="允许局域网访问 CLIProxyAPI"
                  aria-checked={snapshot.lan_access_enabled}
                  disabled={pending === "set_lan_access"}
                  onClick={() => toggleLanAccess(!snapshot.lan_access_enabled)}
                >
                  <span className="switch-track" aria-hidden="true"><span /></span>
                  {pending === "set_lan_access" ? <LoaderCircle className="spin" /> : <ServerCog />}
                  {snapshot.lan_access_enabled ? "已开启" : "已关闭"}
                </button>
              </div>
              <div className="setting-row directory-row">
                <div className="setting-label">
                  <span className="setting-icon"><HardDrive /></span>
                  <div><strong>应用数据目录</strong><span>组件程序、配置、日志</span></div>
                </div>
                <div className="directory-actions">
                  <code>{snapshot.data_directory}</code>
                  <button
                    className="secondary-button"
                    type="button"
                    disabled={Boolean(pending)}
                    onClick={chooseDataDirectory}
                  >
                    {pending === "select_data_directory" ? <LoaderCircle className="spin" /> : <FolderOpen />}
                    选择目录
                  </button>
                </div>
              </div>
              {selectedDataDirectory && selectedDataDirectory !== snapshot.data_directory && (
                <div className="setting-row migration-row">
                  <div><strong>待切换目录</strong><span>迁移完成后原目录保留</span></div>
                  <div className="directory-actions">
                    <code>{selectedDataDirectory}</code>
                    <button
                      className="primary-button"
                      type="button"
                      disabled={Boolean(pending)}
                      onClick={migrateDataDirectory}
                    >
                      {pending === "change_data_directory" ? <LoaderCircle className="spin" /> : <CheckCircle2 />}
                      迁移并切换
                    </button>
                  </div>
                </div>
              )}
              {selectedDataDirectory === snapshot.data_directory && (
                <div className="setting-row migration-row muted-row">
                  <div><strong>待切换目录</strong><span>已是当前目录</span></div>
                  <code>{selectedDataDirectory}</code>
                </div>
              )}
              <div className="setting-row">
                <div><strong>固定配置目录</strong><span>Manager 设置文件</span></div>
                <code>{snapshot.manager_config_directory}</code>
              </div>
              {snapshot.components.map((component) => {
                const lanEnabled = component.id === "cliproxyapi" && snapshot.lan_access_enabled;
                const autoStartPending = pending === `set_component_auto_start:${component.id}`;
                const portPending = pending === `set_component_port:${component.id}`;
                const portChanged = portDrafts[component.id] !== String(component.port);
                return (
                  <div className="setting-row component-setting-row" key={component.id}>
                    <div>
                      <strong>{component.name}</strong>
                      <span>{lanEnabled ? "监听所有网络接口" : "仅监听本机地址"}</span>
                    </div>
                    <div className="component-setting-actions">
                      <button
                        className={`switch-control ${component.auto_start ? "on" : ""}`}
                        type="button"
                        role="switch"
                        aria-label={`${component.name} 随应用自动启动`}
                        aria-checked={component.auto_start}
                        disabled={Boolean(pending)}
                        onClick={() => toggleComponentAutoStart(component.id, !component.auto_start)}
                      >
                        <span className="switch-track" aria-hidden="true"><span /></span>
                        {autoStartPending ? <LoaderCircle className="spin" /> : <Power />}
                        自动启动
                      </button>
                      <label className="port-control">
                        <span>端口</span>
                        <input
                          type="number"
                          min={1}
                          max={65535}
                          value={portDrafts[component.id] ?? component.port}
                          disabled={Boolean(pending)}
                          onChange={(event) => setPortDrafts((current) => ({ ...current, [component.id]: event.target.value }))}
                          onKeyDown={(event) => { if (event.key === "Enter") void saveComponentPort(component.id); }}
                        />
                      </label>
                      <button
                        className="secondary-button"
                        type="button"
                        disabled={Boolean(pending) || !portChanged}
                        onClick={() => saveComponentPort(component.id)}
                      >
                        {portPending ? <LoaderCircle className="spin" /> : <Check />}
                        应用
                      </button>
                    </div>
                  </div>
                );
              })}
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
              {(["all", "cliproxyapi", "cpa-manager-plus", "octopus"] as const).map((value) => (
                <button
                  key={value}
                  className={activeLog === value ? "active" : ""}
                  onClick={() => setActiveLog(value)}
                >
                  {value === "all"
                    ? "全部"
                    : value === "cliproxyapi"
                      ? "Core"
                      : value === "cpa-manager-plus"
                        ? "Manager"
                        : "Octopus"}
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
