import { useEffect, useMemo, useState } from "react";
import { Copy, Check } from "lucide-react";
import { DataTable, type Column } from "./DataTable";
import { api } from "../lib/api";
import { fmtTime, copyToClipboard } from "../lib/utils";

type Row = Record<string, unknown>;

interface Props {
  agentId: string;
  refreshKey: number;
}

function applyKeyCorrections(raw: string): string {
  const out: string[] = [];
  const parts = raw.split(/(\[⌫\]|\[Del\]|\[⇥\])/g);
  for (const p of parts) {
    if (!p) continue;
    if (p === "[⌫]") {
      if (out.length > 0) out.pop();
      continue;
    }
    if (p === "[Del]") continue;
    if (p === "[⇥]") {
      out.push("\t");
      continue;
    }
    for (const ch of p) out.push(ch);
  }
  return out.join("");
}

function CopyableText({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const handle = async () => {
    if (await copyToClipboard(text)) {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    }
  };
  return (
    <div className="group flex items-start gap-1.5 max-w-md">
      <code
        className="text-xs font-mono bg-bg px-1.5 py-0.5 rounded
                       break-all leading-relaxed flex-1 max-h-20 overflow-y-auto"
      >
        {text}
      </code>
      <button
        onClick={handle}
        className="opacity-0 group-hover:opacity-100 transition-opacity
                   p-1 text-muted hover:text-primary flex-shrink-0"
        title="Copy"
      >
        {copied ? <Check size={12} className="text-ok" /> : <Copy size={12} />}
      </button>
    </div>
  );
}

const COLUMNS: Column[] = [
  { key: "app", label: "App", className: "w-36 whitespace-nowrap" },
  { key: "window_title", label: "Window", className: "max-w-[200px]" },
  { key: "text", label: "Keystrokes", sortable: false },
  {
    key: "updated_at",
    label: "Time",
    className: "w-24 whitespace-nowrap",
    render: (v) => fmtTime(String(v)),
  },
];

export function KeysTab({ agentId, refreshKey }: Props) {
  const [rows, setRows] = useState<Row[]>([]);
  const [loading, setLoading] = useState(true);
  const [corrected, setCorrected] = useState(true);

  useEffect(() => {
    // Only show the spinner on the very first load; silently refresh afterwards.
    setLoading((prev) => (rows.length === 0 ? true : prev));
    api
      .keys(agentId, { limit: 200 })
      .then((r) => setRows(r.rows as unknown as Row[]))
      .catch(console.error)
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentId, refreshKey]);

  const columns = useMemo<Column[]>(() => {
    return COLUMNS.map((c) => {
      if (c.key !== "text") return c;
      return {
        ...c,
        render: (v) => {
          const raw = String(v ?? "");
          const shown = corrected ? applyKeyCorrections(raw) : raw;
          return <CopyableText text={shown} />;
        },
      };
    });
  }, [corrected]);

  return (
    <DataTable
      data={rows}
      columns={columns}
      searchPlaceholder="Search by app, window, or text…"
      isLoading={loading}
      emptyMessage="No keystroke sessions recorded yet"
      toolbarRight={
        <button
          onClick={() => setCorrected((v) => !v)}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium
                     border border-border bg-surface text-muted hover:text-primary transition-colors"
          title="Toggle whether backspaces are applied"
        >
          {corrected ? "Corrected" : "Raw"}
        </button>
      }
    />
  );
}
