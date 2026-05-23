import { Fragment, useState, useMemo } from 'react'

function formatTimestamp(ts) {
  if (!ts) return '--'
  try {
    const d = new Date(ts)
    return d.toLocaleString('en-US', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
    })
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

export default function RawDataView({ articles, loading, onRefresh }) {
  const [expandedId, setExpandedId] = useState(null)
  const [langFilter, setLangFilter] = useState('all')
  const [searchQuery, setSearchQuery] = useState('')

  const data = articles || []

  const filtered = useMemo(() => {
    return data.filter((item) => {
      if (langFilter !== 'all' && item.raw_language !== langFilter) return false
      if (searchQuery) {
        const q = searchQuery.toLowerCase()
        const title = (item.title || '').toLowerCase()
        const content = (item.content || '').toLowerCase()
        if (!title.includes(q) && !content.includes(q)) return false
      }
      return true
    })
  }, [data, langFilter, searchQuery])

  const toggleExpand = (id) => {
    setExpandedId(expandedId === id ? null : id)
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

        <span className="toolbar-count">
          Showing {filtered.length} of {data.length} articles
        </span>
      </div>

      {filtered.length === 0 ? (
        <div className="empty-state">
          {data.length === 0
            ? "No raw data available. Click 'Trigger Scan' to start harvesting."
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
                    <span className="text-mono text-sm text-secondary">
                      {formatTimestamp(item.created_at)}
                    </span>
                  </td>
                </tr>
                {expandedId === item.id && (
                  <tr>
                    <td colSpan={6} style={{ padding: 0 }}>
                      <div className="expanded-content">
                        <div className="expanded-meta">
                          <span>ID: <span className="text-mono">{item.id}</span></span>
                          <span>Market: {item.market || '--'}</span>
                          <span>Language: {item.raw_language || '--'}</span>
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
