import { useState } from "react";
import { api, hotkeyLabel, type Settings, type Status } from "./api";
import { ApiKeyPanel, HotkeyPanel, ModelPanel, TryPad } from "./panels";
import { Icon, StatusPill } from "./ui";

const STEPS = ["欢迎", "权限", "本地模型", "云端识别", "快捷键", "试一试"];

export function Onboarding({
  status,
  refresh,
  settings,
  update,
  onDone,
}: {
  status: Status | null;
  refresh: () => void;
  settings: Settings;
  update: (p: Partial<Settings>) => void;
  onDone: () => void;
}) {
  const [step, setStep] = useState(0);
  const next = () => setStep((s) => Math.min(STEPS.length - 1, s + 1));
  const back = () => setStep((s) => Math.max(0, s - 1));
  const localReady = !!status && status.local.finalModel && status.local.preview && status.local.vad;
  const permsOk = !!status && status.microphone && status.accessibility;

  const finish = async () => {
    await api.finishOnboarding();
    onDone();
  };

  return (
    <div className="onboarding">
      <div className="ob-drag" data-tauri-drag-region />
      <ol className="ob-steps">
        {STEPS.map((s, i) => (
          <li key={s} className={i === step ? "current" : i < step ? "done" : ""}>
            <span className="ob-step-dot">{i < step ? <Icon name="check" size={12} /> : i + 1}</span>
            {s}
          </li>
        ))}
      </ol>

      <div className="ob-body">
        {step === 0 && (
          <div className="ob-page welcome">
            <div className="hero-mark">
              <Icon name="mic" size={34} />
            </div>
            <h1>在任何应用里，用说的代替打字</h1>
            <p className="lead">
              按住快捷键说话，松开后整理好的文字会直接出现在光标位置。本地模型边说边识别，云端模型在后台把结果修得更准。
            </p>
            <div className="promises">
              <div className="promise">
                <Icon name="mic" />
                <div>
                  <b>只在你按住快捷键时录音</b>
                  <span>录音时屏幕底部始终显示指示器，松开或按 esc 立即停止。</span>
                </div>
              </div>
              <div className="promise">
                <Icon name="shield" />
                <div>
                  <b>录音不落盘</b>
                  <span>音频只在内存中处理；历史记录只保存文字，可随时关闭或清空。</span>
                </div>
              </div>
              <div className="promise">
                <Icon name="lock" />
                <div>
                  <b>密钥保存在钥匙串</b>
                  <span>API Key 只存在本机 macOS 钥匙串里，不写入任何文件，也不会显示在界面上。</span>
                </div>
              </div>
            </div>
          </div>
        )}

        {step === 1 && (
          <div className="ob-page">
            <h2>需要两项系统权限</h2>
            <p className="lead">macOS 会为每项权限弹出确认。授权后这里会自动更新，不需要重启 App。</p>
            <div className="perm-list">
              <div className="perm">
                <div className="perm-icon"><Icon name="mic" /></div>
                <div className="perm-text">
                  <b>麦克风</b>
                  <span>用来听你说话。只在按住快捷键期间开启。</span>
                </div>
                {status?.microphone ? (
                  <StatusPill ok okText="已允许" badText="" />
                ) : (
                  <button className="btn primary" onClick={() => api.requestMicrophone().then(refresh)}>允许访问</button>
                )}
              </div>
              <div className="perm">
                <div className="perm-icon"><Icon name="keyboard" /></div>
                <div className="perm-text">
                  <b>辅助功能</b>
                  <span>用来监听语音快捷键，并把文字输入到当前输入框。不会读取你的键盘输入内容。</span>
                </div>
                {status?.accessibility ? (
                  <StatusPill ok okText="已允许" badText="" />
                ) : (
                  <button className="btn primary" onClick={() => api.requestAccessibility().then(refresh)}>打开系统设置</button>
                )}
              </div>
            </div>
            {!status?.accessibility && (
              <div className="tip">
                在“系统设置 → 隐私与安全性 → 辅助功能”中打开 <b>VoiceInput</b> 的开关。如果列表里已有 VoiceInput 但不生效，先用“−”删除再重新添加。
              </div>
            )}
          </div>
        )}

        {step === 2 && (
          <div className="ob-page">
            <h2>下载本地识别模型</h2>
            <p className="lead">
              本地模型负责边说边出字，离线时也能完整使用。共约 440 MB，只需下载一次。
            </p>
            <ModelPanel status={status} />
          </div>
        )}

        {step === 3 && (
          <div className="ob-page">
            <h2>云端精识别（可选）</h2>
            <p className="lead">
              配置 OpenRouter API Key 后，说话时音频会分段通过 HTTPS 上传到云端，专有名词、中英混说和标点更准确；网络不好时自动回退到本地结果。
            </p>
            <ApiKeyPanel status={status} onChanged={refresh} />
            <label className="check-line">
              <input type="checkbox" checked={settings.cloudEnabled} onChange={(e) => update({ cloudEnabled: e.target.checked })} />
              使用云端识别和 AI 整理（关闭后完全离线，音频不出本机）
            </label>
          </div>
        )}

        {step === 4 && (
          <div className="ob-page">
            <h2>选择语音快捷键</h2>
            <p className="lead">单独按下一个修饰键来说话，不影响它在组合键里的正常用途。</p>
            {!status?.hotkeyActive && (
              <div className="tip warn">快捷键监听尚未生效，请先在上一步授予“辅助功能”权限。</div>
            )}
            <HotkeyPanel settings={settings} update={update} />
          </div>
        )}

        {step === 5 && (
          <div className="ob-page">
            <h2>试一试</h2>
            <p className="lead">
              点一下下面的输入框，然后按住 <b>{hotkeyLabel(settings.hotkey)}</b> 说话。屏幕底部会出现实时字幕。
            </p>
            <TryPad settings={settings} autoFocus />
            {!localReady && !status?.apiKeySet && (
              <div className="tip warn">还没有可用的识别引擎：请下载本地模型或填写 API Key。</div>
            )}
          </div>
        )}
      </div>

      <div className="ob-footer">
        {step > 0 ? <button className="btn ghost" onClick={back}>上一步</button> : <span />}
        <div className="ob-footer-right">
          {step === 1 && !permsOk && <span className="muted small">也可以稍后再授权</span>}
          {step === 2 && !localReady && <button className="btn ghost" onClick={next}>暂不下载</button>}
          {step < STEPS.length - 1 ? (
            <button className="btn primary" onClick={next}>
              {step === 0 ? "开始设置" : "继续"} <Icon name="arrow-right" size={15} />
            </button>
          ) : (
            <button className="btn primary" onClick={finish}>完成</button>
          )}
        </div>
      </div>
    </div>
  );
}
