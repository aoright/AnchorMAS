import { useState, useEffect, useCallback } from 'react'
import './index.css'
import RawDataView from './components/RawDataView'
import PipelineView from './components/PipelineView'
import BriefingView from './components/BriefingView'
import EvidenceTrackerView from './components/EvidenceTrackerView'
import AgentEvolutionCenter from './components/AgentEvolutionCenter'

const TABS = [
  { id: 'raw', label: 'Raw Data' },
  { id: 'pipeline', label: 'Pipeline' },
  { id: 'briefing', label: 'Briefing' },
  { id: 'tracker', label: 'Evidence Tracker' },
  { id: 'evolution', label: 'Agent Evolution' },
]

export default function App() {
  const [activeTab, setActiveTab] = useState('raw')
  const [articles, setArticles] = useState([])
  const [loadingArticles, setLoadingArticles] = useState(false)
  const [backendStatus, setBackendStatus] = useState('checking')
  const [scanStatus, setScanStatus] = useState('idle')

  const fetchArticles = useCallback(async () => {
    setLoadingArticles(true)
    try {
      const res = await fetch('/api/raw-articles')
      if (!res.ok) throw new Error(res.statusText)
      const data = await res.json()
      setArticles(data)
      setBackendStatus('connected')
    } catch {
      setBackendStatus('disconnected')
    } finally {
      setLoadingArticles(false)
    }
  }, [])

  const checkBackend = useCallback(async () => {
    try {
      const res = await fetch('/api/pipeline/status')
      if (res.ok) {
        setBackendStatus('connected')
      } else {
        setBackendStatus('disconnected')
      }
    } catch {
      setBackendStatus('disconnected')
    }
  }, [])

  useEffect(() => {
    fetchArticles()
    checkBackend()
  }, [fetchArticles, checkBackend])

  const handleTriggerScan = async () => {
    setScanStatus('running')
    try {
      const res = await fetch('/api/scan', { method: 'POST' })
      if (res.ok || res.status === 202) {
        setScanStatus('triggered')
        setTimeout(() => {
          setScanStatus('idle')
          fetchArticles()
        }, 3000)
      } else {
        setScanStatus('error')
        setTimeout(() => setScanStatus('idle'), 3000)
      }
    } catch {
      setScanStatus('error')
      setTimeout(() => setScanStatus('idle'), 3000)
    }
  }

  const scanButtonLabel = () => {
    switch (scanStatus) {
      case 'running': return 'Scanning...'
      case 'triggered': return 'Loaded / Triggered'
      case 'error': return 'Scan Failed'
      default: return 'Load / Scan'
    }
  }

  return (
    <div className="app-container">
      <header className="app-header">
        <h1>AIRS - Market Intelligence Agent</h1>
        <div className="app-header-actions">
          <button
            className={`btn ${scanStatus === 'idle' ? 'btn--primary' : ''}`}
            onClick={handleTriggerScan}
            disabled={scanStatus !== 'idle'}
          >
            {scanButtonLabel()}
          </button>
        </div>
      </header>

      <nav className="tab-nav">
        {TABS.map((tab) => (
          <button
            key={tab.id}
            className={activeTab === tab.id ? 'active' : ''}
            onClick={() => setActiveTab(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </nav>

      <main>
        {activeTab === 'raw' && (
          <RawDataView
            articles={articles}
            loading={loadingArticles}
            onRefresh={fetchArticles}
          />
        )}
        {activeTab === 'pipeline' && <PipelineView />}
        {activeTab === 'briefing' && <BriefingView />}
        {activeTab === 'tracker' && <EvidenceTrackerView />}
        {activeTab === 'evolution' && <AgentEvolutionCenter />}
      </main>

      <footer className="app-footer">
        <span
          className={`status-dot ${
            backendStatus === 'connected'
              ? 'status-dot--green'
              : backendStatus === 'checking'
              ? 'status-dot--amber'
              : 'status-dot--red'
          }`}
        />
        <span>
          Backend: {backendStatus === 'connected' ? 'Connected' : backendStatus === 'checking' ? 'Checking...' : 'Disconnected'}
        </span>
      </footer>
    </div>
  )
}
