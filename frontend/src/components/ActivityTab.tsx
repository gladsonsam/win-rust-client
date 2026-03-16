import { useEffect, useState } from 'react'
import { DataTable, type Column } from './DataTable'
import { api } from '../lib/api'
import { fmtTime, cn } from '../lib/utils'

type Row = Record<string, unknown>

interface Props { agentId: string; refreshKey: number }

const COLUMNS: Column[] = [
  {
    key:    'kind',
    label:  'Status',
    render: (v) => (
      <span className={cn(
        'inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold uppercase tracking-wide',
        v === 'active'
          ? 'bg-ok/15 text-ok'
          : 'bg-danger/15 text-danger',
      )}>
        <span className={cn(
          'w-1.5 h-1.5 rounded-full',
          v === 'active' ? 'bg-ok' : 'bg-danger',
        )} />
        {String(v)}
      </span>
    ),
  },
  {
    key:      'idle_secs',
    label:    'Idle',
    render:   (v) => v != null ? `${v}s` : '—',
    className: 'w-20 tabular-nums',
  },
  {
    key:      'ts',
    label:    'Time',
    className: 'w-24 whitespace-nowrap',
    render:   (v) => fmtTime(String(v)),
  },
]

export function ActivityTab({ agentId, refreshKey }: Props) {
  const [rows,    setRows]    = useState<Row[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    setLoading(prev => (rows.length === 0 ? true : prev))
    api.activity(agentId, { limit: 200 })
      .then(r => setRows(r.rows as unknown as Row[]))
      .catch(console.error)
      .finally(() => setLoading(false))
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentId, refreshKey])

  return (
    <DataTable
      data={rows}
      columns={COLUMNS}
      searchPlaceholder="Search by status…"
      isLoading={loading}
      emptyMessage="No activity events recorded yet"
    />
  )
}
