import { useState, useEffect, useCallback, useRef } from 'react'

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
  switch (status) {
    case 'growth': return 'status-dot--green'
    case 'stable': return 'status-dot--blue'
    case 'warning': return 'status-dot--amber'
    case 'decline': return 'status-dot--red'
    default: return 'status-dot--gray'
  }
}

function statusText(status) {
  switch (status) {
    case 'growth': return '[GROWTH]'
    case 'stable': return '[STABLE]'
    case 'warning': return '[WARN]'
    case 'decline': return '[DECLINE]'
    default: return '[--]'
  }
}

export default function BriefingView() {
  const [briefing, setBriefing] = useState(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(null)
  const [chatMessages, setChatMessages] = useState([])
  const [chatInput, setChatInput] = useState('')
  const [chatLoading, setChatLoading] = useState(false)
  const chatEndRef = useRef(null)

  const fetchBriefing = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const res = await fetch('/api/briefing/latest')
      if (res.status === 404) {
        setBriefing(null)
        setError('No briefings available. Run a scan first to generate a briefing.')
      } else if (res.ok) {
        const data = await res.json()
        setBriefing(data)
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
  }, [])

  useEffect(() => {
    fetchBriefing()
  }, [fetchBriefing])

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
        {error || "No briefings available. Click 'Trigger Scan' to generate one."}
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
      return Object.entries(briefing.heatmap).map(([market, status]) => ({
        market,
        status: typeof status === 'string' ? status : '--',
        notes: '',
      }))
    }
    return []
  })()

  // Parse recommendations: could be array of strings or JSON
  const recommendations = (() => {
    if (!briefing.recommendations) return []
    if (Array.isArray(briefing.recommendations)) return briefing.recommendations
    return []
  })()

  // Parse events
  const events = briefing.events || []

  return (
    <div>
      {/* Overview */}
      <div className="section">
        <div className="section-title">Daily Briefing - {briefing.date || '--'}</div>
        <div className="info-box info-box--surface">
          {briefing.overview || 'No overview available.'}
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
                <tr key={idx}>
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
          <div className="empty-state">No events in this briefing.</div>
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
              </tr>
            </thead>
            <tbody>
              {events.map((evt, idx) => (
                <tr key={idx}>
                  <td className="text-mono text-sm">{evt.market}</td>
                  <td className="text-mono text-sm">{evt.category}</td>
                  <td style={{ fontWeight: 500, maxWidth: 280 }}>{evt.title}</td>
                  <td className="text-sm" style={{ maxWidth: 300 }}>{evt.impact_type || evt.impact || evt.summary || '--'}</td>
                  <td><span className={`label ${severityClass(evt.severity)}`}>{evt.severity}</span></td>
                  <td><span className={`label ${severityClass(evt.urgency)}`}>{evt.urgency}</span></td>
                  <td className="text-mono text-sm">
                    {evt.confidence != null
                      ? (evt.confidence > 1 ? evt.confidence : `${Math.round(evt.confidence * 100)}%`)
                      : '--'}
                  </td>
                  <td className="text-sm">
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
                </tr>
              ))}
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
