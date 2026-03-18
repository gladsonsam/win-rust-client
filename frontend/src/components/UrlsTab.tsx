import { useEffect, useState } from "react";
import { ExternalLink, Copy, Check } from "lucide-react";
import { DataTable, type Column } from "./DataTable";
import { api } from "../lib/api";
import { fmtTime, copyToClipboard } from "../lib/utils";

type Row = Record<string, unknown>;

interface Props {
  agentId: string;
  refreshKey: number;
}

/** Ensure a URL has a protocol so the browser doesn't treat it as a relative path. */
function withProtocol(url: string): string {
  if (/^https?:\/\//i.test(url)) return url;
  return `https://${url}`;
}

function UrlCell({ url }: { url: string }) {
  const [copied, setCopied] = useState(false);
  const href = withProtocol(url);
  const handleCopy = async () => {
    if (await copyToClipboard(url)) {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    }
  };
  return (
    <div className="group flex items-center gap-1.5 max-w-lg">
      <span className="truncate text-accent text-xs">{url}</span>
      <a
        href={href}
        target="_blank"
        rel="noreferrer"
        className="opacity-0 group-hover:opacity-100 transition-opacity
                   text-muted hover:text-primary flex-shrink-0"
        title="Open URL"
      >
        <ExternalLink size={11} />
      </a>
      <button
        onClick={handleCopy}
        className="opacity-0 group-hover:opacity-100 transition-opacity
                   text-muted hover:text-primary flex-shrink-0"
        title="Copy URL"
      >
        {copied ? <Check size={11} className="text-ok" /> : <Copy size={11} />}
      </button>
    </div>
  );
}

const COLUMNS: Column[] = [
  {
    key: "url",
    label: "URL",
    render: (v) => <UrlCell url={String(v ?? "")} />,
  },
  { key: "browser", label: "Browser", className: "w-28 whitespace-nowrap" },
  {
    key: "ts",
    label: "Time",
    className: "w-24 whitespace-nowrap",
    render: (v) => fmtTime(String(v)),
  },
];

export function UrlsTab({ agentId, refreshKey }: Props) {
  const [rows, setRows] = useState<Row[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading((prev) => (rows.length === 0 ? true : prev));
    api
      .urls(agentId, { limit: 200 })
      .then((r) => setRows(r.rows as unknown as Row[]))
      .catch(console.error)
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentId, refreshKey]);

  return (
    <DataTable
      data={rows}
      columns={COLUMNS}
      searchPlaceholder="Search by URL or browser…"
      isLoading={loading}
      emptyMessage="No URL visits recorded yet"
    />
  );
}
