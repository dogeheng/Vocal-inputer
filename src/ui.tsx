import type { ReactNode } from "react";

export function Icon({ name, size = 18 }: { name: string; size?: number }) {
  const p = { width: size, height: size, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 1.8, strokeLinecap: "round" as const, strokeLinejoin: "round" as const };
  switch (name) {
    case "mic":
      return (
        <svg {...p}>
          <rect x="9" y="3" width="6" height="11" rx="3" />
          <path d="M5 11a7 7 0 0 0 14 0M12 18v3M8.5 21h7" />
        </svg>
      );
    case "shield":
      return (
        <svg {...p}>
          <path d="M12 3l7 3v5c0 4.5-3 8.3-7 10-4-1.7-7-5.5-7-10V6l7-3z" />
          <path d="M9 12l2 2 4-4" />
        </svg>
      );
    case "keyboard":
      return (
        <svg {...p}>
          <rect x="2.5" y="6" width="19" height="12" rx="2.5" />
          <path d="M6.5 10h.01M10 10h.01M13.5 10h.01M17 10h.01M7 14h10" />
        </svg>
      );
    case "cpu":
      return (
        <svg {...p}>
          <rect x="6" y="6" width="12" height="12" rx="2" />
          <path d="M9.5 9.5h5v5h-5zM9 2.5V6M15 2.5V6M9 18v3.5M15 18v3.5M2.5 9H6M2.5 15H6M18 9h3.5M18 15h3.5" />
        </svg>
      );
    case "cloud":
      return (
        <svg {...p}>
          <path d="M7 18h10.5a4 4 0 0 0 .6-7.95A6 6 0 0 0 6.4 9.2 4.5 4.5 0 0 0 7 18z" />
        </svg>
      );
    case "home":
      return (
        <svg {...p}>
          <path d="M4 10.5L12 4l8 6.5V20a1 1 0 0 1-1 1h-4.5v-6h-5v6H5a1 1 0 0 1-1-1z" />
        </svg>
      );
    case "history":
      return (
        <svg {...p}>
          <path d="M3.5 12a8.5 8.5 0 1 0 2.5-6M3.5 4v4h4" />
          <path d="M12 8v4.5l3 2" />
        </svg>
      );
    case "book":
      return (
        <svg {...p}>
          <path d="M5 4.5A1.5 1.5 0 0 1 6.5 3H19v15H6.5A1.5 1.5 0 0 0 5 19.5z" />
          <path d="M5 19.5A1.5 1.5 0 0 0 6.5 21H19M9 7.5h6" />
        </svg>
      );
    case "gear":
      return (
        <svg {...p}>
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
        </svg>
      );
    case "check":
      return (
        <svg {...p} strokeWidth={2.4}>
          <path d="M5 12.5l4.5 4.5L19 7.5" />
        </svg>
      );
    case "x":
      return (
        <svg {...p} strokeWidth={2.2}>
          <path d="M6 6l12 12M18 6L6 18" />
        </svg>
      );
    case "lock":
      return (
        <svg {...p}>
          <rect x="5" y="11" width="14" height="10" rx="2" />
          <path d="M8 11V7.5a4 4 0 0 1 8 0V11" />
        </svg>
      );
    case "copy":
      return (
        <svg {...p}>
          <rect x="8.5" y="8.5" width="12" height="12" rx="2" />
          <path d="M15.5 8.5V5a1.5 1.5 0 0 0-1.5-1.5H5A1.5 1.5 0 0 0 3.5 5v9A1.5 1.5 0 0 0 5 15.5h3.5" />
        </svg>
      );
    case "trash":
      return (
        <svg {...p}>
          <path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3" />
        </svg>
      );
    case "download":
      return (
        <svg {...p}>
          <path d="M12 4v11M7.5 10.5L12 15l4.5-4.5M5 20h14" />
        </svg>
      );
    case "sparkle":
      return (
        <svg {...p}>
          <path d="M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z" />
          <path d="M19 16l.7 1.8 1.8.7-1.8.7L19 21l-.7-1.8-1.8-.7 1.8-.7z" />
        </svg>
      );
    case "arrow-right":
      return (
        <svg {...p}>
          <path d="M5 12h14M13 6l6 6-6 6" />
        </svg>
      );
    default:
      return null;
  }
}

export function Toggle({ checked, onChange, disabled }: { checked: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      className={`toggle ${checked ? "on" : ""}`}
      onClick={() => onChange(!checked)}
    >
      <span className="knob" />
    </button>
  );
}

export function Row({ title, desc, children }: { title: ReactNode; desc?: ReactNode; children?: ReactNode }) {
  return (
    <div className="row">
      <div className="row-text">
        <div className="row-title">{title}</div>
        {desc && <div className="row-desc">{desc}</div>}
      </div>
      <div className="row-control">{children}</div>
    </div>
  );
}

export function Section({ title, children, footer }: { title?: string; children: ReactNode; footer?: ReactNode }) {
  return (
    <section className="section">
      {title && <h3 className="section-title">{title}</h3>}
      <div className="card">{children}</div>
      {footer && <div className="section-footer">{footer}</div>}
    </section>
  );
}

export function Badge({ tone = "neutral", children }: { tone?: "ok" | "warn" | "err" | "neutral" | "accent"; children: ReactNode }) {
  return <span className={`badge ${tone}`}>{children}</span>;
}

export function StatusPill({ ok, okText, badText }: { ok: boolean; okText: string; badText: string }) {
  return (
    <span className={`status-pill ${ok ? "ok" : "bad"}`}>
      <span className="dot" />
      {ok ? okText : badText}
    </span>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="kbd">{children}</kbd>;
}

export function Progress({ value }: { value: number }) {
  return (
    <div className="progress">
      <div className="progress-bar" style={{ width: `${Math.max(0, Math.min(100, value * 100))}%` }} />
    </div>
  );
}
