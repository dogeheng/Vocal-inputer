import { useEffect, useState } from "react";
import {
  api,
  CLEANUP_MODELS,
  CLOUD_MODELS,
  hotkeyLabel,
  LANGUAGES,
  sourceLabel,
  useEvent,
  useSettings,
  useStatus,
  type HistoryEntry,
  type Settings,
  type Status,
} from "./api";
import { Onboarding } from "./Onboarding";
import { ApiKeyPanel, HotkeyPanel, ModelPanel, TryPad } from "./panels";
import { Badge, Icon, Row, Section, StatusPill, Toggle } from "./ui";

type Page = "home" | "history" | "dictionary" | "settings";

export default function App() {
  const { status, refresh } = useStatus();
  const { settings, update } = useSettings();
  const [page, setPage] = useState<Page>("home");
  const [onboarding, setOnboarding] = useState<boolean | null>(null);

  useEffect(() => {
    if (settings && onboarding === null) setOnboarding(!settings.onboardingDone);
  }, [settings, onboarding]);

  if (!settings || onboarding === null) return <div className="loading" />;

  if (onboarding) {
    return (
      <Onboarding
        status={status}
        refresh={refresh}
        settings={settings}
        update={update}
        onDone={() => {
          update({ onboardingDone: true });
          setOnboarding(false);
        }}
      />
    );
  }

  const nav: { id: Page; label: string; icon: string }[] = [
    { id: "home", label: "概览", icon: "home" },
    { id: "history", label: "历史记录", icon: "history" },
    { id: "dictionary", label: "词典", icon: "book" },
    { id: "settings", label: "设置", icon: "gear" },
  ];

  return (
    <div className="shell">
      <aside className="sidebar" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          <span className="brand-mark"><Icon name="mic" size={16} /></span>
          VoiceInput
        </div>
        <nav>
          {nav.map((n) => (
            <button key={n.id} className={`nav-item ${page === n.id ? "active" : ""}`} onClick={() => setPage(n.id)}>
              <Icon name={n.icon} size={17} />
              {n.label}
            </button>
          ))}
        </nav>
        <div className="sidebar-foot">
          <PrivacyIndicator status={status} settings={settings} />
        </div>
      </aside>
      <main className="content">
        <div className="content-drag" data-tauri-drag-region />
        {page === "home" && <Home status={status} settings={settings} goto={setPage} restartOnboarding={() => setOnboarding(true)} />}
        {page === "history" && <History settings={settings} />}
        {page === "dictionary" && <Dictionary settings={settings} update={update} />}
        {page === "settings" && <SettingsPage status={status} refresh={refresh} settings={settings} update={update} />}
      </main>
    </div>
  );
}

function PrivacyIndicator({ status, settings }: { status: Status | null; settings: Settings }) {
  const recording = status?.phase === "recording";
  const cloud = settings.cloudEnabled && status?.apiKeySet;
  return (
    <div className="privacy">
      <div className={`privacy-line ${recording ? "rec" : ""}`}>
        <span className="dot" />
        {recording ? "正在录音" : "麦克风未使用"}
      </div>
      <div className="privacy-line muted">
        <Icon name={cloud ? "cloud" : "cpu"} size={13} />
        {cloud ? "本地 + 云端识别" : "仅本地识别"}
      </div>
    </div>
  );
}

/* ---------------- Home ---------------- */

function Home({
  status,
  settings,
  goto,
  restartOnboarding,
}: {
  status: Status | null;
  settings: Settings;
  goto: (p: Page) => void;
  restartOnboarding: () => void;
}) {
  const localOk = !!status && status.local.finalModel && status.local.preview && status.local.vad;
  const cards = [
    { icon: "mic", title: "麦克风", ok: !!status?.microphone, okText: "已允许", bad: "未授权", fix: () => api.requestMicrophone() },
    { icon: "keyboard", title: "辅助功能", ok: !!status?.accessibility, okText: "已允许", bad: "未授权", fix: () => api.requestAccessibility() },
    { icon: "sparkle", title: "快捷键", ok: !!status?.hotkeyActive, okText: hotkeyLabel(settings.hotkey), bad: "未生效", fix: () => goto("settings") },
    { icon: "cpu", title: "本地模型", ok: localOk, okText: "离线可用", bad: status?.local.loading ? "加载中" : "未下载", fix: () => goto("settings") },
    {
      icon: "cloud",
      title: "云端识别",
      ok: !!status?.apiKeySet && settings.cloudEnabled,
      okText: "已连接",
      bad: settings.cloudEnabled ? "未配置 Key" : "已关闭",
      fix: () => goto("settings"),
    },
  ];
  return (
    <div className="page">
      <header className="page-head">
        <h1>概览</h1>
        <p className="muted">在任何应用里按住 {hotkeyLabel(settings.hotkey)} 说话，松开后文字会输入到光标位置。</p>
      </header>
      <div className="status-grid">
        {cards.map((c) => (
          <button key={c.title} className={`status-card ${c.ok ? "ok" : "bad"}`} onClick={c.ok ? undefined : c.fix}>
            <Icon name={c.icon} size={18} />
            <span className="status-card-title">{c.title}</span>
            <StatusPill ok={c.ok} okText={c.okText} badText={c.bad} />
          </button>
        ))}
      </div>
      {status?.secureInput && (
        <div className="tip warn">
          当前有应用开启了“安全输入”（通常是密码框或终端的安全键盘输入），系统会阻止快捷键与文字输入。关闭该输入框后即可恢复。
        </div>
      )}
      <Section title="试一试">
        <div className="pad">
          <TryPad settings={settings} />
        </div>
      </Section>
      <div className="home-foot">
        <button className="btn ghost small" onClick={restartOnboarding}>重新查看新手引导</button>
        <span className="muted small">v{status?.version}</span>
      </div>
    </div>
  );
}

/* ---------------- History ---------------- */

function History({ settings }: { settings: Settings }) {
  const [items, setItems] = useState<HistoryEntry[]>([]);
  const [copied, setCopied] = useState<number | null>(null);
  const load = () => api.listHistory(300).then(setItems);
  useEffect(() => {
    load();
  }, []);
  useEvent("history://changed", load);

  return (
    <div className="page">
      <header className="page-head row-between">
        <div>
          <h1>历史记录</h1>
          <p className="muted">
            {settings.saveHistory ? `仅保存文字，不保存录音；保留 ${settings.historyDays} 天。` : "历史记录已关闭，不会保存任何内容。"}
          </p>
        </div>
        {items.length > 0 && (
          <button
            className="btn danger-text"
            onClick={async () => {
              await api.clearHistory();
              load();
            }}
          >
            <Icon name="trash" size={15} /> 清空全部
          </button>
        )}
      </header>
      {items.length === 0 ? (
        <div className="empty">
          <Icon name="history" size={28} />
          <span>还没有记录</span>
        </div>
      ) : (
        <div className="history-list">
          {items.map((h) => (
            <div className="history-item" key={h.id}>
              <div className="history-meta">
                <span>{new Date(h.createdAt).toLocaleString("zh-CN", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" })}</span>
                {h.appName && <span className="history-app">{h.appName}</span>}
                <Badge tone={h.source === "cloud" ? "accent" : "neutral"}>{sourceLabel(h.source)}</Badge>
                <span className="muted">{h.audioSecs.toFixed(1)} 秒 · {h.latencyMs} ms</span>
                {h.outcome === "copied" && <Badge tone="warn">仅复制</Badge>}
              </div>
              <div className="history-text">{h.text}</div>
              {h.rawText && h.rawText !== h.text && <div className="history-raw">原始识别：{h.rawText}</div>}
              <div className="history-actions">
                <button
                  className="icon-btn"
                  title="复制"
                  onClick={async () => {
                    await api.copyText(h.text);
                    setCopied(h.id);
                    setTimeout(() => setCopied(null), 1200);
                  }}
                >
                  <Icon name={copied === h.id ? "check" : "copy"} size={15} />
                </button>
                <button
                  className="icon-btn"
                  title="删除"
                  onClick={async () => {
                    await api.deleteHistory(h.id);
                    load();
                  }}
                >
                  <Icon name="trash" size={15} />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/* ---------------- Dictionary ---------------- */

function Dictionary({ settings, update }: { settings: Settings; update: (p: Partial<Settings>) => void }) {
  const [term, setTerm] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const addTerm = () => {
    const t = term.trim();
    if (!t || settings.dictionary.includes(t)) return;
    update({ dictionary: [...settings.dictionary, t] });
    setTerm("");
  };
  const addRep = () => {
    if (!from.trim()) return;
    update({ replacements: [...settings.replacements, { from: from.trim(), to: to.trim() }] });
    setFrom("");
    setTo("");
  };
  return (
    <div className="page">
      <header className="page-head">
        <h1>词典</h1>
        <p className="muted">添加人名、产品名和术语，AI 整理时会按这里的写法输出，英文词的大小写也会自动统一。</p>
      </header>
      <Section title="专有名词">
        <div className="pad">
          <div className="chips">
            {settings.dictionary.map((w) => (
              <span className="chip" key={w}>
                {w}
                <button onClick={() => update({ dictionary: settings.dictionary.filter((x) => x !== w) })}>
                  <Icon name="x" size={11} />
                </button>
              </span>
            ))}
            {settings.dictionary.length === 0 && <span className="muted small">例如：OpenRouter、Kubernetes、张伟</span>}
          </div>
          <div className="inline-form">
            <input className="input" placeholder="输入词语后回车" value={term} onChange={(e) => setTerm(e.target.value)} onKeyDown={(e) => e.key === "Enter" && addTerm()} />
            <button className="btn" onClick={addTerm}>添加</button>
          </div>
        </div>
      </Section>
      <Section title="替换规则" footer="识别结果中出现左侧文字时，替换为右侧文字（区分大小写）。离线时也会生效。">
        <div className="pad">
          {settings.replacements.map((r, i) => (
            <div className="rep-row" key={i}>
              <span className="mono">{r.from}</span>
              <Icon name="arrow-right" size={14} />
              <span className="mono">{r.to || <i className="muted">（删除）</i>}</span>
              <button className="icon-btn" onClick={() => update({ replacements: settings.replacements.filter((_, j) => j !== i) })}>
                <Icon name="trash" size={14} />
              </button>
            </div>
          ))}
          <div className="inline-form">
            <input className="input" placeholder="识别成" value={from} onChange={(e) => setFrom(e.target.value)} />
            <input className="input" placeholder="替换为" value={to} onChange={(e) => setTo(e.target.value)} onKeyDown={(e) => e.key === "Enter" && addRep()} />
            <button className="btn" onClick={addRep}>添加</button>
          </div>
        </div>
      </Section>
    </div>
  );
}

/* ---------------- Settings ---------------- */

function SettingsPage({
  status,
  refresh,
  settings,
  update,
}: {
  status: Status | null;
  refresh: () => void;
  settings: Settings;
  update: (p: Partial<Settings>) => void;
}) {
  const [proxy, setProxy] = useState(settings.proxyUrl);
  const [baseUrl, setBaseUrl] = useState(settings.cloudBaseUrl);
  const customCloud = !CLOUD_MODELS.some((m) => m.id === settings.cloudModel);
  const customCleanup = !CLEANUP_MODELS.some((m) => m.id === settings.cleanupModel);
  return (
    <div className="page">
      <header className="page-head">
        <h1>设置</h1>
      </header>

      <Section title="快捷键与输入方式">
        <div className="pad">
          <HotkeyPanel settings={settings} update={update} />
        </div>
      </Section>

      <Section title="本地识别（离线可用）">
        <div className="pad">
          <ModelPanel status={status} compact />
        </div>
      </Section>

      <Section title="云端识别">
        <Row title="使用云端识别" desc="边说边把音频分段通过 HTTPS 上传，松开后用更准确的结果替换本地结果。关闭后音频完全不出本机。">
          <Toggle checked={settings.cloudEnabled} onChange={(v) => update({ cloudEnabled: v })} />
        </Row>
        <div className="pad tight">
          <div className="row-title">OpenRouter API Key</div>
          <ApiKeyPanel status={status} onChanged={refresh} />
        </div>
        <Row title="识别模型" desc={CLOUD_MODELS.find((m) => m.id === settings.cloudModel)?.note ?? "自定义模型"}>
          <select className="select" value={customCloud ? "__custom" : settings.cloudModel} onChange={(e) => e.target.value !== "__custom" && update({ cloudModel: e.target.value })}>
            {CLOUD_MODELS.map((m) => (
              <option key={m.id} value={m.id}>{m.label}</option>
            ))}
            {customCloud && <option value="__custom">{settings.cloudModel}</option>}
          </select>
        </Row>
        <Row title="语言" desc="自动识别支持中英混说；固定语言可略微提高准确度">
          <select className="select" value={settings.language} onChange={(e) => update({ language: e.target.value })}>
            {LANGUAGES.map((l) => (
              <option key={l.id} value={l.id}>{l.label}</option>
            ))}
          </select>
        </Row>
        <Row title="等待云端结果" desc={`松开后最多等待 ${(settings.cloudDeadlineMs / 1000).toFixed(1)} 秒；超时先上屏本地结果，云端结果到达后再安全替换`}>
          <input
            type="range"
            min={300}
            max={3000}
            step={100}
            value={settings.cloudDeadlineMs}
            onChange={(e) => update({ cloudDeadlineMs: Number(e.target.value) })}
          />
        </Row>
        <Row title="迟到结果自动替换" desc="仅当光标仍在刚插入文字的末尾、且文字未被修改时才替换，否则只更新历史记录">
          <Toggle checked={settings.lateUpgradeWindowMs > 0} onChange={(v) => update({ lateUpgradeWindowMs: v ? 8000 : 0 })} />
        </Row>
      </Section>

      <Section title="文字整理">
        <Row title="AI 整理" desc="检测到语气词、改口或长段落时，用 AI 去掉口头禅、按改正后的意思整理并补全标点">
          <Toggle checked={settings.cleanupEnabled} onChange={(v) => update({ cleanupEnabled: v })} disabled={!settings.cloudEnabled} />
        </Row>
        <Row title="整理模型" desc={CLEANUP_MODELS.find((m) => m.id === settings.cleanupModel)?.note ?? "自定义模型"}>
          <select className="select" value={customCleanup ? "__custom" : settings.cleanupModel} onChange={(e) => e.target.value !== "__custom" && update({ cleanupModel: e.target.value })}>
            {CLEANUP_MODELS.map((m) => (
              <option key={m.id} value={m.id}>{m.label}</option>
            ))}
            {customCleanup && <option value="__custom">{settings.cleanupModel}</option>}
          </select>
        </Row>
        <Row title="去除语气词" desc="嗯、呃、um、uh 等（离线规则，始终生效）">
          <Toggle checked={settings.removeFillers} onChange={(v) => update({ removeFillers: v })} />
        </Row>
        <Row title="中英文之间加空格" desc="例如“和 Alex 开会”">
          <Toggle checked={settings.cjkSpacing} onChange={(v) => update({ cjkSpacing: v })} />
        </Row>
      </Section>

      <Section title="隐私">
        <Row title="保存历史记录" desc="只保存文字，不保存录音。关闭时会立即清空已有记录。">
          <Toggle checked={settings.saveHistory} onChange={(v) => update({ saveHistory: v })} />
        </Row>
        <Row title="保留天数">
          <select className="select" value={settings.historyDays} onChange={(e) => update({ historyDays: Number(e.target.value) })} disabled={!settings.saveHistory}>
            {[1, 7, 30, 90, 365].map((d) => (
              <option key={d} value={d}>{d} 天</option>
            ))}
          </select>
        </Row>
        <Row title="系统权限" desc="可随时在系统设置中撤销">
          <div className="btn-group">
            <button className="btn small" onClick={() => api.openPrivacyPane("microphone")}>麦克风</button>
            <button className="btn small" onClick={() => api.openPrivacyPane("accessibility")}>辅助功能</button>
          </div>
        </Row>
      </Section>

      <Section title="网络">
        <Row title="代理" desc="留空则使用系统代理，例如 http://127.0.0.1:7890">
          <input className="input" value={proxy} placeholder="系统代理" onChange={(e) => setProxy(e.target.value)} onBlur={() => update({ proxyUrl: proxy })} />
        </Row>
        <Row title="接口地址" desc="OpenRouter 或兼容 OpenAI 的转写接口">
          <input className="input mono" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} onBlur={() => update({ cloudBaseUrl: baseUrl })} />
        </Row>
      </Section>
    </div>
  );
}
