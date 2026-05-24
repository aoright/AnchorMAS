import { useState, useEffect } from 'react'

export default function AgentParliament() {
  const [registry, setRegistry] = useState([])
  const [ledger, setLedger] = useState([])
  const [proposals, setProposals] = useState([])
  const [loading, setLoading] = useState(false)
  const [trialRunning, setTrialRunning] = useState(false)
  const [trialLog, setTrialLog] = useState(null)
  
  // Forms
  const [proposalProposer, setProposalProposer] = useState('')
  const [proposalType, setProposalType] = useState('constitutional')
  const [proposalTitle, setProposalTitle] = useState('')
  const [proposalDesc, setProposalDesc] = useState('')
  const [proposalResult, setProposalResult] = useState(null)
  const [submittingProposal, setSubmittingProposal] = useState(false)

  const [parentA, setParentA] = useState('')
  const [parentB, setParentB] = useState('')
  const [crossoverCategory, setCrossoverCategory] = useState('')
  const [crossoverResult, setCrossoverResult] = useState(null)
  const [breeding, setBreeding] = useState(false)

  const [notification, setNotification] = useState(null)

  const fetchData = async () => {
    setLoading(true)
    try {
      const regRes = await fetch('/api/parliament/registry')
      if (regRes.ok) {
        const data = await regRes.json()
        setRegistry(data)
        // Set default proposer if not set
        if (data.length > 0 && !proposalProposer) {
          setProposalProposer(data[0].role_id)
          setParentA(data[0].role_id)
          if (data.length > 1) {
            setParentB(data[1].role_id)
          } else {
            setParentB(data[0].role_id)
          }
        }
      }

      const ledRes = await fetch('/api/parliament/ledger')
      if (ledRes.ok) {
        const data = await ledRes.json()
        setLedger(data)
      }

      const propRes = await fetch('/api/parliament/proposals')
      if (propRes.ok) {
        const data = await propRes.json()
        setProposals(data)
      }
    } catch (e) {
      console.error("Failed to load parliament data", e)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    fetchData()
  }, [])

  const triggerStagnationTrial = async () => {
    setTrialRunning(true)
    setTrialLog(null)
    try {
      const res = await fetch('/api/parliament/trial', { method: 'POST' })
      const data = await res.json()
      if (res.ok) {
        setTrialLog(data.log)
        fetchData()
      } else {
        setTrialLog("Trial failed: " + (data.error || "Unknown error"))
      }
    } catch (e) {
      setTrialLog("Trial connection error: " + e.message)
    } finally {
      setTrialRunning(false)
    }
  }

  const triggerCrossover = async (e) => {
    e.preventDefault()
    if (!parentA || !parentB || !crossoverCategory) {
      showNotice("Please fill in all crossover fields.", "error")
      return
    }
    if (parentA === parentB) {
      showNotice("Crossover requires two different parent agents.", "error")
      return
    }

    setBreeding(true)
    setCrossoverResult(null)
    try {
      const res = await fetch('/api/parliament/crossover', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          parent_a: parentA,
          parent_b: parentB,
          category: crossoverCategory,
        })
      })
      const data = await res.json()
      if (res.ok) {
        setCrossoverResult(data.message)
        setCrossoverCategory('')
        fetchData()
      } else {
        showNotice("Crossover failed: " + (data.error || "Unknown error"), "error")
      }
    } catch (e) {
      showNotice("Crossover connection error: " + e.message, "error")
    } finally {
      setBreeding(false)
    }
  }

  const triggerProposal = async (e) => {
    e.preventDefault()
    if (!proposalTitle || !proposalDesc) {
      showNotice("Please fill in proposal title and description.", "error")
      return
    }

    setSubmittingProposal(true)
    setProposalResult(null)
    try {
      const res = await fetch('/api/parliament/proposals', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          proposer_role_id: proposalProposer,
          proposal_type: proposalType,
          title: proposalTitle,
          description: proposalDesc,
        })
      })
      const data = await res.json()
      if (res.ok) {
        setProposalResult(data.summary)
        setProposalTitle('')
        setProposalDesc('')
        fetchData()
      } else {
        showNotice("Failed to submit proposal: " + (data.error || "Unknown error"), "error")
      }
    } catch (e) {
      showNotice("Proposal submission error: " + e.message, "error")
    } finally {
      setSubmittingProposal(false)
    }
  }

  const triggerWeeklyDistribution = async () => {
    try {
      const res = await fetch('/api/parliament/distribute', { method: 'POST' })
      const data = await res.json()
      if (res.ok) {
        showNotice(data.message || "Credits distributed successfully!", "success")
        fetchData()
      } else {
        showNotice("Distribution failed: " + (data.error || "Unknown error"), "error")
      }
    } catch (e) {
      showNotice("Distribution connection error: " + e.message, "error")
    }
  }

  const showNotice = (msg, type = 'success') => {
    setNotification({ msg, type })
    setTimeout(() => setNotification(null), 5000)
  }

  // Derived Stats
  const activeCount = registry.filter(a => a.status === 'active').length
  const probationCount = registry.filter(a => a.status === 'probation').length
  const paroleCount = registry.filter(a => a.status === 'parole').length
  const bankruptCount = registry.filter(a => a.status === 'bankruptcy').length

  const totalCredits = registry.reduce((acc, a) => acc + (a.compute_credits || 0), 0)

  // Graveyard: trial verdicts where status was destroy/execution
  const graveyardEntries = ledger.filter(log => {
    if (log.event_type !== 'trial_verdict') return false
    const details = log.details || {}
    return details.verdict === 'destroy'
  })

  // Format faction name
  const getFactionBadgeClass = (faction) => {
    if (faction === 'Creativity') return 'badge-creativity'
    if (faction === 'Efficiency') return 'badge-efficiency'
    return 'badge-neutral'
  }

  const getStatusBadgeClass = (status) => {
    if (status === 'active') return 'badge-status-active'
    if (status === 'probation') return 'badge-status-probation'
    if (status === 'parole') return 'badge-status-parole'
    return 'badge-status-bankrupt'
  }

  return (
    <div className="parliament-container">
      {/* Parliament Banner */}
      <div className="parliament-banner">
        <div className="banner-glow-effect"></div>
        <div className="banner-content">
          <div className="banner-header">
            <span className="banner-badge">Meta-Governance Office</span>
            <h2>Agent Parliament & Governance</h2>
          </div>
          <p>
            An autonomous democratic collective enforcing resource efficiency and targeted evolutionary progression. 
            PRESIDED BY: <strong>The Speaker (Meta-Agent)</strong>. Enforces resource controls, processes stagnation trials, 
            presides over weighted legislation voting, and hybridization.
          </p>
        </div>
        <div className="banner-actions">
          <button 
            className="btn btn--secondary" 
            onClick={triggerWeeklyDistribution}
            disabled={loading}
          >
            Central Bank: Allocate Credits
          </button>
        </div>
      </div>

      {notification && (
        <div className={`notification-bar ${notification.type === 'error' ? 'notice-error' : 'notice-success'}`}>
          {notification.msg}
        </div>
      )}

      {/* Grid of Stats */}
      <div className="parliament-stats-grid">
        <div className="stat-card">
          <div className="stat-label">Speaker of the House</div>
          <div className="stat-value text-blue">Presiding</div>
          <div className="stat-subtitle">Auto-Registry Auditor</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Active Seats</div>
          <div className="stat-value">{activeCount}</div>
          <div className="stat-subtitle">{probationCount} Sandbox Probation | {paroleCount} Parole</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Central Bank Credits</div>
          <div className="stat-value text-green">{totalCredits.toLocaleString()}</div>
          <div className="stat-subtitle">{bankruptCount} Bankrupt (Restricted)</div>
        </div>
        <div className="stat-card">
          <div className="stat-label">Ledger Entries</div>
          <div className="stat-value text-amber">{ledger.length}</div>
          <div className="stat-subtitle">Immutable audit trail</div>
        </div>
      </div>

      <div className="parliament-layout">
        {/* Left Column: Registry & Graveyard */}
        <div className="parliament-main-col">
          {/* Active Registry */}
          <div className="parliament-section">
            <div className="section-header-row">
              <h3 className="section-title">Active Agent Registry</h3>
              <button className="btn btn-sm btn--outline" onClick={fetchData} disabled={loading}>
                {loading ? 'Syncing...' : 'Sync Registry'}
              </button>
            </div>
            
            <div className="table-responsive">
              <table className="parliament-table">
                <thead>
                  <tr>
                    <th>Agent Name</th>
                    <th>Faction</th>
                    <th>Status</th>
                    <th>Task Success (SR)</th>
                    <th>Compute Credits</th>
                    <th>Token Cost</th>
                  </tr>
                </thead>
                <tbody>
                  {registry.map(agent => {
                    const totalTasks = agent.tasks_completed + agent.tasks_failed
                    const sr = totalTasks > 0 ? ((agent.tasks_completed / totalTasks) * 100).toFixed(1) + '%' : '100.0%'
                    const balancePercent = Math.min(100, Math.max(0, (agent.compute_credits / 100000) * 100))
                    
                    return (
                      <tr key={agent.role_id}>
                        <td>
                          <div className="agent-name-cell">
                            <strong>{agent.name}</strong>
                            <span className="agent-role-id">{agent.role_id}</span>
                            {agent.sponsor_role_id && (
                              <span className="agent-sponsor">Sponsor: {agent.sponsor_role_id.replace('analyst_', '')}</span>
                            )}
                          </div>
                        </td>
                        <td>
                          <span className={`badge ${getFactionBadgeClass(agent.faction)}`}>
                            {agent.faction}
                          </span>
                        </td>
                        <td>
                          <span className={`badge ${getStatusBadgeClass(agent.status)}`}>
                            {agent.status.toUpperCase()}
                          </span>
                        </td>
                        <td>
                          <div className="sr-cell">
                            <strong>{sr}</strong>
                            <span className="sr-detail">{agent.tasks_completed} ok / {agent.tasks_failed} fail</span>
                          </div>
                        </td>
                        <td>
                          <div className="credit-cell">
                            <span className={agent.compute_credits <= 0 ? 'text-danger font-bold' : 'font-mono'}>
                              {agent.compute_credits.toLocaleString()}
                            </span>
                            <div className="progress-bar-container">
                              <div 
                                className={`progress-fill ${agent.compute_credits <= 0 ? 'bg-danger' : agent.compute_credits < 30000 ? 'bg-warning' : 'bg-success'}`}
                                style={{ width: `${balancePercent}%` }}
                              />
                            </div>
                          </div>
                        </td>
                        <td className="font-mono text-secondary">
                          {agent.token_cost.toLocaleString()}
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
          </div>

          {/* Governance Proposals */}
          <div className="parliament-section">
            <h3 className="section-title">Legislation & Proposals</h3>
            
            <div className="proposals-split">
              {/* Proposal List */}
              <div className="proposals-list-pane">
                {proposals.length === 0 ? (
                  <div className="empty-slate">No governance proposals recorded yet.</div>
                ) : (
                  <div className="proposals-stack">
                    {proposals.map(prop => {
                      const totalVotes = prop.yes_votes + prop.no_votes
                      const yesPercent = totalVotes > 0 ? ((prop.yes_votes / totalVotes) * 100).toFixed(1) : '0.0'
                      const noPercent = totalVotes > 0 ? ((prop.no_votes / totalVotes) * 100).toFixed(1) : '0.0'
                      
                      return (
                        <div key={prop.id} className={`proposal-item-card status-${prop.status}`}>
                          <div className="proposal-item-header">
                            <div>
                              <span className="proposal-item-type">{prop.proposal_type.toUpperCase()}</span>
                              <h4>{prop.title}</h4>
                            </div>
                            <span className={`badge badge-proposal-${prop.status}`}>{prop.status.toUpperCase()}</span>
                          </div>
                          <p className="proposal-item-desc">{prop.description}</p>
                          <div className="proposal-item-footer">
                            <div className="proposal-votes-gauge">
                              <span>Yes: {yesPercent}% ({prop.yes_votes.toFixed(1)}w)</span>
                              <div className="gauge-track">
                                <div className="gauge-fill-yes" style={{ width: `${yesPercent}%` }} />
                                <div className="gauge-fill-no" style={{ width: `${noPercent}%` }} />
                              </div>
                              <span>No: {noPercent}% ({prop.no_votes.toFixed(1)}w)</span>
                            </div>
                            <div className="proposal-meta-info">
                              <span>Proposer: {prop.proposer_role_id}</span>
                              <span>Date: {new Date(prop.created_at).toLocaleDateString()}</span>
                            </div>
                          </div>
                        </div>
                      )
                    })}
                  </div>
                )}
              </div>

              {/* Submit Proposal Form */}
              <div className="proposals-form-pane">
                <form className="parliament-form card-form" onSubmit={triggerProposal}>
                  <h4>Draft New Governance Bill</h4>
                  
                  <div className="form-group">
                    <label>Proposing Agent Seat</label>
                    <select 
                      className="input"
                      value={proposalProposer}
                      onChange={e => setProposalProposer(e.target.value)}
                    >
                      {registry.filter(a => a.status === 'active' || a.status === 'parole').map(agent => (
                        <option key={agent.role_id} value={agent.role_id}>
                          {agent.name} ({agent.role_id})
                        </option>
                      ))}
                    </select>
                  </div>

                  <div className="form-group">
                    <label>Legislation Category</label>
                    <select 
                      className="input"
                      value={proposalType}
                      onChange={e => setProposalType(e.target.value)}
                    >
                      <option value="constitutional">Constitutional (Amend Rules, 66% weight threshold)</option>
                      <option value="budget">Budget allocation & central bank grants (50% threshold)</option>
                      <option value="merger">Merger & knowledge fusion (50% threshold)</option>
                      <option value="admission">Admission of new candidate (50% threshold)</option>
                    </select>
                  </div>

                  <div className="form-group">
                    <label>Proposal Title</label>
                    <input 
                      type="text" 
                      className="input" 
                      placeholder="e.g. Set core voting threshold to 66%"
                      value={proposalTitle}
                      onChange={e => setProposalTitle(e.target.value)}
                    />
                  </div>

                  <div className="form-group">
                    <label>Proposal Details / Directive Description</label>
                    <textarea 
                      className="input input-textarea" 
                      placeholder="Specify rationale and A/B variables. To trigger Self-Legislation rule amendment, include: Update Key: [key_name] Value: [new_value]"
                      value={proposalDesc}
                      onChange={e => setProposalDesc(e.target.value)}
                    />
                  </div>

                  <button className="btn btn--primary w-100" type="submit" disabled={submittingProposal}>
                    {submittingProposal ? 'Simulating House Vote...' : 'Submit & Trigger Debate'}
                  </button>

                  {proposalResult && (
                    <div className="form-result-box margin-top-12 success-box">
                      <strong>Debate Consensus Result:</strong>
                      <p>{proposalResult}</p>
                    </div>
                  )}
                </form>
              </div>
            </div>
          </div>

          {/* Stagnation Trial Arena */}
          <div className="parliament-section">
            <h3 className="section-title">The Trial of Stagnation Courtroom</h3>
            <p className="section-desc">
              Suspends stagnating, redundant, or low-performing agents. Opens a sandboxed court containing Public Accusation, 
              Jury LLM voting, and executes parole, merge, or wipe commands.
            </p>

            <div className="trial-panel">
              <div className="trial-controls">
                <button 
                  className="btn btn--danger btn-lg" 
                  onClick={triggerStagnationTrial}
                  disabled={trialRunning}
                >
                  {trialRunning ? 'Presiding over Court...' : 'Trigger Stagnation Audit & Trial'}
                </button>
              </div>

              {trialRunning && (
                <div className="courtroom-anim-box">
                  <div className="scale-glow-ring">Court</div>
                  <div className="courtroom-loading-text">
                    The Sandbox Court is in session. Accusing stagnated agents, requesting defense briefs, and convening the jury...
                  </div>
                </div>
              )}

              {trialLog && (
                <div className="trial-result-log">
                  <h4>Courtroom Stenographer Transcript</h4>
                  <pre>{trialLog}</pre>
                </div>
              )}
            </div>
          </div>

          {/* Cemetery / Graveyard */}
          <div className="parliament-section graveyard-section">
            <div className="graveyard-fog"></div>
            <h3 className="section-title text-white">The Cemetery of Decommissioned Agents</h3>
            <p className="section-desc text-gray">
              Wiped agents reside here. Their permanent logs prevent historical errors from repeating, 
              and their "Last Words Prompts" continue to enrich the shared repository.
            </p>

            {graveyardEntries.length === 0 ? (
              <div className="cemetery-empty text-center">
                <p>The graveyard is currently empty. All agents are operational or in parole.</p>
              </div>
            ) : (
              <div className="tombstone-grid">
                {graveyardEntries.map(log => {
                  const details = log.details || {}
                  
                  return (
                    <div key={log.id} className="tombstone-card">
                      <div className="tombstone-header">
                        <span className="tombstone-role">ID: {log.role_id}</span>
                        <span className="tombstone-date">Ended: {new Date(log.created_at).toLocaleDateString()}</span>
                      </div>
                      <div className="tombstone-body">
                        <div className="tombstone-grave-icon">†</div>
                        <h5>{details.accusation ? details.accusation.split('因')[0].replace('智能体【', '').replace('】', '') : 'Unknown Agent'}</h5>
                        <p className="death-note">
                          <strong>Death Note:</strong> {details.death_note || 'Redundant functionality and low jury score.'}
                        </p>
                        {details.last_words && (
                          <div className="last-words-box">
                            <strong>Last Words Spark:</strong>
                            <p>"{details.last_words}"</p>
                          </div>
                        )}
                      </div>
                    </div>
                  )
                })}
              </div>
            )}
          </div>
        </div>

        {/* Right Column: Crossover & Ledger */}
        <div className="parliament-side-col">
          {/* Hybrid Propagation */}
          <div className="parliament-section">
            <h3 className="section-title">Crossover & Sandbox Admission</h3>
            <p className="section-desc">
              Breed parent agents to synthesize a new specialized offspring. Offspring starts in sandbox probation 
              and must complete M tasks successfully to earn a full seat.
            </p>

            <form className="parliament-form side-form" onSubmit={triggerCrossover}>
              <div className="form-group">
                <label>Parent Agent A</label>
                <select 
                  className="input"
                  value={parentA}
                  onChange={e => setParentA(e.target.value)}
                >
                  {registry.filter(a => a.status === 'active' || a.status === 'parole').map(agent => (
                    <option key={agent.role_id} value={agent.role_id}>
                      {agent.name} ({agent.role_id})
                    </option>
                  ))}
                </select>
              </div>

              <div className="form-group">
                <label>Parent Agent B</label>
                <select 
                  className="input"
                  value={parentB}
                  onChange={e => setParentB(e.target.value)}
                >
                  {registry.filter(a => a.status === 'active' || a.status === 'parole').map(agent => (
                    <option key={agent.role_id} value={agent.role_id}>
                      {agent.name} ({agent.role_id})
                    </option>
                  ))}
                </select>
              </div>

              <div className="form-group">
                <label>New Category Focus Name</label>
                <input 
                  type="text" 
                  className="input"
                  placeholder="e.g. LegalDisputes"
                  value={crossoverCategory}
                  onChange={e => setCrossoverCategory(e.target.value)}
                />
              </div>

              <button className="btn btn--primary w-100" type="submit" disabled={breeding}>
                {breeding ? 'Synthesizing Offspring...' : 'Trigger Hybrid Crossover'}
              </button>

              {crossoverResult && (
                <div className="form-result-box margin-top-12 success-box">
                  <strong>Offspring Created:</strong>
                  <p>{crossoverResult}</p>
                </div>
              )}
            </form>
          </div>

          {/* Parliament Ledger */}
          <div className="parliament-section">
            <h3 className="section-title">Immutable Ledger Logs</h3>
            <p className="section-desc text-secondary">Tamper-evident logs of all parliament decisions.</p>
            
            <div className="ledger-timeline">
              {ledger.length === 0 ? (
                <div className="empty-slate-dense">No ledger logs yet.</div>
              ) : (
                ledger.map(log => {
                  let badgeClass = 'ledger-badge-default'
                  let eventTitle = log.event_type.replace('_', ' ').toUpperCase()
                  
                  if (log.event_type === 'trial_verdict') badgeClass = 'ledger-badge-trial'
                  else if (log.event_type === 'proposal_result') badgeClass = 'ledger-badge-proposal'
                  else if (log.event_type === 'bankruptcy') badgeClass = 'ledger-badge-bankruptcy'
                  else if (log.event_type === 'crossover') badgeClass = 'ledger-badge-crossover'
                  else if (log.event_type === 'admission') badgeClass = 'ledger-badge-admission'
                  
                  const details = log.details || {}
                  let summary = ''
                  if (log.event_type === 'trial_verdict') {
                    summary = `Verdict: ${details.verdict?.toUpperCase()}. Agent: ${log.role_id}`
                  } else if (log.event_type === 'proposal_result') {
                    summary = `Proposal "${details.title || ''}" was ${details.passed ? 'PASSED' : 'REJECTED'} (${(details.yes_ratio * 100).toFixed(0)}% yes).`
                  } else if (log.event_type === 'crossover') {
                    summary = `Bred child: ${details.child_role_id} from ${details.parent_a?.replace('analyst_', '')} and ${details.parent_b?.replace('analyst_', '')}`
                  } else if (log.event_type === 'bankruptcy') {
                    summary = `Agent ${log.role_id} declared bankrupt (Credits: ${details.final_credits})`
                  } else {
                    summary = JSON.stringify(details)
                  }

                  return (
                    <div key={log.id} className="ledger-timeline-item">
                      <div className="ledger-timeline-header">
                        <span className={`ledger-type-badge ${badgeClass}`}>{eventTitle}</span>
                        <span className="ledger-timeline-time">{new Date(log.created_at).toLocaleTimeString()}</span>
                      </div>
                      <p className="ledger-timeline-summary">{summary}</p>
                    </div>
                  )
                })
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
