import { useState, useEffect, useCallback } from 'react'

export default function EvidenceTrackerView() {
  const [bookmarks, setBookmarks] = useState([])
  const [selectedBookmarkId, setSelectedBookmarkId] = useState(null)
  const [selectedChain, setSelectedChain] = useState(null)
  const [loadingList, setLoadingList] = useState(true)
  const [loadingChain, setLoadingChain] = useState(false)
  const [error, setError] = useState(null)

  const fetchBookmarks = useCallback(async () => {
    setLoadingList(true)
    setError(null)
    try {
      const res = await fetch('/api/bookmarks')
      if (res.ok) {
        const data = await res.json()
        setBookmarks(data)
        if (data.length > 0 && !selectedBookmarkId) {
          setSelectedBookmarkId(data[0].id)
        }
      } else {
        setError(`Failed to fetch bookmarks: ${res.statusText}`)
      }
    } catch (err) {
      setError(`Failed to connect to backend: ${err.message}`)
    } finally {
      setLoadingList(false)
    }
  }, [selectedBookmarkId])

  const fetchEvidenceChain = useCallback(async (bookmarkId) => {
    if (!bookmarkId) return
    setLoadingChain(true)
    try {
      const res = await fetch(`/api/bookmarks/${bookmarkId}/evidence-chain`)
      if (res.ok) {
        const data = await res.json()
        setSelectedChain(data)
      } else {
        console.error('Failed to fetch evidence chain', res.statusText)
      }
    } catch (err) {
      console.error('Connection error fetching evidence chain', err)
    } finally {
      setLoadingChain(false)
    }
  }, [])

  useEffect(() => {
    fetchBookmarks()
  }, [fetchBookmarks])

  useEffect(() => {
    if (selectedBookmarkId) {
      fetchEvidenceChain(selectedBookmarkId)
    } else {
      setSelectedChain(null)
    }
  }, [selectedBookmarkId, fetchEvidenceChain])

  const handleDeleteBookmark = async (bookmarkId) => {
    if (!window.confirm('确定取消对此新闻的证据链追踪吗？')) return
    try {
      const res = await fetch(`/api/bookmarks/${bookmarkId}`, { method: 'DELETE' })
      if (res.ok) {
        setBookmarks((prev) => {
          const next = prev.filter((b) => b.id !== bookmarkId)
          if (next.length > 0) {
            setSelectedBookmarkId(next[0].id)
          } else {
            setSelectedBookmarkId(null)
          }
          return next
        })
      } else {
        alert('取消收藏失败')
      }
    } catch (err) {
      console.error(err)
      alert('无法连接到后端服务器')
    }
  }

  if (loadingList) {
    return <div className="empty-state">加载追踪列表中...</div>
  }

  if (bookmarks.length === 0) {
    return (
      <div className="empty-state">
        <h3>暂无证据追踪事件</h3>
        <p style={{ marginTop: 8, fontSize: 13, color: 'var(--color-text-secondary)' }}>
          您可以在“Briefing (每日简报)”选项卡中，点击新闻事件旁边的 Track 按钮进行收藏。<br />
          系统会自动提取关键特征并追溯该新闻的历史前因（Past）以及追踪未来的后续进展（Future）。
        </p>
      </div>
    )
  }

  return (
    <div className="evidence-container">
      {/* Left Sidebar: Bookmarks List */}
      <aside className="evidence-sidebar">
        <div className="sidebar-title">追踪新闻列表 ({bookmarks.length})</div>
        <div className="bookmark-list">
          {bookmarks.map((bookmark) => (
            <button
              key={bookmark.id}
              className={`bookmark-item ${selectedBookmarkId === bookmark.id ? 'active' : ''}`}
              onClick={() => setSelectedBookmarkId(bookmark.id)}
            >
              <div className="bookmark-item-title">{bookmark.title}</div>
              <div className="bookmark-item-meta">
                <span>收藏于: {bookmark.created_at.split(' ')[0]}</span>
              </div>
              {bookmark.keywords && bookmark.keywords.length > 0 && (
                <div className="bookmark-tags">
                  {bookmark.keywords.map((kw, i) => (
                    <span key={i} className="bookmark-tag">
                      {kw}
                    </span>
                  ))}
                </div>
              )}
            </button>
          ))}
        </div>
      </aside>

      {/* Right Panel: Story Line Evidence Chain */}
      <main className="timeline-panel">
        {loadingChain && !selectedChain ? (
          <div className="empty-state">加载证据链分析中...</div>
        ) : selectedChain ? (
          <div>
            <header className="timeline-header">
              <div>
                <h2 className="timeline-header-title">
                  追踪目标：{selectedChain.bookmark.title}
                </h2>
                <div className="bookmark-tags" style={{ marginTop: 4 }}>
                  <span style={{ fontSize: 12, color: 'var(--color-text-secondary)', marginRight: 8, display: 'inline-flex', alignItems: 'center' }}>
                    追踪关键词：
                  </span>
                  {selectedChain.bookmark.keywords.map((kw, i) => (
                    <span key={i} className="bookmark-tag" style={{ background: 'rgba(37, 99, 235, 0.08)', color: 'var(--color-blue)', borderColor: 'rgba(37, 99, 235, 0.2)' }}>
                      {kw}
                    </span>
                  ))}
                </div>
              </div>
              <button
                className="btn btn--sm"
                onClick={() => handleDeleteBookmark(selectedChain.bookmark.id)}
                style={{ color: 'var(--color-red)', borderColor: 'rgba(220, 38, 38, 0.2)', padding: '6px 12px' }}
              >
                取消追踪
              </button>
            </header>

            <div className="timeline-flow-vertical">
              <div className="timeline-vertical-line" />

              {selectedChain.chain.map((item, index) => {
                const isPast = item.direction === 'past'
                const isCurrent = item.direction === 'current'
                const isFuture = item.direction === 'future'

                let nodeClass = 'timeline-node--current'
                let dotChar = 'C'
                let badgeText = '当前事件'
                let badgeClass = 'timeline-badge--current'

                if (isPast) {
                  nodeClass = 'timeline-node--past'
                  dotChar = 'B'
                  badgeText = '历史背景'
                  badgeClass = 'timeline-badge--past'
                } else if (isFuture) {
                  nodeClass = 'timeline-node--future'
                  dotChar = 'F'
                  badgeText = '后续进展'
                  badgeClass = 'timeline-badge--future'
                }

                return (
                  <div key={item.event_id || index} className={`timeline-node ${nodeClass}`}>
                    <div className="timeline-dot">{dotChar}</div>
                    
                    <div className="timeline-card">
                      <div className="timeline-card-header">
                        <div className="timeline-card-title">{item.title}</div>
                        <div style={{ display: 'flex', gap: 6, flexShrink: 0 }}>
                          <span className={`timeline-badge ${badgeClass}`}>{badgeText}</span>
                          {!isCurrent && (
                            <span className="match-score-badge">
                              {(item.match_score * 100).toFixed(2)}% 相似度
                            </span>
                          )}
                        </div>
                      </div>

                      <div className="timeline-card-meta">
                        <span>日期: {item.date.split(' ')[0]}</span>
                      </div>

                      <div className="timeline-card-summary">{item.summary}</div>

                      <div className="relation-box">
                        <div className={`relation-label ${isFuture ? 'relation-label--future' : 'relation-label--past'}`}>
                          {isCurrent ? '追踪状态' : isFuture ? '大模型未来追踪判定' : '大模型历史追溯判定'}
                        </div>
                        <div className="relation-content">
                          {item.relation_description}
                        </div>
                      </div>
                    </div>
                  </div>
                )
              })}
            </div>
          </div>
        ) : (
          <div className="empty-state">请从左侧选择一个追踪事件</div>
        )}
      </main>
    </div>
  )
}
