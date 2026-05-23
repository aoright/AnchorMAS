import { useState, useEffect } from 'react'

export default function AgentEvolutionCenter() {
  const [roles, setRoles] = useState([])
  const [history, setHistory] = useState([])
  const [loading, setLoading] = useState(false)
  const [evolving, setEvolving] = useState(false)
  const [evolveResult, setEvolveResult] = useState(null)

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
        <div className="section-title">Multi-Agent System (MAS) Architecture Flow</div>
        <div className="mas-diagram-container">
          <div className="mas-flow">
            <div className="mas-node-box color-scout">
              <span className="mas-node-index">01</span>
              <span className="mas-node-title">Scout Agent</span>
              <span className="mas-node-desc">Data Harvester</span>
            </div>
            <div className="mas-node-arrow">-&gt;</div>
            <div className="mas-node-box color-filter">
              <span className="mas-node-index">02</span>
              <span className="mas-node-title">Gatekeeper</span>
              <span className="mas-node-desc">Filter & Classify</span>
            </div>
            <div className="mas-node-arrow">-&gt;</div>
            <div className="mas-nodes-group">
              <div className="mas-node-index">03</div>
              <div className="mas-node-title-sub">Domain Expert Analysts</div>
              <div className="mas-nodes-grid">
                <span className="mas-grid-item">Competition</span>
                <span className="mas-grid-item">Product</span>
                <span className="mas-grid-item">Platform</span>
                <span className="mas-grid-item">Regulation</span>
                <span className="mas-grid-item">Social</span>
              </div>
            </div>
            <div className="mas-node-arrow">-&gt;</div>
            <div className="mas-node-box color-critic">
              <span className="mas-node-index">04</span>
              <span className="mas-node-title">Factual Critic</span>
              <span className="mas-node-desc">Factual Audit</span>
            </div>
            <div className="mas-node-arrow">&lt;-&gt;</div>
            <div className="mas-node-box color-refiner">
              <span className="mas-node-index">05</span>
              <span className="mas-node-title">Refiner Agent</span>
              <span className="mas-node-desc">Critique Revision</span>
            </div>
            <div className="mas-node-arrow">-&gt;</div>
            <div className="mas-node-box color-synth">
              <span className="mas-node-index">06</span>
              <span className="mas-node-title">Chief Strategist</span>
              <span className="mas-node-desc">Briefing Synth</span>
            </div>
          </div>
          
          <div className="mas-evolution-loop-bar">
            <div className="mas-loop-down">Audits critique diffs</div>
            <div className="mas-node-box color-evolution">
              <span className="mas-node-index">07</span>
              <span className="mas-node-title">Evolution Director</span>
              <span className="mas-node-desc">Playbook Auto-Mutation</span>
            </div>
            <div className="mas-loop-up">Updates SQLite dynamic playbook DB</div>
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
                    {role.system_prompt}
                  </div>
                </div>

                <div className="role-card-section">
                  <div className="role-section-label">Active Evolved Guidelines (Dynamic)</div>
                  <div className={`role-guidelines-box ${role.guidelines ? 'has-guidelines' : 'empty-guidelines'}`}>
                    {role.guidelines ? (
                      <pre>{role.guidelines}</pre>
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
          <div className="history-timeline">
            {history.map((log) => (
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
                      <strong>Evolution Reasoning:</strong> {log.reasoning}
                    </div>
                    
                    <div className="timeline-diff-grid">
                      <div className="diff-panel diff-old">
                        <span className="diff-title">Previous Evolved Guidelines</span>
                        <pre>{log.old_guidelines || '(Empty Guidelines)'}</pre>
                      </div>
                      <div className="diff-panel diff-new">
                        <span className="diff-title">Added Mutation Rules</span>
                        <pre>+ {log.new_guidelines}</pre>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
