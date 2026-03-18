import { useEffect, useState } from "react";
import { DataTable, type Column } from "./DataTable";
import { api } from "../lib/api";
import { fmtTime } from "../lib/utils";

type Row = Record<string, unknown>;

interface Props {
  agentId: string;
  refreshKey: number;
}

const COLUMNS: Column[] = [
  { key: "title", label: "Window Title" },
  { key: "app", label: "App", className: "w-36 whitespace-nowrap" },
  {
    key: "ts",
    label: "Time",
    className: "w-24 whitespace-nowrap",
    render: (v) => fmtTime(String(v)),
  },
];

export function WindowsTab({ agentId, refreshKey }: Props) {
  const [rows, setRows] = useState<Row[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading((prev) => (rows.length === 0 ? true : prev));
    api
      .windows(agentId, { limit: 200 })
      .then((r) => setRows(r.rows as unknown as Row[]))
      .catch(console.error)
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentId, refreshKey]);

  return (
    <DataTable
      data={rows}
      columns={COLUMNS}
      searchPlaceholder="Search by title or app…"
      isLoading={loading}
      emptyMessage="No window focus events recorded yet"
    />
  );
}
