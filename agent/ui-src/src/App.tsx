/**
 * Sentinel Agent – Settings Webview
 *
 * Communicates with the Rust backend through Tauri's IPC `invoke()` API.
 *
 * Commands exposed by Rust:
 *   get_config()  -> Config
 *   save_config(config: Config) -> void
 *   get_status()  -> { status: "Connected"|"Connecting"|"Disconnected"|"Error", message?: string }
 *   exit_agent()  -> never
 *   hide_window() -> void
 */

import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { LogicalSize } from "@tauri-apps/api/dpi";
import {
  Shield,
  Save,
  X,
  Power,
  Lock,
  Eye,
  EyeOff,
  ChevronDown,
  ChevronUp,
  Wifi,
  WifiOff,
  Loader2,
  Settings,
  AlertTriangle,
} from "lucide-react";

// ── Types ─────────────────────────────────────────────────────────────────────

interface AgentConfig {
  server_url: string;
  agent_name: string;
  agent_password: string;
  ui_password_hash: string;
}

type ConnectionStatus = "Connected" | "Connecting" | "Disconnected" | "Error";

interface StatusResponse {
  status: ConnectionStatus;
  message?: string;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function StatusBadge({ status, message }: { status: ConnectionStatus; message?: string }) {
  const configs: Record<ConnectionStatus, { color: string; icon: React.ReactNode; label: string }> = {
    Connected: {
      color: "text-ok",
      icon: <Wifi size={12} />,
      label: "Connected",
    },
    Connecting: {
      color: "text-warn",
      icon: <Loader2 size={12} className="animate-spin" />,
      label: "Connecting…",
    },
    Disconnected: {
      color: "text-muted",
      icon: <WifiOff size={12} />,
      label: "Disconnected",
    },
    Error: {
      color: "text-danger",
      icon: <AlertTriangle size={12} />,
      label: message ? `Error: ${message}` : "Error",
    },
  };

  const cfg = configs[status];

  return (
    <div className={`flex items-center gap-1.5 text-xs font-medium ${cfg.color}`}>
      {cfg.icon}
      <span className="truncate max-w-[180px]">{cfg.label}</span>
    </div>
  );
}

function PasswordInput({
  id,
  value,
  onChange,
  placeholder,
  disabled,
}: {
  id: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  disabled?: boolean;
}) {
  const [show, setShow] = useState(false);
  return (
    <div className="relative">
      <input
        id={id}
        type={show ? "text" : "password"}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        disabled={disabled}
        style={{ paddingRight: "2.5rem" }}
      />
      <button
        type="button"
        onClick={() => setShow((s) => !s)}
        className="absolute right-2 top-1/2 -translate-y-1/2 text-muted hover:text-primary transition-colors"
      >
        {show ? <EyeOff size={14} /> : <Eye size={14} />}
      </button>
    </div>
  );
}

function Label({ htmlFor, children }: { htmlFor?: string; children: React.ReactNode }) {
  return (
    <label htmlFor={htmlFor} className="text-sm font-medium text-primary block mb-1.5">
      {children}
    </label>
  );
}

function FormGroup({ children }: { children: React.ReactNode }) {
  return <div className="flex flex-col gap-1 mb-4">{children}</div>;
}

// ── Password Gate ─────────────────────────────────────────────────────────────

function PasswordGate({ onUnlock }: { onUnlock: () => void }) {
  const [pw, setPw] = useState("");
  const [error, setError] = useState(false);
  const [checking, setChecking] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!pw) return;
    setChecking(true);
    try {
      await invoke("verify_ui_password", { password: pw });
      setError(false);
      onUnlock();
    } catch {
      setError(true);
      setPw("");
    } finally {
      setChecking(false);
    }
  };

  return (
    <div className="flex h-[380px] items-center justify-center bg-bg animate-fade-in">
      <div
        className="w-[340px] rounded-2xl border border-border bg-surface shadow-2xl p-8"
        style={{ boxShadow: "0 25px 60px rgba(0,0,0,0.6)" }}
      >
        {/* Icon */}
        <div className="flex justify-center mb-6">
          <div className="w-12 h-12 rounded-full bg-accent/10 flex items-center justify-center">
            <Lock size={22} className="text-accent" />
          </div>
        </div>

        <h1 className="text-lg font-bold text-center text-primary mb-1">Agent Settings</h1>
        <p className="text-xs text-muted text-center mb-6">Enter UI access password to continue</p>

        <form onSubmit={handleSubmit} className="flex flex-col gap-3">
          <PasswordInput
            id="gate-password"
            value={pw}
            onChange={setPw}
            placeholder="Password"
          />

          {error && (
            <p className="text-xs text-danger flex items-center gap-1.5">
              <AlertTriangle size={12} />
              Wrong password — try again
            </p>
          )}

          <button
            type="submit"
            disabled={checking || !pw}
            className="mt-1 w-full py-2 rounded-lg bg-accent text-white text-sm font-semibold
                       hover:bg-accent/90 disabled:opacity-50 disabled:cursor-not-allowed
                       transition-colors"
          >
            {checking ? "Checking…" : "Unlock"}
          </button>
        </form>
      </div>
    </div>
  );
}

// ── Settings Panel ────────────────────────────────────────────────────────────

function SettingsPanel() {
  const [config, setConfig] = useState<AgentConfig>({
    server_url: "",
    agent_name: "",
    agent_password: "",
    ui_password_hash: "",
  });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saveMsg, setSaveMsg] = useState<{ text: string; ok: boolean } | null>(null);

  const [status, setStatus] = useState<StatusResponse>({ status: "Disconnected" });
  const [pwOpen, setPwOpen] = useState(false);
  const [newPw, setNewPw] = useState("");
  const [confirmPw, setConfirmPw] = useState("");

  const saveMsgTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // ── Load config on mount ────────────────────────────────────────────────────
  useEffect(() => {
    invoke<AgentConfig>("get_config").then((cfg) => {
      setConfig(cfg);
      setLoading(false);
    });
  }, []);

  // ── Poll status every 2 s ───────────────────────────────────────────────────
  useEffect(() => {
    const poll = async () => {
      try {
        const s = await invoke<StatusResponse>("get_status");
        setStatus(s);
      } catch {
        setStatus({ status: "Error", message: "IPC unavailable" });
      }
    };
    poll();
    const id = setInterval(poll, 2000);
    return () => clearInterval(id);
  }, []);

  // ── Save ────────────────────────────────────────────────────────────────────
  const handleSave = useCallback(async () => {
    if (newPw && newPw !== confirmPw) {
      setSaveMsg({ text: "Passwords don't match", ok: false });
      return;
    }
    setSaving(true);

    try {
      const payload: AgentConfig & { new_password?: string } = {
        ...config,
        ...(newPw ? { new_password: newPw } : {}),
      };
      await invoke("save_config", { config: payload });
      setSaveMsg({ text: "Settings saved ✓", ok: true });
      setNewPw("");
      setConfirmPw("");
      // Reload config so ui_password_hash is up-to-date
      const fresh = await invoke<AgentConfig>("get_config");
      setConfig(fresh);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      setSaveMsg({ text: `Save failed: ${msg}`, ok: false });
    } finally {
      setSaving(false);
      if (saveMsgTimer.current) clearTimeout(saveMsgTimer.current);
      saveMsgTimer.current = setTimeout(() => setSaveMsg(null), 4000);
    }
  }, [config, newPw, confirmPw]);

  const handleClose = useCallback(() => {
    invoke("hide_window").catch(() => {});
  }, []);

  const handleExit = useCallback(() => {
    invoke("exit_agent").catch(() => {});
  }, []);

  if (loading) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-bg">
        <Loader2 size={28} className="animate-spin text-muted" />
      </div>
    );
  }

  return (
    <div className="bg-bg flex flex-col animate-fade-in">
      {/* ── Header ── */}
      <header className="flex items-center justify-between px-5 h-12 bg-surface border-b border-border flex-shrink-0">
        <div className="flex items-center gap-2">
          <Shield size={16} className="text-accent" />
          <span className="text-[15px] font-semibold tracking-wide text-primary">
            Sentinel Agent
          </span>
        </div>
        <StatusBadge status={status.status} message={status.message} />
      </header>

      {/* ── Body ── */}
      <main className="flex-1 p-6" style={{ overflow: "hidden" }}>

        {/* ── Connection section ── */}
        <section className="mb-6">
          <div className="flex items-center gap-2 mb-3">
            <Settings size={14} className="text-muted" />
            <h2 className="text-xs uppercase tracking-widest font-bold text-muted">
              Connection
            </h2>
          </div>

          <div
            className="rounded-xl border border-border bg-surface p-5"
            style={{ boxShadow: "0 4px 16px rgba(0,0,0,0.4)" }}
          >
            <FormGroup>
              <Label htmlFor="server-url">Server URL</Label>
              <input
                id="server-url"
                type="text"
                value={config.server_url}
                onChange={(e) => setConfig((c) => ({ ...c, server_url: e.target.value }))}
                placeholder="wss://host:port/ws/agent"
              />
            </FormGroup>

            <FormGroup>
              <Label htmlFor="agent-name">Agent Name</Label>
              <input
                id="agent-name"
                type="text"
                value={config.agent_name}
                onChange={(e) => setConfig((c) => ({ ...c, agent_name: e.target.value }))}
                placeholder="My-PC"
              />
            </FormGroup>

            <FormGroup>
              <Label htmlFor="agent-password">Agent Password</Label>
              <PasswordInput
                id="agent-password"
                value={config.agent_password}
                onChange={(v) => setConfig((c) => ({ ...c, agent_password: v }))}
                placeholder="Server auth secret"
              />
            </FormGroup>
            
            <p className="text-xs text-muted mt-4">
              TLS is enforced: the agent must connect using <code>wss://</code>.
            </p>
          </div>
        </section>

        {/* ── UI Password section (collapsible) ── */}
        <section className="mb-6">
          <button
            onClick={() => setPwOpen((o) => !o)}
            className="flex items-center gap-2 mb-3 w-full text-left"
          >
            <Lock size={14} className="text-muted" />
            <h2 className="text-xs uppercase tracking-widest font-bold text-muted flex-1">
              UI Access Password
            </h2>
            {pwOpen ? (
              <ChevronUp size={14} className="text-muted" />
            ) : (
              <ChevronDown size={14} className="text-muted" />
            )}
          </button>

          {pwOpen && (
            <div
              className="rounded-xl border border-border bg-surface p-5 animate-fade-in"
              style={{ boxShadow: "0 4px 16px rgba(0,0,0,0.4)" }}
            >
              <p className="text-xs text-muted mb-4">
                Leave blank to keep the current password. Set a password to require it when
                reopening the settings window via Ctrl+Shift+F12.
              </p>

              <FormGroup>
                <Label htmlFor="new-password">New Password</Label>
                <PasswordInput
                  id="new-password"
                  value={newPw}
                  onChange={setNewPw}
                  placeholder="New password"
                />
              </FormGroup>

              <FormGroup>
                <Label htmlFor="confirm-password">Confirm Password</Label>
                <PasswordInput
                  id="confirm-password"
                  value={confirmPw}
                  onChange={setConfirmPw}
                  placeholder="Confirm password"
                />
              </FormGroup>
            </div>
          )}
        </section>

        {/* ── Action buttons ── */}
        <div className="flex items-center gap-2">
          <button
            onClick={handleSave}
            disabled={saving}
            className="flex items-center gap-2 px-4 py-2 rounded-lg bg-accent text-white
                       text-sm font-semibold hover:bg-accent/90 disabled:opacity-50
                       disabled:cursor-not-allowed transition-colors"
          >
            {saving ? (
              <Loader2 size={14} className="animate-spin" />
            ) : (
              <Save size={14} />
            )}
            Save
          </button>

          <button
            onClick={handleClose}
            className="flex items-center gap-2 px-4 py-2 rounded-lg border border-border
                       bg-surface text-sm font-medium text-primary hover:bg-border/40
                       transition-colors"
          >
            <X size={14} />
            Close
          </button>

          <div className="flex-1" />

          <button
            onClick={handleExit}
            className="flex items-center gap-2 px-4 py-2 rounded-lg border border-danger/30
                       bg-danger/10 text-danger text-sm font-medium hover:bg-danger/20
                       transition-colors"
          >
            <Power size={14} />
            Exit Agent
          </button>
        </div>

        {/* ── Save message ── */}
        {saveMsg && (
          <p
            className={`mt-3 text-sm flex items-center gap-1.5 animate-fade-in
                        ${saveMsg.ok ? "text-ok" : "text-danger"}`}
          >
            {!saveMsg.ok && <AlertTriangle size={13} />}
            {saveMsg.text}
          </p>
        )}

        {/* ── Hotkey hint ── */}
        <p className="mt-5 text-[11px] text-muted">
          Reopen anytime: <kbd className="font-mono bg-surface border border-border rounded px-1">Ctrl+Shift+F12</kbd>
        </p>
      </main>
    </div>
  );
}

// ── Root App ──────────────────────────────────────────────────────────────────

type AppScreen = "loading" | "password" | "settings";

export default function App() {
  const [screen, setScreen] = useState<AppScreen>("loading");

  const checkLock = useCallback(() => {
    invoke<boolean>("has_ui_password")
      .then((has) => setScreen(has ? "password" : "settings"))
      .catch(() => setScreen("settings"));
  }, []);

  const forceRelock = useCallback(() => {
    // Immediately hide the settings UI (no flash), then decide whether a password gate is needed.
    setScreen("password");
    checkLock();
  }, [checkLock]);

  useEffect(() => {
    // Dynamic native auto-resizing
    const ob = new ResizeObserver(() => {
      const height = document.documentElement.scrollHeight;
      const win = getCurrentWebviewWindow();
      win.setSize(new LogicalSize(520, height));
    });
    ob.observe(document.body);
    return () => ob.disconnect();
  }, []);

  useEffect(() => {
    checkLock();

    const unlistenLock = listen("lock_ui", () => {
      forceRelock();
    });

    // Re-lock whenever the window gains focus (covers hotkey show and any other way
    // the window is brought back into view).
    const unlistenFocus = getCurrentWebviewWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) forceRelock();
    });

    // Fallbacks: some platforms / versions are more reliable with DOM events.
    const onFocus = () => forceRelock();
    window.addEventListener("focus", onFocus);

    const onVisibility = () => {
      if (!document.hidden) forceRelock();
    };
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      unlistenLock.then((unlisten: () => void) => unlisten());
      unlistenFocus.then((unlisten: () => void) => unlisten());
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [checkLock, forceRelock]);

  if (screen === "loading") {
    return (
      <div className="flex h-[200px] items-center justify-center bg-bg">
        <Loader2 size={28} className="animate-spin text-accent" />
      </div>
    );
  }

  if (screen === "password") {
    return <PasswordGate onUnlock={() => setScreen("settings")} />;
  }

  return <SettingsPanel />;
}
