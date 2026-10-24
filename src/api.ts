import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";

export type HotkeyChoice = "fn" | "rightOption" | "rightCommand" | "rightControl";
export type TriggerMode = "hybrid" | "hold" | "toggle";

export interface Replacement {
  from: string;
  to: string;
}

export interface Settings {
  onboardingDone: boolean;
  hotkey: HotkeyChoice;
  triggerMode: TriggerMode;
  tapThresholdMs: number;
  cloudEnabled: boolean;
  cloudBaseUrl: string;
  cloudApiStyle: "openrouterJson" | "openaiMultipart";
  cloudModel: string;
  cloudDeadlineMs: number;
  lateUpgradeWindowMs: number;
  language: string;
  proxyUrl: string;
  previewModel: string;
  cleanupEnabled: boolean;
  cleanupModel: string;
  cleanupTimeoutMs: number;
  cjkSpacing: boolean;
  removeFillers: boolean;
  dictionary: string[];
  replacements: Replacement[];
  saveHistory: boolean;
  historyDays: number;
  soundFeedback: boolean;
}

export interface ModelStatus {
  id: string;
  label: string;
  ready: boolean;
  size: number;
  path?: string | null;
}

export interface Status {
  accessibility: boolean;
  microphone: boolean;
  hotkeyActive: boolean;
  secureInput: boolean;
  models: ModelStatus[];
  local: { preview: boolean; finalModel: boolean; vad: boolean; loading: boolean };
  downloading: boolean;
  apiKeySet: boolean;
  keyHint?: string | null;
  phase: string;
  version: string;
}

export interface HistoryEntry {
  id: number;
  createdAt: string;
  appName: string;
  appBundle: string;
  text: string;
  rawText: string;
  source: string;
  audioSecs: number;
  latencyMs: number;
  outcome: string;
}

export interface DownloadProgress {
  model: string;
  file: string;
  downloaded: number;
  total: number;
  source: string;
}

export interface DictationResult {
  text: string;
  raw: string;
  preview: string;
  source: string;
  latencyMs: number;
  firstPartialMs: number | null;
  outcome: string;
}

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  getStatus: () => invoke<Status>("get_status"),
  requestMicrophone: () => invoke<boolean>("request_microphone"),
  requestAccessibility: () => invoke<boolean>("request_accessibility"),
  openPrivacyPane: (kind: string) => invoke<void>("open_privacy_pane", { kind }),
  setApiKey: (key: string) => invoke<string>("set_api_key", { key }),
  clearApiKey: () => invoke<void>("clear_api_key"),
  testCloud: () => invoke<string>("test_cloud"),
  downloadModels: () => invoke<void>("download_models"),
  listHistory: (limit?: number) => invoke<HistoryEntry[]>("list_history", { limit }),
  deleteHistory: (id: number) => invoke<void>("delete_history", { id }),
  clearHistory: () => invoke<void>("clear_history"),
  copyText: (text: string) => invoke<void>("copy_text", { text }),
  finishOnboarding: () => invoke<void>("finish_onboarding"),
  setPaused: (paused: boolean) => invoke<void>("set_paused", { paused }),
};

export const HOTKEYS: { id: HotkeyChoice; key: string; label: string; hint: string }[] = [
  { id: "fn", key: "fn", label: "fn / 地球仪键", hint: "Mac 内建键盘推荐。需在系统设置里把“按下 🌐 键”设为“不执行任何操作”" },
  { id: "rightOption", key: "⌥", label: "右侧 Option", hint: "外接键盘推荐，几乎不与其他快捷键冲突" },
  { id: "rightCommand", key: "⌘", label: "右侧 Command", hint: "单独按下时触发，组合键不受影响" },
  { id: "rightControl", key: "⌃", label: "右侧 Control", hint: "适合把 Caps Lock 映射为 Control 的用户" },
];

export const MODES: { id: TriggerMode; label: string; hint: string }[] = [
  { id: "hybrid", label: "按住说话 + 轻点免提", hint: "按住说话，松开结束；轻点一下进入免提，再按一下结束" },
  { id: "hold", label: "仅按住说话", hint: "松开立即结束，最不容易误触" },
  { id: "toggle", label: "按一下开始，再按一下结束", hint: "适合长时间口述" },
];

export const CLOUD_MODELS: { id: string; label: string; note: string }[] = [
  { id: "mistralai/voxtral-mini-transcribe-3-0", label: "Voxtral Mini Transcribe 3", note: "默认 · 速度快，中英混说好" },
  { id: "mistralai/voxtral-small-transcribe-3-0", label: "Voxtral Small Transcribe 3", note: "更准，稍慢" },
  { id: "openai/gpt-transcribe", label: "OpenAI GPT Transcribe", note: "英文强" },
  { id: "openai/gpt-4o-mini-transcribe", label: "GPT-4o Mini Transcribe", note: "便宜" },
  { id: "elevenlabs/scribe-v2", label: "ElevenLabs Scribe v2", note: "多语言" },
  { id: "qwen/qwen3-asr-1.7b", label: "Qwen3 ASR 1.7B", note: "中文强，价格低" },
  { id: "deepgram/nova-3", label: "Deepgram Nova-3", note: "英文低延迟" },
];

export const CLEANUP_MODELS: { id: string; label: string; note: string }[] = [
  { id: "openai/gpt-oss-20b", label: "GPT-OSS 20B", note: "默认 · 不翻译、不乱改，约 0.7 秒" },
  { id: "mistralai/mistral-small-3.2-24b-instruct", label: "Mistral Small 3.2", note: "稍慢" },
  { id: "google/gemini-2.5-flash-lite", label: "Gemini 2.5 Flash Lite", note: "快，但偶尔会翻译英文" },
];

export const LANGUAGES: { id: string; label: string }[] = [
  { id: "auto", label: "自动识别（推荐，支持中英混说）" },
  { id: "zh", label: "中文" },
  { id: "en", label: "English" },
  { id: "ja", label: "日本語" },
  { id: "ko", label: "한국어" },
  { id: "yue", label: "粤语" },
];

export function hotkeyLabel(h: HotkeyChoice): string {
  return HOTKEYS.find((x) => x.id === h)?.label ?? h;
}

export function formatBytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(0)} MB`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(0)} KB`;
  return `${n} B`;
}

export function sourceLabel(s: string): string {
  return (
    { cloud: "云端", local: "本地", mixed: "云端 + 本地", preview: "本地预览", empty: "—" } as Record<string, string>
  )[s] ?? s;
}

/** Subscribe to a backend event for the lifetime of the component. */
export function useEvent<T>(name: string, handler: (payload: T) => void) {
  const ref = useRef(handler);
  ref.current = handler;
  useEffect(() => {
    let un: UnlistenFn | undefined;
    let alive = true;
    listen<T>(name, (e) => ref.current(e.payload)).then((u) => {
      if (alive) un = u;
      else u();
    });
    return () => {
      alive = false;
      un?.();
    };
  }, [name]);
}

/** Poll backend status (permissions can change outside the app). */
export function useStatus(intervalMs = 1500) {
  const [status, setStatus] = useState<Status | null>(null);
  const refresh = () => api.getStatus().then(setStatus).catch(() => {});
  useEffect(() => {
    refresh();
    const t = setInterval(refresh, intervalMs);
    return () => clearInterval(t);
  }, [intervalMs]);
  useEvent("models://loaded", refresh);
  useEvent("models://done", refresh);
  return { status, refresh };
}

export function useSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  useEffect(() => {
    api.getSettings().then(setSettings);
  }, []);
  const update = async (patch: Partial<Settings>) => {
    if (!settings) return;
    const next = { ...settings, ...patch };
    setSettings(next);
    try {
      setSettings(await api.saveSettings(next));
    } catch (e) {
      console.error(e);
    }
  };
  return { settings, update };
}
