import { useEffect, useRef, useState } from "react";
import {
  api,
  formatBytes,
  HOTKEYS,
  hotkeyLabel,
  MODES,
  sourceLabel,
  useEvent,
  type DictationResult,
  type DownloadProgress,
  type Settings,
  type Status,
} from "./api";
import { Badge, Icon, Kbd, Progress } from "./ui";

/* ---------------- Local models ---------------- */

export function ModelPanel({ status, compact }: { status: Status | null; compact?: boolean }) {
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});
  const [error, setError] = useState<string | null>(null);
  const downloading = status?.downloading ?? false;
  useEvent<DownloadProgress>("models://progress", (p) => setProgress((x) => ({ ...x, [p.model]: p })));
  useEvent<{ ok: boolean; error?: string | null }>("models://done", (r) => {
    if (!r.ok) setError(r.error ?? "下载失败");
  });
  const models = status?.models ?? [];
  const missing = models.filter((m) => !m.ready);
  const totalMissing = missing.reduce((a, m) => a + m.size, 0);

  return (
    <div className={`models ${compact ? "compact" : ""}`}>
      {models.map((m) => {
        const p = progress[m.id];
        const frac = p && p.total ? p.downloaded / p.total : 0;
        return (
          <div className="model-row" key={m.id}>
            <div className="model-icon">
              <Icon name={m.id === "vad" ? "mic" : "cpu"} />
            </div>
            <div className="model-text">
              <div className="model-title">
                {m.label}
                {m.ready ? <Badge tone="ok">已就绪</Badge> : <Badge>{m.size ? formatBytes(m.size) : "约 2 MB"}</Badge>}
              </div>
              {!m.ready && downloading && p && (
                <div className="model-progress">
                  <Progress value={frac} />
                  <span className="muted small">
                    {formatBytes(p.downloaded)} / {formatBytes(p.total)} · {p.source}
                  </span>
                </div>
              )}
            </div>
          </div>
        );
      })}
      {missing.length > 0 && (
        <div className="model-actions">
          <button className="btn primary" disabled={downloading} onClick={() => { setError(null); api.downloadModels(); }}>
            <Icon name="download" size={16} />
            {downloading ? "正在下载…" : `下载本地模型（${formatBytes(totalMissing)}）`}
          </button>
          <span className="muted small">自动选择最快的线路：Hugging Face 或国内镜像 hf-mirror.com</span>
        </div>
      )}
      {error && <div className="inline-error">{error}，请检查网络后重试。</div>}
      {status?.local.loading && <div className="muted small">正在加载本地模型…</div>}
    </div>
  );
}

/* ---------------- Cloud key ---------------- */

export function ApiKeyPanel({ status, onChanged }: { status: Status | null; onChanged: () => void }) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  const save = async () => {
    setBusy(true);
    setMsg(null);
    try {
      const summary = await api.setApiKey(value);
      setMsg({ ok: true, text: summary });
      setValue("");
      setEditing(false);
      onChanged();
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
    } finally {
      setBusy(false);
    }
  };
  const test = async () => {
    setBusy(true);
    setMsg(null);
    try {
      setMsg({ ok: true, text: await api.testCloud() });
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const hasKey = status?.apiKeySet && !editing;
  return (
    <div className="apikey">
      {hasKey ? (
        <div className="apikey-row">
          <div className="apikey-chip">
            <Icon name="lock" size={15} />
            <span className="mono">{status?.keyHint}</span>
            <span className="muted small">保存在 macOS 钥匙串</span>
          </div>
          <div className="btn-group">
            <button className="btn" disabled={busy} onClick={test}>测试连接</button>
            <button className="btn" onClick={() => setEditing(true)}>更换</button>
            <button
              className="btn danger-text"
              onClick={async () => {
                await api.clearApiKey();
                setMsg(null);
                onChanged();
              }}
            >
              移除
            </button>
          </div>
        </div>
      ) : (
        <div className="apikey-row">
          <input
            className="input mono"
            type="password"
            autoComplete="off"
            spellCheck={false}
            placeholder="sk-or-v1-…"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && value.trim() && save()}
          />
          <button className="btn primary" disabled={busy || !value.trim()} onClick={save}>
            {busy ? "验证中…" : "保存并验证"}
          </button>
          {editing && <button className="btn" onClick={() => setEditing(false)}>取消</button>}
        </div>
      )}
      {msg && <div className={msg.ok ? "inline-ok" : "inline-error"}>{msg.text}</div>}
    </div>
  );
}

/* ---------------- Hotkey ---------------- */

export function HotkeyPanel({ settings, update }: { settings: Settings; update: (p: Partial<Settings>) => void }) {
  const [seen, setSeen] = useState<string | null>(null);
  const timer = useRef<number | undefined>(undefined);
  useEvent<string>("hotkey://event", (e) => {
    if (e === "TriggerDown" || e === "TriggerUp") {
      setSeen(e);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setSeen(null), 1500);
    }
  });
  return (
    <div className="hotkey-panel">
      <div className="choice-grid">
        {HOTKEYS.map((h) => (
          <button
            key={h.id}
            className={`choice ${settings.hotkey === h.id ? "selected" : ""}`}
            onClick={() => update({ hotkey: h.id })}
          >
            <span className="choice-key">{h.key}</span>
            <span className="choice-label">{h.label}</span>
            <span className="choice-hint">{h.hint}</span>
          </button>
        ))}
      </div>
      <div className={`key-detector ${seen ? "active" : ""}`}>
        {seen ? (
          <>
            <Icon name="check" size={16} /> 检测到 {hotkeyLabel(settings.hotkey)}
            {seen === "TriggerDown" ? " 按下" : " 松开"}
          </>
        ) : (
          <>按一下 <Kbd>{HOTKEYS.find((h) => h.id === settings.hotkey)?.key}</Kbd> 检测快捷键是否生效</>
        )}
      </div>
      <div className="mode-list">
        {MODES.map((m) => (
          <label key={m.id} className={`mode ${settings.triggerMode === m.id ? "selected" : ""}`}>
            <input type="radio" name="mode" checked={settings.triggerMode === m.id} onChange={() => update({ triggerMode: m.id })} />
            <span>
              <span className="mode-label">{m.label}</span>
              <span className="mode-hint">{m.hint}</span>
            </span>
          </label>
        ))}
      </div>
    </div>
  );
}

/* ---------------- Try it ---------------- */

export function TryPad({ settings, autoFocus }: { settings: Settings; autoFocus?: boolean }) {
  const [last, setLast] = useState<DictationResult | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);
  useEvent<DictationResult>("dictation://result", setLast);
  useEffect(() => {
    if (autoFocus) ref.current?.focus();
  }, [autoFocus]);
  const key = HOTKEYS.find((h) => h.id === settings.hotkey)?.key ?? "fn";
  return (
    <div className="trypad">
      <div className="trypad-hint">
        {settings.triggerMode === "toggle" ? (
          <>按一下 <Kbd>{key}</Kbd> 开始说话，再按一下结束</>
        ) : (
          <>按住 <Kbd>{key}</Kbd> 说一句话，松开后文字会出现在下面</>
        )}
        <span className="muted"> · 说话中按 <Kbd>esc</Kbd> 取消</span>
      </div>
      <textarea
        ref={ref}
        className="trypad-input"
        placeholder="例如：明天下午三点和 Alex 开 Q4 review 的会，记得带上部署文档。"
      />
      {last && (
        <div className="trypad-metrics">
          <Badge tone="accent">{sourceLabel(last.source)}</Badge>
          <span>松开到上屏 {last.latencyMs} ms</span>
          {last.firstPartialMs != null && <span>首字 {last.firstPartialMs} ms</span>}
          {last.raw !== last.text && (
            <span className="muted ellipsis" title={last.raw}>原始识别：{last.raw}</span>
          )}
        </div>
      )}
    </div>
  );
}
