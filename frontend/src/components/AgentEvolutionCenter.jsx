import { useState, useEffect, useMemo } from 'react'

const HISTORY_PAGE_SIZE = 20
const GLYPH_PATTERN = /[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}]/gu

function cleanDisplayText(value) {
  return String(value || '').replace(GLYPH_PATTERN, '').trim()
}

export default function AgentEvolutionCenter() {
  const [roles, setRoles] = useState([])
  const [history, setHistory] = useState([])
  const [loading, setLoading] = useState(false)
  const [evolving, setEvolving] = useState(false)
  const [evolveResult, setEvolveResult] = useState(null)
  const [historyRoleFilter, setHistoryRoleFilter] = useState('all')
  const [visibleHistoryCount, setVisibleHistoryCount] = useState(HISTORY_PAGE_SIZE)

  const fetchData = async () => {
    setLoading(true)
    try {
      const rolesRes = await fetch('/api/agent/roles')
      if (rolesRes.ok) {
        const data = await rolesRes.json()
        setRoles(data)
      }

      const historyRes = await fetch('/api/agent/evolution-history')
      if (historyRes.ok) {
        const data = await historyRes.json()
        setHistory(data)
      }
    } catch (e) {
      console.error("Failed to load agent roles/history", e)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    fetchData()
  }, [])

  const handleTriggerEvolve = async () => {
    setEvolving(true)
    setEvolveResult(null)
    try {
      const res = await fetch('/api/agent/evolve', { method: 'POST' })
      if (res.ok) {
        setEvolveResult({
          status: 'success',
          message: 'Agent evolution triggered in background. Please wait 10-15 seconds for the LLM to complete its analysis, and refresh this page.',
        })
        setTimeout(fetchData, 15000)
      } else {
        setEvolveResult({
          status: 'error',
          message: 'Failed to trigger agent evolution.',
        })
      }
    } catch {
      setEvolveResult({
        status: 'error',
        message: 'Network error triggering agent evolution.',
      })
    } finally {
      setEvolving(false)
    }
  }

  // Helper to map role ID to category color or class
  const getRoleColor = (roleId) => {
    if (roleId.startsWith('analyst_')) return 'label--info'
    if (roleId === 'critic') return 'label--high'
    if (roleId === 'filter') return 'label--medium'
    if (roleId === 'synthesizer') return 'label--low'
    return 'label--info'
  }

  const getRoleName = (roleId) => {
    const found = roles.find(r => r.role_id === roleId)
    return found ? found.name : roleId
  }

  const historyRoleOptions = useMemo(() => {
    const ids = new Set(history.map(log => log.role_id).filter(Boolean))
    const orderedRoles = roles
      .filter(role => ids.has(role.role_id))
      .map(role => ({ role_id: role.role_id, name: role.name }))
    const orderedIds = new Set(orderedRoles.map(role => role.role_id))
    const unknownRoles = [...ids]
      .filter(roleId => !orderedIds.has(roleId))
      .sort()
      .map(roleId => ({ role_id: roleId, name: roleId }))

    return [...orderedRoles, ...unknownRoles]
  }, [history, roles])

  const filteredHistory = useMemo(() => {
    if (historyRoleFilter === 'all') return history
    return history.filter(log => log.role_id === historyRoleFilter)
  }, [history, historyRoleFilter])

  const visibleHistory = useMemo(() => {
    return filteredHistory.slice(0, visibleHistoryCount)
  }, [filteredHistory, visibleHistoryCount])

  const handleHistoryFilterChange = (event) => {
    setHistoryRoleFilter(event.target.value)
    setVisibleHistoryCount(HISTORY_PAGE_SIZE)
  }

  return (
    <div className="evolution-center">
      {/* Header */}
      <div className="evolution-banner">
        <div className="evolution-banner-content">
          <h2>Agent Evolution Control Center</h2>
          <p>
            Dynamic multi-agent consensus, reflection, and reinforcement loop. The Evolution Agent reviews
            factual verifier critiques and automatically mutates analyst prompts to evolve domain-specific guidelines.
          </p>
        </div>
        <div className="evolution-banner-action">
          <button
            className="btn btn--primary btn-evolve"
            onClick={handleTriggerEvolve}
            disabled={evolving || loading}
          >
            {evolving ? 'Evaluating logs...' : 'Trigger Evolution Cycle'}
          </button>
        </div>
      </div>

      {evolveResult && (
        <div className={`info-box ${evolveResult.status === 'success' ? 'info-box-success' : 'info-box-error'}`} style={{ marginBottom: 24 }}>
          {evolveResult.message}
        </div>
      )}

      {/* MAS Architecture Section */}
      <div className="section">
        <div className="section-title">Multi-Agent System (MAS) Blackboard Architecture Flow</div>
        
        <div className="mas-blackboard-container">
          
          {/* Left Side: Input Pipeline */}
          <div className="mas-side-column mas-left-column">
            <div className="mas-column-header">Ingestion Pipeline</div>
            
            <div className="mas-node-item scout-agent">
              <div className="node-icon">01</div>
              <div className="node-info">
                <span className="node-title">Scout Agent</span>
                <span className="node-desc">Data Harvester</span>
              </div>
            </div>
            
            <div className="mas-connector-arrow">↓</div>
            
            <div className="mas-node-item gatekeeper-agent">
              <div className="node-icon">02</div>
              <div className="node-info">
                <span className="node-title">Gatekeeper (Filter)</span>
                <span className="node-desc">Noise Filter & Classifier</span>
              </div>
              <div className="badge-custom">Spawns custom roles</div>
            </div>
            
            <div className="mas-connector-arrow">↓</div>
            
            <div className="mas-node-item deduplicator-agent">
              <div className="node-icon">03</div>
              <div className="node-info">
                <span className="node-title">Deduplicator</span>
                <span className="node-desc">Similarity Check</span>
              </div>
            </div>
            
            <div className="mas-column-link-right">Writes Event Data →</div>
          </div>

          {/* Center: The Blackboard Hub & Analysts Pool */}
          <div className="mas-center-column">
            <div className="blackboard-hub">
              <div className="hub-glow-ring"></div>
              <div className="hub-content">
                <div className="hub-badge">Core Hub</div>
                <h4>BLACKBOARD</h4>
                <p className="hub-subtitle">SQLite & Vector DB State Storage</p>
                <div className="hub-data-tags">
                  <span>Raw Articles</span>
                  <span>Filtered Events</span>
                  <span>Critique Notes</span>
                  <span>Consensus</span>
                </div>
              </div>
            </div>

            {/* Dynamic Analysts Pool */}
            <div className="analyst-pool-container">
              <div className="pool-header">
                <span>Domain Expert Pool</span>
                <span className="pool-desc">Parallel Dispatch / Auto-Evolution</span>
              </div>
              <div className="analyst-pool-grid">
                {/* Core and Custom categories */}
                {roles.filter(r => r.role_id.startsWith('analyst_')).map((role) => {
                  const isCustom = !['analyst_competition', 'analyst_product', 'analyst_platform', 'analyst_regulation', 'analyst_social'].includes(role.role_id);
                  return (
                    <div key={role.role_id} className={`analyst-node-card ${isCustom ? 'custom-agent-node' : ''}`}>
                      <div className="analyst-node-status">
                        <span className="pulse-dot"></span>
                        v{role.version}
                      </div>
                      <span className="analyst-node-title">{role.name.replace('分析特工', '')}</span>
                      {isCustom && <span className="custom-spawn-badge">Dynamically Spawned</span>}
                    </div>
                  )
                })}
              </div>
            </div>
          </div>

          {/* Right Side: Verification, Refinement & Synthesis */}
          <div className="mas-side-column mas-right-column">
            <div className="mas-column-header">Quality Assurance & Strategy</div>
            
            <div className="mas-node-item peer-agent">
              <div className="node-icon">04</div>
              <div className="node-info">
                <span className="node-title">Peer Reviewer</span>
                <span className="node-desc">Cross-Domain Peer Feedback</span>
              </div>
            </div>
            
            <div className="mas-connector-arrow">↓</div>
            
            <div className="verification-loop-box">
              <div className="verification-loop-title">Factual Alignment Loop</div>
              
              <div className="mas-node-item critic-agent">
                <div className="node-icon">05</div>
                <div className="node-info">
                  <span className="node-title">Factual Critic</span>
                  <span className="node-desc">Sources Fact-Check Auditor</span>
                </div>
              </div>
              
              <div className="loop-arrow-bidirectional">⇅ Critique & Rewrite</div>
              
              <div className="mas-node-item refiner-agent">
                <div className="node-icon">06</div>
                <div className="node-info">
                  <span className="node-title">Refiner Agent</span>
                  <span className="node-desc">Critique Revision</span>
                </div>
              </div>
            </div>
            
            <div className="mas-connector-arrow">↓</div>
            
            <div className="mas-node-item strategist-agent">
              <div className="node-icon">07</div>
              <div className="node-info">
                <span className="node-title">Chief Strategist</span>
                <span className="node-desc">Briefing Synthesizer</span>
              </div>
            </div>
          </div>

          {/* Evolution Overlay / Top Panel */}
          <div className="mas-evolution-overlay-bar">
            <div className="evolution-director-node">
              <div className="node-info-inline">
                <span className="director-badge">Meta-Agent Architect</span>
                <strong>Evolution Director (Methodology Auditor) & Designer Agent</strong>
                <p>Monitors feedback log &rarr; Auto-mutates instructions in SQLite Playbook & Spawns custom analysts</p>
              </div>
              <div className="evolution-loop-svg">
                <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67"/>
                </svg>
              </div>
            </div>
          </div>
          
        </div>
      </div>

      {/* Grid of Agent Roles */}
      <div className="section">
        <div className="section-title">Active Agent Playbook</div>
        {loading && roles.length === 0 ? (
          <div className="empty-state">Loading active playbooks...</div>
        ) : (
          <div className="roles-grid">
            {roles.map((role) => (
              <div key={role.role_id} className="role-card">
                <div className="role-card-header">
                  <div>
                    <h3 className="role-card-title">{role.name}</h3>
                    <span className="role-card-id">{role.role_id}</span>
                  </div>
                  <div className="role-version-badge">
                    v{role.version}
                  </div>
                </div>
                
                <div className="role-card-section">
                  <div className="role-section-label">Base Capabilities System Prompt</div>
                  <div className="role-prompt-box">
                    {cleanDisplayText(role.system_prompt)}
                  </div>
                </div>

                <div className="role-card-section">
                  <div className="role-section-label">Active Evolved Guidelines (Dynamic)</div>
                  <div className={`role-guidelines-box ${role.guidelines ? 'has-guidelines' : 'empty-guidelines'}`}>
                    {role.guidelines ? (
                      <pre>{cleanDisplayText(role.guidelines)}</pre>
                    ) : (
                      <span>No evolved guidelines yet. Guidelines will evolve dynamically based on verifier feedback and error critique.</span>
                    )}
                  </div>
                </div>

                <div className="role-card-footer">
                  <span>Last updated: {new Date(role.updated_at).toLocaleString()}</span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Evolution History Logs */}
      <div className="section" style={{ marginTop: 40 }}>
        <div className="section-title">Agent Evolution Logs (Mutation History)</div>
        {loading && history.length === 0 ? (
          <div className="empty-state">Loading evolution logs...</div>
        ) : history.length === 0 ? (
          <div className="empty-state">No evolution cycles recorded yet. Generate briefings with verifier rejects to trigger auto-evolution.</div>
        ) : (
          <>
            <div className="history-toolbar">
              <select
                className="input"
                value={historyRoleFilter}
                onChange={handleHistoryFilterChange}
              >
                <option value="all">All Agents</option>
                {historyRoleOptions.map((role) => (
                  <option key={role.role_id} value={role.role_id}>
                    {role.name} ({role.role_id})
                  </option>
                ))}
              </select>
              <span className="history-count">
                Showing {visibleHistory.length} of {filteredHistory.length} logs
              </span>
            </div>

            {filteredHistory.length === 0 ? (
              <div className="empty-state">No evolution logs match the selected agent.</div>
            ) : (
              <>
                <div className="history-timeline">
                  {visibleHistory.map((log) => (
                    <div key={log.id} className="timeline-item">
                      <div className="timeline-marker" />
                      <div className="timeline-content-card">
                        <div className="timeline-header">
                          <div>
                            <span className={`label ${getRoleColor(log.role_id)}`} style={{ marginRight: 8 }}>
                              {getRoleName(log.role_id)}
                            </span>
                            <span className="timeline-role-id">({log.role_id})</span>
                          </div>
                          <span className="timeline-time">
                            {new Date(log.created_at).toLocaleString()}
                          </span>
                        </div>
                        
                        <div className="timeline-body">
                          <div className="timeline-reasoning">
                            <strong>Evolution Reasoning:</strong> {cleanDisplayText(log.reasoning)}
                          </div>
                          
                          <div className="timeline-diff-grid">
                            <div className="diff-panel diff-old">
                              <span className="diff-title">Previous Evolved Guidelines</span>
                              <pre>{cleanDisplayText(log.old_guidelines) || '(Empty Guidelines)'}</pre>
                            </div>
                            <div className="diff-panel diff-new">
                              <span className="diff-title">Added Mutation Rules</span>
                              <pre>+ {cleanDisplayText(log.new_guidelines)}</pre>
                            </div>
                          </div>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>

                {visibleHistory.length < filteredHistory.length && (
                  <div className="history-load-row">
                    <button
                      className="btn"
                      onClick={() => setVisibleHistoryCount(count => count + HISTORY_PAGE_SIZE)}
                    >
                      Load 20 More
                    </button>
                  </div>
                )}
              </>
            )}
          </>
        )}
      </div>
    </div>
  )
}
