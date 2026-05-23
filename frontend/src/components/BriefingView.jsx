import { useState, useEffect, useCallback, useRef, Fragment } from 'react'

function severityClass(severity) {
  if (typeof severity === 'number') {
    if (severity >= 4) return 'label--high'
    if (severity >= 2) return 'label--medium'
    return 'label--low'
  }
  switch (severity) {
    case 'high': return 'label--high'
    case 'medium': return 'label--medium'
    case 'low': return 'label--low'
    default: return 'label--info'
  }
}

function statusDotClass(status) {
  if (!status) return 'status-dot--gray'
  const normalized = status.toLowerCase()
  switch (normalized) {
    case 'growth':
    case '关注':
      return 'status-dot--green'
    case 'stable':
    case '稳定':
      return 'status-dot--blue'
    case 'warning':
    case '警告':
      return 'status-dot--amber'
    case 'decline':
    case '紧急':
      return 'status-dot--red'
    default:
      return 'status-dot--gray'
  }
}

function statusText(status) {
  if (!status) return '[--]'
  const normalized = status.toLowerCase()
  switch (normalized) {
    case 'growth':
    case '关注':
      return '[GROWTH]'
    case 'stable':
    case '稳定':
      return '[STABLE]'
    case 'warning':
    case '警告':
      return '[WARN]'
    case 'decline':
    case '紧急':
      return '[DECLINE]'
    default:
      return `[${status.toUpperCase()}]`
  }
}

const MARKETS = [
  { key: 'Global', label: 'Global' },
  { key: 'China', label: 'China' },
  { key: 'Japan', label: 'Japan' },
  { key: 'Korea', label: 'Korea' },
  { key: 'SoutheastAsia', label: 'Southeast Asia' },
  { key: 'UnitedStates', label: 'United States' },
]

function getMarketData(overviewObj, key) {
  if (!overviewObj) return null
  if (overviewObj[key]) return overviewObj[key]
  
  // Normalize key lookup (lowercase, strip whitespace)
  const normalizedKey = key.toLowerCase().replace(/\s+/g, '')
  for (const [k, val] of Object.entries(overviewObj)) {
    if (k.toLowerCase().replace(/\s+/g, '') === normalizedKey) {
      return val
    }
  }
  return null
}

function formatSnapshotLabel(item) {
  const date = item.date || 'Undated'
  const createdAt = item.created_at || '--'
  const count = Number.isFinite(item.event_count) ? item.event_count : 0
  return `${date} ${createdAt} / ${count} events`
}

export default function BriefingView() {
  const [briefing, setBriefing] = useState(null)
  const [briefingHistory, setBriefingHistory] = useState([])
  const [selectedBriefingId, setSelectedBriefingId] = useState('latest')
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [chatMessages, setChatMessages] = useState([])
  const [chatInput, setChatInput] = useState('')
  const [chatLoading, setChatLoading] = useState(false)
  const [activeMarket, setActiveMarket] = useState('Global')
  const [bookmarks, setBookmarks] = useState([])
  const chatEndRef = useRef(null)
  const [expandedEvents, setExpandedEvents] = useState(new Set())
  const [reSynthesizing, setReSynthesizing] = useState(false)
  const [reSynthesizeMessage, setReSynthesizeMessage] = useState('')

  const handleReSynthesize = async () => {
    setReSynthesizing(true)
    setReSynthesizeMessage('Starting briefing re-synthesis...')
    try {
      const res = await fetch('/api/scan?synthesize_only=true', { method: 'POST' })
      if (!res.ok && res.status !== 202) {
        throw new Error(`Server returned ${res.status}`)
      }
      
      let attempts = 0
      const interval = setInterval(async () => {
        attempts++
        if (attempts > 300) {
          clearInterval(interval)
          setReSynthesizing(false)
          alert('重新生成简报超时（超过 5 分钟）')
          return
        }
        try {
          const statusRes = await fetch('/api/pipeline/status')
          if (statusRes.ok) {
            const statusData = await statusRes.json()
            if (statusData.status === 'completed') {
              clearInterval(interval)
              setReSynthesizing(false)
              // Refresh to load the newly synthesized briefing!
              const history = await fetchBriefingHistory()
              await fetchBriefing('latest')
            } else if (statusData.status === 'error') {
              clearInterval(interval)
              setReSynthesizing(false)
              alert(`重新生成简报失败: ${statusData.error_message || '未知错误'}`)
            } else {
              const msg = statusData.progress?.message || '正在合成战略简报...'
              setReSynthesizeMessage(msg)
            }
          }
        } catch (err) {
          console.error('Error polling pipeline status', err)
        }
      }, 1000)
    } catch (err) {
      setReSynthesizing(false)
      alert(`无法触发重新生成简报: ${err.message}`)
    }
  }

  const toggleExpandEvent = (eventId) => {
    setExpandedEvents((prev) => {
      const next = new Set(prev)
      if (next.has(eventId)) {
        next.delete(eventId)
      } else {
        next.add(eventId)
      }
      return next
    })
  }

  const fetchBookmarks = useCallback(async () => {
    try {
      const res = await fetch('/api/bookmarks')
      if (res.ok) {
        const data = await res.json()
        setBookmarks(data)
      }
    } catch (err) {
      console.error('Failed to fetch bookmarks', err)
    }
  }, [])

  const fetchBriefingHistory = useCallback(async () => {
    try {
      const res = await fetch('/api/briefings')
      if (!res.ok) {
        setBriefingHistory([])
        return []
      }
      const data = await res.json()
      const items = Array.isArray(data) ? data : data.items || []
      setBriefingHistory(items)
      return items
    } catch (err) {
      console.error('Failed to fetch briefing history', err)
      setBriefingHistory([])
      return []
    }
  }, [])

  const fetchBriefing = useCallback(async (briefingId = 'latest') => {
    setLoading(true)
    setError(null)
    try {
      const endpoint = briefingId && briefingId !== 'latest'
        ? `/api/briefings/${encodeURIComponent(briefingId)}`
        : '/api/briefing/latest'
      const res = await fetch(endpoint)
      if (res.status === 404) {
        setBriefing(null)
        setError('No briefings available. Run a scan first to generate a briefing.')
      } else if (res.ok) {
        const data = await res.json()
        setBriefing(data)
        setSelectedBriefingId(data.id || briefingId || 'latest')
        setActiveMarket('Global')
        setExpandedEvents(new Set())
        setChatMessages([])
        setChatInput('')
        await fetchBookmarks()
      } else {
        setBriefing(null)
        setError(`Failed to fetch briefing: ${res.status} ${res.statusText}`)
      }
    } catch (err) {
      setBriefing(null)
      setError(`Backend not connected: ${err.message}`)
    } finally {
      setLoading(false)
    }
  }, [fetchBookmarks])

  const handleBriefingChange = (event) => {
    const nextId = event.target.value
    setSelectedBriefingId(nextId)
    fetchBriefing(nextId)
  }

  const handleRefreshBriefings = async () => {
    const prevHistory = briefingHistory
    const nextHistory = await fetchBriefingHistory()
    const isViewingLatest = !briefing || prevHistory.length === 0 || selectedBriefingId === 'latest' || selectedBriefingId === prevHistory[0].id
    if (isViewingLatest) {
      await fetchBriefing('latest')
    } else {
      await fetchBriefing(selectedBriefingId || 'latest')
    }
  }

  const handleToggleBookmark = async (eventId) => {
    const existing = bookmarks.find((b) => b.event_id === eventId)
    if (existing) {
      try {
        const res = await fetch(`/api/bookmarks/${existing.id}`, { method: 'DELETE' })
        if (res.ok) {
          setBookmarks((prev) => prev.filter((b) => b.id !== existing.id))
        } else {
          alert('取消追踪失败')
        }
      } catch (err) {
        console.error(err)
        alert('无法连接到后端')
      }
    } else {
      try {
        const res = await fetch('/api/bookmarks', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ event_id: eventId }),
        })
        if (res.ok || res.status === 201) {
          const data = await res.json()
          setBookmarks((prev) => [data, ...prev])
        } else {
          alert('添加证据追踪失败')
        }
      } catch (err) {
        console.error(err)
        alert('无法连接到后端')
      }
    }
  }

  const handleScrollToEvent = (eventId, eventMarket) => {
    if (!eventId) return

    // Find the correct market tab key
    let targetMarket = 'Global'
    if (eventMarket) {
      const match = MARKETS.find(
        (m) => m.key.toLowerCase() === eventMarket.toLowerCase()
      )
      if (match) {
        targetMarket = match.key
      }
    }

    // Set the active market tab
    setActiveMarket(targetMarket)

    // Expand the event automatically so the user can see the full analysis
    setExpandedEvents((prev) => {
      const next = new Set(prev)
      next.add(eventId)
      return next
    })

    // Wait for the DOM to update, then scroll and flash-highlight
    setTimeout(() => {
      const element = document.getElementById(`event-row-${eventId}`)
      if (element) {
        element.scrollIntoView({ behavior: 'smooth', block: 'center' })
        
        // Add a temporary CSS class for the flash effect
        element.classList.add('event-highlight')
        
        // Remove the class after the animation completes so it can be re-triggered
        setTimeout(() => {
          element.classList.remove('event-highlight')
        }, 2000)
      }
    }, 150)
  }

  useEffect(() => {
    fetchBriefingHistory()
    fetchBriefing()
  }, [fetchBriefing, fetchBriefingHistory])

  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [chatMessages])

  const handleChatSend = async () => {
    const msg = chatInput.trim()
    if (!msg || chatLoading || !briefing) return

    const userMsg = { role: 'user', content: msg }
    setChatMessages((prev) => [...prev, userMsg])
    setChatInput('')
    setChatLoading(true)

    try {
      const res = await fetch('/api/chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          message: msg,
          briefing_id: briefing.id,
        }),
      })
      if (res.ok) {
        const data = await res.json()
        setChatMessages((prev) => [
          ...prev,
          { role: 'assistant', content: data.response || data.reply || JSON.stringify(data) },
        ])
      } else {
        setChatMessages((prev) => [
          ...prev,
          { role: 'system', content: `Error: ${res.status} ${res.statusText}` },
        ])
      }
    } catch (err) {
      setChatMessages((prev) => [
        ...prev,
        { role: 'system', content: `Connection error: ${err.message}` },
      ])
    } finally {
      setChatLoading(false)
    }
  }

  const handleChatKeyDown = (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleChatSend()
    }
  }

  if (loading) {
    return <div className="empty-state">Loading briefing...</div>
  }

  if (error || !briefing) {
    return (
      <div className="empty-state">
        {error || "No briefings available. Open Pipeline and run a scan to generate one."}
        <div style={{ marginTop: 12 }}>
          <button className="btn btn--sm" onClick={fetchBriefing}>Retry</button>
        </div>
      </div>
    )
  }

  // Parse heatmap: could be object {market: status} or array
  const heatmapEntries = (() => {
    if (!briefing.heatmap) return []
    if (Array.isArray(briefing.heatmap)) return briefing.heatmap
    if (typeof briefing.heatmap === 'object') {
      return Object.entries(briefing.heatmap).map(([market, val]) => {
        if (val && typeof val === 'object') {
          return {
            market,
            status: val.status || '--',
            notes: val.notes || '--',
          }
        }
        return {
          market,
          status: typeof val === 'string' ? val : '--',
          notes: '--',
        }
      })
    }
    return []
  })()

  // Parse recommendations: could be array of strings or JSON
  const recommendations = (() => {
    if (!briefing.recommendations) return []
    if (Array.isArray(briefing.recommendations)) return briefing.recommendations
    return []
  })()

  // Parse overview as JSON if possible
  const parsedOverview = (() => {
    if (!briefing.overview) return null
    try {
      const parsed = JSON.parse(briefing.overview)
      if (parsed && typeof parsed === 'object') {
        return parsed
      }
    } catch (e) {
      // Return null to fallback to string rendering
    }
    return null
  })()

  // Filter events by selected market tab
  const rawEvents = briefing.events || []
  const events = activeMarket === 'Global'
    ? rawEvents
    : rawEvents.filter(evt => {
        const evMarket = (evt.market || '').toLowerCase().replace(/\s+/g, '')
        const activeKey = activeMarket.toLowerCase()
        return evMarket === activeKey
      })
  const selectedSnapshot = briefingHistory.find((item) => item.id === selectedBriefingId)
  const savedSnapshotCount = briefingHistory.length

  return (
    <div>
      <div className="briefing-toolbar">
        <div className="briefing-selector">
          <label htmlFor="briefing-snapshot">Briefing Snapshot</label>
          <select
            id="briefing-snapshot"
            className="input briefing-select"
            value={selectedBriefingId}
            onChange={handleBriefingChange}
            disabled={briefingHistory.length === 0 || reSynthesizing}
          >
            {briefingHistory.length === 0 ? (
              <option value={selectedBriefingId}>Latest briefing</option>
            ) : (
              briefingHistory.map((item) => (
                <option key={item.id} value={item.id}>
                  {formatSnapshotLabel(item)}
                </option>
              ))
            )}
          </select>
        </div>
        <div className="briefing-meta">
          <span>Generated: {briefing.created_at || selectedSnapshot?.created_at || '--'}</span>
          <span>Events: {rawEvents.length}</span>
          <span>Saved snapshots: {savedSnapshotCount}</span>
        </div>
        <button className="btn btn--sm" onClick={handleRefreshBriefings} disabled={reSynthesizing}>
          Refresh
        </button>
        <button 
          className="btn btn--primary btn--sm" 
          onClick={handleReSynthesize} 
          disabled={reSynthesizing}
          style={{ marginLeft: 8 }}
        >
          {reSynthesizing ? 'Re-synthesizing...' : '重新生成简报'}
        </button>
      </div>

      {reSynthesizing && (
        <div className="info-box info-box--surface" style={{ marginBottom: 16, borderLeft: '4px solid var(--color-blue)', padding: '12px 16px' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <span className="status-dot status-dot--blue" />
            <div style={{ fontWeight: 600, fontSize: 13 }}>重新生成简报中:</div>
            <div style={{ fontSize: 13, color: 'var(--color-text-secondary)' }}>{reSynthesizeMessage}</div>
          </div>
        </div>
      )}

      {/* Market Selector Tabs */}
      <div className="tab-nav" style={{ marginBottom: 20 }}>
        {MARKETS.map((m) => (
          <button
            key={m.key}
            className={activeMarket === m.key ? 'active' : ''}
            onClick={() => setActiveMarket(m.key)}
          >
            {m.label}
          </button>
        ))}
      </div>

      {/* Overview */}
      <div className="section">
        <div className="section-title">
          {activeMarket === 'Global' ? 'Daily Briefing' : `${activeMarket} Market Briefing`} - {briefing.date || '--'}
        </div>
        <div className="info-box info-box--surface" style={{ minHeight: 60 }}>
          {(() => {
            if (parsedOverview) {
              const marketData = getMarketData(parsedOverview, activeMarket);
              if (marketData) {
                if (typeof marketData === 'object') {
                  const summary = marketData.summary || '';
                  const keywords = marketData.keywords || [];
                  return (
                    <div>
                      <div className="briefing-summary">
                        {summary || '今日该市场无重大战略事件。'}
                      </div>
                      
                      {keywords.length > 0 && (
                        <div>
                          <div className="briefing-keywords-title">今日关键词/句</div>
                          <div className="briefing-keywords-list">
                            {keywords.map((kw, kwIdx) => (
                              <div key={kwIdx} className="keyword-item">
                                <div className="keyword-word">{kw.word}</div>
                                <div className="keyword-explanation">{kw.explanation}</div>
                                
                                {kw.event_ids && kw.event_ids.length > 0 && (
                                  <div className="keyword-events">
                                    <span className="keyword-events-label">相关新闻/事件:</span>
                                    {kw.event_ids.map((evtId) => {
                                      const matchingEvt = rawEvents.find(e => e.id === evtId);
                                      if (!matchingEvt) return null;
                                      return (
                                        <button
                                          key={evtId}
                                          className="keyword-event-link"
                                          onClick={() => handleScrollToEvent(evtId, matchingEvt.market)}
                                        >
                                          {matchingEvt.title}
                                        </button>
                                      );
                                    })}
                                  </div>
                                )}
                              </div>
                            ))}
                          </div>
                        </div>
                      )}
                    </div>
                  );
                } else if (typeof marketData === 'string') {
                  return (
                    <div className="briefing-summary">
                      {marketData || '今日该市场无重大战略事件。'}
                    </div>
                  );
                }
              }
            }

            // Fallback: If it's old plain text or we couldn't parse the market data
            if (activeMarket === 'Global') {
              return <div className="briefing-summary">{briefing.overview}</div>;
            } else {
              return (
                <div style={{ color: 'var(--color-text-secondary)', fontSize: 13 }}>
                  未检测到区域分类简报数据。请重新触发扫描以生成最新区域简报。
                </div>
              );
            }
          })()}
        </div>
      </div>

      {/* Market Heatmap */}
      {heatmapEntries.length > 0 && (
        <div className="section">
          <div className="section-title">Market Status</div>
          <table className="data-table">
            <thead>
              <tr>
                <th>Market</th>
                <th>Status</th>
                <th>Notes</th>
              </tr>
            </thead>
            <tbody>
              {heatmapEntries.map((row, idx) => (
                <tr
                  key={idx}
                  onClick={() => setActiveMarket(row.market)}
                  className="expandable"
                  style={{
                    backgroundColor: activeMarket === row.market ? 'rgba(37, 99, 235, 0.05)' : '',
                    fontWeight: activeMarket === row.market ? 600 : 'normal',
                    cursor: 'pointer'
                  }}
                >
                  <td style={{ fontWeight: 600 }}>{row.market}</td>
                  <td>
                    <span className={`status-dot ${statusDotClass(row.status)}`} />{' '}
                    <span className="text-mono text-sm">{statusText(row.status)}</span>
                  </td>
                  <td className="text-sm">{row.notes || '--'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Events */}
      <div className="section">
        <div className="section-title">Intelligence Events ({events.length})</div>
        {events.length === 0 ? (
          <div className="empty-state">No events found for {activeMarket} in this briefing.</div>
        ) : (
          <table className="data-table">
            <thead>
              <tr>
                <th>Market</th>
                <th>Category</th>
                <th>Title</th>
                <th>Impact</th>
                <th>Severity</th>
                <th>Urgency</th>
                <th>Confidence</th>
                <th>Sources</th>
                <th>Track</th>
              </tr>
            </thead>
            <tbody>
              {events.map((evt, idx) => {
                const isExpanded = expandedEvents.has(evt.id);
                return (
                  <Fragment key={evt.id || idx}>
                    <tr
                      id={evt.id ? `event-row-${evt.id}` : undefined}
                      className={`expandable ${isExpanded ? 'expanded-row' : ''}`}
                      onClick={() => evt.id && toggleExpandEvent(evt.id)}
                    >
                      <td className="text-mono text-sm">{evt.market}</td>
                      <td className="text-mono text-sm">{evt.category}</td>
                      <td style={{ fontWeight: 500, maxWidth: 280 }}>
                        <span className="event-title-link" style={{ color: 'var(--color-blue)', textDecoration: 'underline' }}>{evt.title}</span>
                      </td>
                      <td className="text-sm" style={{ maxWidth: 300 }}>{evt.impact_type || evt.impact || evt.summary || '--'}</td>
                      <td><span className={`label ${severityClass(evt.severity)}`}>{evt.severity}</span></td>
                      <td><span className={`label ${severityClass(evt.urgency)}`}>{evt.urgency}</span></td>
                      <td className="text-mono text-sm">
                        {evt.confidence != null
                          ? evt.confidence.toFixed(2)
                          : '--'}
                      </td>
                      <td className="text-sm" onClick={(e) => e.stopPropagation()}>
                        {(() => {
                          const urls = evt.source_urls || evt.sources || []
                          const srcList = Array.isArray(urls) ? urls : []
                          return srcList.length > 0
                            ? srcList.map((src, si) => (
                                <a
                                  key={si}
                                  href={src}
                                  target="_blank"
                                  rel="noopener noreferrer"
                                  style={{ display: 'block', fontSize: 11, fontFamily: 'var(--font-mono)' }}
                                >
                                  [{si + 1}]
                                </a>
                              ))
                            : <span className="text-secondary">--</span>
                        })()}
                      </td>
                      <td onClick={(e) => e.stopPropagation()}>
                        <button
                          className="btn-star"
                          onClick={() => handleToggleBookmark(evt.id)}
                          title={bookmarks.some(b => b.event_id === evt.id) ? "取消证据追踪" : "开启证据链追溯"}
                        >
                          {bookmarks.some(b => b.event_id === evt.id) ? 'Tracked' : 'Track'}
                        </button>
                      </td>
                    </tr>
                    {isExpanded && (
                      <tr className="expanded-row" onClick={(e) => e.stopPropagation()}>
                        <td colSpan={9}>
                          <div className="expanded-content">
                            <div style={{ marginBottom: 12 }}>
                              <strong style={{ display: 'block', marginBottom: 4, fontSize: 13, color: 'var(--color-text)' }}>事件摘要:</strong>
                              <span style={{ fontSize: 13, color: 'var(--color-text-secondary)', lineHeight: 1.5 }}>{evt.summary || '暂无摘要'}</span>
                            </div>
                            <div style={{ marginBottom: 12 }}>
                              <strong style={{ display: 'block', marginBottom: 4, fontSize: 13, color: 'var(--color-text)' }}>智能深度分析:</strong>
                              <span style={{ fontSize: 13, color: 'var(--color-text-secondary)', lineHeight: 1.5, whiteSpace: 'pre-line' }}>{evt.analysis || '暂无分析内容'}</span>
                            </div>
                            {evt.raw_sources && evt.raw_sources.length > 0 && (
                              <div style={{ marginTop: 12, borderTop: '1px dashed var(--color-border)', paddingTop: 12 }}>
                                <strong style={{ display: 'block', marginBottom: 6, fontSize: 13, color: 'var(--color-text)' }}>新闻原文:</strong>
                                {evt.raw_sources.map((src, sidx) => (
                                  <div key={sidx} style={{ marginBottom: 10 }}>
                                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
                                      <span style={{ fontWeight: 600, fontSize: 12, color: 'var(--color-text)' }}>
                                        [{sidx + 1}] {src.title || '无标题'}
                                      </span>
                                      <a
                                        href={src.source_url}
                                        target="_blank"
                                        rel="noopener noreferrer"
                                        style={{ fontSize: 11, color: 'var(--color-blue)', fontFamily: 'var(--font-mono)' }}
                                      >
                                        查看网页原文
                                      </a>
                                    </div>
                                    <pre style={{
                                      fontFamily: 'var(--font-mono)',
                                      fontSize: 12,
                                      lineHeight: 1.5,
                                      whiteSpace: 'pre-wrap',
                                      wordBreak: 'break-word',
                                      background: 'var(--color-bg)',
                                      border: '1px solid var(--color-border)',
                                      borderRadius: 4,
                                      padding: '10px 12px',
                                      maxHeight: '180px',
                                      overflowY: 'auto',
                                      margin: 0
                                    }}>
                                      {src.content || '无原文内容'}
                                    </pre>
                                  </div>
                                ))}
                              </div>
                            )}
                          </div>
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {/* Recommendations */}
      {recommendations.length > 0 && (
        <div className="section">
          <div className="section-title">Recommendations</div>
          <ol className="recommendations-list">
            {recommendations.map((rec, idx) => (
              <li key={idx}>{rec}</li>
            ))}
          </ol>
        </div>
      )}

      {/* Chat Panel */}
      <div className="section">
        <div className="chat-section">
          <div className="chat-header">Ask about this briefing</div>
          <div className="chat-messages">
            {chatMessages.length === 0 && (
              <div className="text-secondary text-sm">
                Ask follow-up questions about the briefing data. Messages are sent to the AI analyst.
              </div>
            )}
            {chatMessages.map((msg, idx) => (
              <div key={idx} className="chat-msg">
                <div className="chat-msg-role">{msg.role}</div>
                <div className="chat-msg-content">{msg.content}</div>
              </div>
            ))}
            {chatLoading && (
              <div className="chat-msg">
                <div className="chat-msg-role">assistant</div>
                <div className="chat-msg-content text-secondary">Thinking...</div>
              </div>
            )}
            <div ref={chatEndRef} />
          </div>
          <div className="chat-input-row">
            <input
              className="input"
              type="text"
              placeholder="Type a question..."
              value={chatInput}
              onChange={(e) => setChatInput(e.target.value)}
              onKeyDown={handleChatKeyDown}
              disabled={chatLoading || !briefing}
            />
            <button
              className="btn btn--primary"
              onClick={handleChatSend}
              disabled={chatLoading || !chatInput.trim() || !briefing}
            >
              Send
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
