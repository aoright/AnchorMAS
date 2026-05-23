import { Fragment, useState, useMemo } from 'react'

function formatTimestamp(ts) {
  if (!ts) return '--'
  if (ts.length >= 10 && /^\d{4}-\d{2}-\d{2}/.test(ts)) {
    return ts.substring(0, 10)
  }
  try {
    const d = new Date(ts)
    if (isNaN(d.getTime())) return ts
    const year = d.getFullYear()
    const month = String(d.getMonth() + 1).padStart(2, '0')
    const day = String(d.getDate()).padStart(2, '0')
    return `${year}-${month}-${day}`
  } catch {
    return ts
  }
}

function truncate(text, maxLen) {
  if (!text) return ''
  if (text.length <= maxLen) return text
  return text.slice(0, maxLen) + '...'
}

const LANGUAGE_OPTIONS = [
  { value: 'all', label: 'All Languages' },
  { value: 'en', label: 'English' },
  { value: 'zh', label: 'Chinese' },
  { value: 'ja', label: 'Japanese' },
  { value: 'ko', label: 'Korean' },
]

const STATUS_OPTIONS = [
  { value: 'all', label: 'All Statuses' },
  { value: 'unprocessed', label: 'Unprocessed' },
  { value: 'processed', label: 'Processed' },
  { value: 'invalid', label: 'Invalid' },
]

export default function RawDataView({ articles, loading, onRefresh }) {
  const [expandedId, setExpandedId] = useState(null)
  const [langFilter, setLangFilter] = useState('all')
  const [statusFilter, setStatusFilter] = useState('all')
  const [searchQuery, setSearchQuery] = useState('')
  const [searchHours, setSearchHours] = useState('24')
  const [searchStatus, setSearchStatus] = useState('idle')

  const data = articles || []

  const filtered = useMemo(() => {
    return data.filter((item) => {
      if (langFilter !== 'all' && item.raw_language !== langFilter) return false
      if (statusFilter !== 'all') {
        const itemStatus = item.pipeline_status || 'unprocessed';
        if (statusFilter === 'unprocessed' && itemStatus !== 'unprocessed') return false
        if (statusFilter === 'processed' && itemStatus !== 'processed') return false
        if (statusFilter === 'invalid' && itemStatus !== 'invalid') return false
      }
      if (searchQuery) {
        const q = searchQuery.toLowerCase()
        const title = (item.title || '').toLowerCase()
        const content = (item.content || '').toLowerCase()
        if (!title.includes(q) && !content.includes(q)) return false
      }
      return true
    })
  }, [data, langFilter, statusFilter, searchQuery])

  const toggleExpand = (id) => {
    setExpandedId(expandedId === id ? null : id)
  }

  const handleTriggerSearch = async () => {
    setSearchStatus('searching')
    try {
      const res = await fetch(`/api/scan?hours=${searchHours}`, { method: 'POST' })
      if (res.ok || res.status === 202) {
        setSearchStatus('triggered')
        alert('新闻搜索/扫描已在后台启动！您可以切换到 Pipeline 标签页查看实时 analysis 进度。')
        setTimeout(() => setSearchStatus('idle'), 5000)
      } else {
        setSearchStatus('error')
        alert('触发新闻搜索失败')
        setTimeout(() => setSearchStatus('idle'), 3000)
      }
    } catch (err) {
      setSearchStatus('error')
      alert(`连接后端失败: ${err.message}`)
      setTimeout(() => setSearchStatus('idle'), 3000)
    }
  }

  if (loading) {
    return <div className="empty-state">Loading raw data...</div>
  }

  return (
    <div>
      <div className="toolbar">
        <select
          className="input"
          value={langFilter}
          onChange={(e) => setLangFilter(e.target.value)}
        >
          {LANGUAGE_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>

        <select
          className="input"
          value={statusFilter}
          onChange={(e) => setStatusFilter(e.target.value)}
        >
          {STATUS_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>

        <input
          className="input input--wide"
          type="text"
          placeholder="Search by title or content..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
        />

        <button className="btn btn--sm" onClick={onRefresh}>
          Refresh
        </button>

        <span style={{ margin: '0 8px', color: 'var(--color-border)' }}>|</span>

        <select
          className="input"
          value={searchHours}
          onChange={(e) => setSearchHours(e.target.value)}
          style={{ width: 140 }}
        >
          <option value="24">最近 24 小时</option>
          <option value="48">最近 48 小时</option>
          <option value="72">最近 72 小时</option>
          <option value="168">最近 7 天</option>
          <option value="720">最近 30 天</option>
        </select>

        <button
          className="btn btn--primary btn--sm"
          onClick={handleTriggerSearch}
          disabled={searchStatus === 'searching'}
        >
          {searchStatus === 'searching' ? 'Searching...' : '搜集最新新闻'}
        </button>

        <span className="toolbar-count">
          Showing {filtered.length} of {data.length} articles
        </span>
      </div>

      {filtered.length === 0 ? (
        <div className="empty-state">
          {data.length === 0
            ? "No raw data available. Open Pipeline and run a scan to start harvesting."
            : 'No articles match the current filters.'}
        </div>
      ) : (
        <table className="data-table">
          <thead>
            <tr>
              <th>Source</th>
              <th>Lang</th>
              <th>Title</th>
              <th>Content</th>
              <th>Chars</th>
              <th>Status</th>
              <th>Timestamp</th>
            </tr>
          </thead>
          <tbody>
            {filtered.map((item) => (
              <Fragment key={item.id}>
                <tr
                  className={`expandable ${expandedId === item.id ? 'expanded-row' : ''}`}
                  onClick={() => toggleExpand(item.id)}
                >
                  <td>
                    <span className="text-mono text-sm">
                      {item.market || '--'}
                    </span>
                  </td>
                  <td>
                    <span className="text-mono text-sm">
                      {item.raw_language || '--'}
                    </span>
                  </td>
                  <td>{item.title || '--'}</td>
                  <td>
                    <span className="text-truncate" style={{ display: 'block' }}>
                      {truncate(item.content, 80)}
                    </span>
                  </td>
                  <td>
                    <span className="text-mono text-sm">
                      {(item.content || '').length}
                    </span>
                  </td>
                  <td>
                    {item.pipeline_status === 'invalid' ? (
                      <span className="badge badge-status-bankrupt">Invalid</span>
                    ) : item.pipeline_status === 'processed' ? (
                      <span className="badge badge-status-active">Processed</span>
                    ) : (
                      <span className="badge badge-neutral">Unprocessed</span>
                    )}
                  </td>
                  <td>
                    <span className="text-mono text-sm text-secondary">
                      {formatTimestamp(item.created_at)}
                    </span>
                  </td>
                </tr>
                {expandedId === item.id && (
                  <tr>
                    <td colSpan={7} style={{ padding: 0 }}>
                      <div className="expanded-content">
                        <div className="expanded-meta">
                          <span>ID: <span className="text-mono">{item.id}</span></span>
                          <span>Market: {item.market || '--'}</span>
                          <span>Language: {item.raw_language || '--'}</span>
                          <span>
                            Status: <span className="text-mono">{item.pipeline_status || 'unprocessed'}</span>
                          </span>
                          <span>
                            Source:{' '}
                            <a
                              href={item.source_url}
                              target="_blank"
                              rel="noopener noreferrer"
                              onClick={(e) => e.stopPropagation()}
                            >
                              {item.source_url}
                            </a>
                          </span>
                        </div>
                        <pre>{item.content}</pre>
                      </div>
                    </td>
                  </tr>
                )}
              </Fragment>
            ))}
          </tbody>
        </table>
      )}
    </div>
  )
}
