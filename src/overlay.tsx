import React, { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import "./overlay.css";

type Phase = "hidden" | "listening" | "processing" | "done" | "notice" | "error" | "cancelled";

interface OverlayState {
  phase: Phase;
  app?: string;
  text?: string;
  hint?: string;
  source?: string;
  cleaned?: boolean;
  latched?: boolean;
}

const BARS = 9;

function Waveform({ level }: { level: number }) {
  // Bars follow the mic level with a gentle per-bar variation.
  const [t, setT] = useState(0);
  useEffect(() => {
    let raf = 0;
    const loop = () => {
      setT(performance.now() / 1000);
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);
  const l = Math.min(1, Math.sqrt(level) * 1.6);
  return (
    <div className="wave">
      {Array.from({ length: BARS }).map((_, i) => {
        const centre = 1 - Math.abs(i - (BARS - 1) / 2) / ((BARS + 1) / 2);
        const wobble = 0.55 + 0.45 * Math.sin(t * 9 + i * 1.7);
        const h = 4 + (l * 18 * centre + 2) * wobble;
        return <span key={i} style={{ height: `${h}px` }} />;
      })}
    </div>
  );
}

function sourceText(s?: string) {
  return ({ cloud: "云端", local: "本地", mixed: "云端+本地", preview: "预览" } as Record<string, string>)[s ?? ""] ?? "";
}

function Overlay() {
  const [state, setState] = useState<OverlayState>({ phase: "hidden" });
  const [partial, setPartial] = useState("");
  const [level, setLevel] = useState(0);
  const textRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const subs = [
      listen<OverlayState>("overlay://state", (e) => {
        const p = e.payload;
        setState((prev) => {
          // "latched" updates only patch the listening state.
          if (p.phase === "listening" && prev.phase === "listening" && p.app === undefined) {
            return { ...prev, latched: p.latched };
          }
          return p;
        });
        if (p.phase === "listening" && p.app !== undefined) setPartial("");
      }),
      listen<string>("overlay://partial", (e) => setPartial(e.payload)),
      listen<number>("overlay://level", (e) => setLevel(e.payload)),
    ];
    return () => {
      subs.forEach((s) => s.then((u) => u()));
    };
  }, []);

  useEffect(() => {
    const el = textRef.current;
    if (el) el.scrollLeft = el.scrollWidth;
  }, [partial, state]);

  const { phase } = state;
  if (phase === "hidden") return null;

  let body: React.ReactNode = null;
  if (phase === "listening") {
    body = (
      <>
        <div className="lead rec">
          <span className="rec-dot" />
          <Waveform level={level} />
        </div>
        <div className="text live" ref={textRef}>
          {partial ? partial : <span className="placeholder">正在听{state.app ? ` · 输入到 ${state.app}` : "…"}</span>}
        </div>
        <div className="hint">{state.latched ? "免提中 · 再按一次结束" : "松开结束"} · esc 取消</div>
      </>
    );
  } else if (phase === "processing") {
    body = (
      <>
        <div className="lead"><span className="spinner" /></div>
        <div className="text live" ref={textRef}>
          <span className="shimmer">{partial || "正在识别…"}</span>
        </div>
        <div className="hint">esc 取消</div>
      </>
    );
  } else if (phase === "done" || phase === "notice") {
    body = (
      <>
        <div className="lead ok">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>
        </div>
        <div className="text final" ref={textRef}>{state.text}</div>
        <div className="hint">
          {state.hint || (
            <>
              {sourceText(state.source)}
              {state.cleaned ? " · 已整理" : ""}
            </>
          )}
        </div>
      </>
    );
  } else {
    body = (
      <>
        <div className={`lead ${phase === "error" ? "err" : ""}`}>
          {phase === "error" ? (
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round"><path d="M12 7v6M12 17h.01" /><circle cx="12" cy="12" r="9" /></svg>
          ) : (
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round"><path d="M6 6l12 12M18 6L6 18" /></svg>
          )}
        </div>
        <div className="text final">{state.hint}</div>
      </>
    );
  }

  return (
    <div className="stage">
      <div className={`pill ${phase}`} key={phase === "listening" ? "l" : "o"}>
        {body}
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("overlay-root") as HTMLElement).render(<Overlay />);
