import { useState, useEffect, useCallback } from 'react'

const PIPELINE_STEPS = [
  { key: 'harvester', name: 'Harvester', description: 'Scrape raw articles from sources' },
  { key: 'filter', name: 'Filter', description: 'Deduplicate and relevance filter' },
  { key: 'analyst', name: 'Analyst', description: 'AI analysis and event extraction' },
  { key: 'verifier', name: 'Verifier', description: 'Cross-reference and verify facts' },
  { key: 'synthesizer', name: 'Synthesizer', description: 'Generate strategic briefing' },
]

const DEFAULT_STATUS = {
  status: 'idle',
  current_step: null,
  last_run: null,
  error_message: null,
  stats: {
    raw_count: 0,
    filtered_count: 0,
    analyzed_count: 0,
    verified_count: 0,
  },
  progress: {
    message: null,
    processed_count: 0,
    total_count: 0,
    output_count: 0,
    batch_index: null,
    batch_total: null,
    completed_batches: 0,
    failed_batches: 0,
    last_error: null,
    updated_at: null,
  },
}

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

function getActiveStepIndex(currentStep, stats) {
  const explicitIndex = PIPELINE_STEPS.findIndex((step) => step.key === currentStep)
  if (explicitIndex >= 0) return explicitIndex

  if (!stats?.raw_count) return 0
  if (!stats?.filtered_count) return 1
  if (!stats?.analyzed_count) return 2
  if (!stats?.verified_count) return 3
  return 4
}

function getStepStatus(pipelineStatus, stepIndex, stats, currentStep) {
  if (pipelineStatus === 'completed') {
    return { status: 'done', count: getStepCount(stepIndex, stats) }
  }
  if (pipelineStatus === 'idle') {
    return { status: 'idle', count: getStepCount(stepIndex, stats) }
  }

  const activeStep = getActiveStepIndex(currentStep, stats)

  if (pipelineStatus === 'error') {
    if (stepIndex < activeStep) return { status: 'done', count: getStepCount(stepIndex, stats) }
    if (stepIndex === activeStep) return { status: 'error', count: getStepCount(stepIndex, stats) }
    return { status: 'idle', count: '--' }
  }
  // running
  if (stepIndex < activeStep) return { status: 'done', count: getStepCount(stepIndex, stats) }
  if (stepIndex === activeStep) return { status: 'running', count: '...' }
  return { status: 'idle', count: '--' }
}

function getStepCount(stepIndex, stats) {
  if (!stats) return '--'
  switch (stepIndex) {
    case 0: return stats.raw_count || 0
    case 1: return stats.filtered_count || 0
    case 2: return stats.analyzed_count || 0
    case 3: return stats.verified_count || stats.analyzed_count || 0
    case 4: return stats.analyzed_count ? 1 : 0
    default: return '--'
  }
}

function progressPercent(progress) {
  if (!progress?.total_count) return 0
  return Math.min(100, Math.round((progress.processed_count / progress.total_count) * 100))
}

function progressRatio(progress) {
  if (!progress?.total_count) return '--'
  return `${progress.processed_count || 0} / ${progress.total_count}`
}

function batchRatio(progress) {
  if (!progress?.batch_total) return '--'
  return `${progress.completed_batches || 0} / ${progress.batch_total}`
}

function getFallbackProgress(data) {
  const stats = data.stats || {}
  const step = data.current_step

  switch (step) {
    case 'filter':
      return {
        ...DEFAULT_STATUS.progress,
        message: `Filtering ${stats.raw_count || 0} articles. Restart the backend to enable batch-level progress for this run.`,
        processed_count: stats.filtered_count ? stats.raw_count || 0 : 0,
        total_count: stats.raw_count || 0,
        output_count: stats.filtered_count || 0,
      }
    case 'analyst':
      return {
        ...DEFAULT_STATUS.progress,
        message: `Analyzing ${stats.filtered_count || 0} filtered events`,
        processed_count: stats.analyzed_count || 0,
        total_count: stats.filtered_count || 0,
        output_count: stats.analyzed_count || 0,
      }
    case 'verifier':
      return {
        ...DEFAULT_STATUS.progress,
        message: `Verifying ${stats.analyzed_count || 0} analyzed events`,
        processed_count: stats.verified_count || 0,
        total_count: stats.analyzed_count || 0,
        output_count: stats.verified_count || 0,
      }
    case 'synthesizer': {
      const total = stats.verified_count || stats.analyzed_count || 0
      return {
        ...DEFAULT_STATUS.progress,
        message: `Synthesizing briefing from ${total} events`,
        processed_count: total,
        total_count: total,
        output_count: total,
      }
    }
    case 'harvester':
      return {
        ...DEFAULT_STATUS.progress,
        message: 'Harvesting RSS feeds and Reddit',
      }
    default:
      return DEFAULT_STATUS.progress
  }
}

function getDisplayProgress(data) {
  if (data.progress?.message || data.progress?.total_count) return data.progress
  return getFallbackProgress(data)
}

function statusLabel(status) {
  switch (status) {
    case 'completed': return '[OK]'
    case 'error': return '[ERR]'
    case 'running': return '[RUN]'
    default: return '[IDLE]'
  }
}

function statusDotClass(status) {
  switch (status) {
    case 'completed':
    case 'done':
      return 'status-dot--green'
    case 'error':
      return 'status-dot--red'
    case 'running':
      return 'status-dot--blue'
    default:
      return 'status-dot--gray'
  }
}

export default function PipelineView() {
  const [pipelineData, setPipelineData] = useState(DEFAULT_STATUS)
  const [loading, setLoading] = useState(false)
  const [runStatus, setRunStatus] = useState('idle')
  const [backendConnected, setBackendConnected] = useState(false)

  const fetchStatus = useCallback(async () => {
    try {
      const res = await fetch('/api/pipeline/status')
      if (res.ok) {
        const data = await res.json()
        setPipelineData(data)
        setBackendConnected(true)
      } else {
        setBackendConnected(false)
      }
    } catch {
      setBackendConnected(false)
    }
  }, [])

  useEffect(() => {
    fetchStatus()
    const interval = setInterval(fetchStatus, pipelineData.status === 'running' ? 1000 : 5000)
    return () => clearInterval(interval)
  }, [fetchStatus, pipelineData.status])

  const handleRunPipeline = async (force = false, synthesizeOnly = false) => {
    setRunStatus('running')
    try {
      let url = '/api/scan'
      if (synthesizeOnly) {
        url += '?synthesize_only=true'
      } else if (force) {
        url += '?force=true'
      }
      const res = await fetch(url, { method: 'POST' })
      if (res.ok || res.status === 202) {
        setRunStatus('triggered')
        setTimeout(() => {
          setRunStatus('idle')
          fetchStatus()
        }, 5000)
      } else {
        setRunStatus('error')
        setTimeout(() => setRunStatus('idle'), 3000)
      }
    } catch {
      setRunStatus('error')
      setTimeout(() => setRunStatus('idle'), 3000)
    }
  }

  const data = pipelineData
  const progress = getDisplayProgress(data)
  const percent = progressPercent(progress)

  return (
    <div>
      {!backendConnected && (
        <div className="empty-state">
          Backend is not connected. Start the Rust backend on port 3000 to view pipeline status.
        </div>
      )}

      {/* Pipeline Flow Diagram */}
      <div className="section">
        <div className="section-title">Data Processing Pipeline</div>
        <div className="pipeline-flow">
          {PIPELINE_STEPS.map((step, idx) => {
            const stepState = getStepStatus(data.status, idx, data.stats, data.current_step)
            return (
              <div key={step.key} style={{ display: 'flex', alignItems: 'center' }}>
                <div className="pipeline-step">
                  <div className={`pipeline-step-box status-${stepState.status}`}>
                    <div className="pipeline-step-name">{step.name}</div>
                    <div className="pipeline-step-status">
                      <span className={`status-dot ${statusDotClass(stepState.status)}`} />{' '}
                      {stepState.status}
                    </div>
                    <div className="pipeline-step-count">{stepState.count}</div>
                  </div>
                </div>
                {idx < PIPELINE_STEPS.length - 1 && (
                  <div className="pipeline-arrow">
                    <div className="pipeline-arrow-line" />
                  </div>
                )}
              </div>
            )
          })}
        </div>
      </div>

      {/* Detailed Progress */}
      <div className="section">
        <div className="section-title">Detailed Progress</div>
        <div className="progress-panel">
          <div className="progress-panel-header">
            <div>
              <div className="progress-step-name">{data.current_step || 'idle'}</div>
              <div className="progress-message">{progress.message || 'Waiting for pipeline activity'}</div>
            </div>
            <div className="progress-percent">{percent}%</div>
          </div>
          <div className="progress-bar" aria-label="Pipeline progress">
            <div className="progress-bar-fill" style={{ width: `${percent}%` }} />
          </div>
          <div className="progress-grid">
            <div className="progress-metric">
              <span className="progress-metric-label">Processed</span>
              <span className="progress-metric-value">{progressRatio(progress)}</span>
            </div>
            <div className="progress-metric">
              <span className="progress-metric-label">Generated</span>
              <span className="progress-metric-value">{progress.output_count ?? 0}</span>
            </div>
            <div className="progress-metric">
              <span className="progress-metric-label">Batches</span>
              <span className="progress-metric-value">{batchRatio(progress)}</span>
            </div>
            <div className="progress-metric">
              <span className="progress-metric-label">Failed</span>
              <span className="progress-metric-value">{progress.failed_batches ?? 0}</span>
            </div>
            <div className="progress-metric progress-metric--wide">
              <span className="progress-metric-label">Last Update</span>
              <span className="progress-metric-value">{formatTimestamp(progress.updated_at)}</span>
            </div>
          </div>
          {progress.last_error && (
            <div className="progress-error">
              Last batch error: {progress.last_error}
            </div>
          )}
        </div>
      </div>

      {/* Pipeline Controls */}
      <div className="section">
        <div className="section-title">Controls</div>
        <div style={{ display: 'flex', gap: '12px', alignItems: 'center' }}>
          <button
            className="btn btn--primary"
            onClick={() => handleRunPipeline(false)}
            disabled={runStatus !== 'idle' || !backendConnected || data.status === 'running'}
          >
            {data.status === 'running' ? 'Pipeline Running' : runStatus === 'running' ? 'Running...' : runStatus === 'triggered' ? 'Loaded / Triggered' : runStatus === 'error' ? 'Failed' : 'Load Cached / Scan'}
          </button>
          <button
            className="btn"
            onClick={() => handleRunPipeline(true)}
            disabled={runStatus !== 'idle' || !backendConnected || data.status === 'running'}
          >
            Force Rescan
          </button>
          <button
            className="btn"
            onClick={() => handleRunPipeline(false, true)}
            disabled={runStatus !== 'idle' || !backendConnected || data.status === 'running'}
          >
            Re-synthesize Briefing (Synthesizer Only)
          </button>
          <button className="btn" onClick={fetchStatus} disabled={loading}>
            Refresh Status
          </button>
          <span className="text-sm text-secondary">
            Pipeline status: {data.status} | Step: {data.current_step || '--'} | Last run: {formatTimestamp(data.last_run)}
          </span>
        </div>
        {data.error_message && (
          <div className="info-box" style={{ marginTop: 8, color: '#dc2626', borderColor: '#dc2626' }}>
            Error: {data.error_message}
          </div>
        )}
      </div>

      {/* Stats */}
      <div className="section">
        <div className="section-title">Current Statistics</div>
        <div className="info-box">
          <div className="kv-row">
            <span className="kv-label">Pipeline Status</span>
            <span className="kv-value">{data.status}</span>
          </div>
          <div className="kv-row">
            <span className="kv-label">Last Run</span>
            <span className="kv-value">{formatTimestamp(data.last_run)}</span>
          </div>
          <div className="kv-row">
            <span className="kv-label">Current Step</span>
            <span className="kv-value">{data.current_step || '--'}</span>
          </div>
          <div className="kv-row">
            <span className="kv-label">Raw Articles</span>
            <span className="kv-value">{data.stats?.raw_count ?? '--'}</span>
          </div>
          <div className="kv-row">
            <span className="kv-label">After Filtering</span>
            <span className="kv-value">{data.stats?.filtered_count ?? '--'}</span>
          </div>
          <div className="kv-row">
            <span className="kv-label">Analyzed Events</span>
            <span className="kv-value">{data.stats?.analyzed_count ?? '--'}</span>
          </div>
          <div className="kv-row">
            <span className="kv-label">Verified Events</span>
            <span className="kv-value">{data.stats?.verified_count ?? '--'}</span>
          </div>
        </div>
      </div>
    </div>
  )
}
