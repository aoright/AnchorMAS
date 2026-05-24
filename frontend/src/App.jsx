import { useState, useEffect, useCallback } from 'react'
import './index.css'
import RawDataView from './components/RawDataView'
import PipelineView from './components/PipelineView'
import BriefingView from './components/BriefingView'
import EvidenceTrackerView from './components/EvidenceTrackerView'
import AgentEvolutionCenter from './components/AgentEvolutionCenter'
import AgentParliament from './components/AgentParliament'

const TABS = [
  { id: 'raw', label: 'Raw Data' },
  { id: 'pipeline', label: 'Pipeline' },
  { id: 'briefing', label: 'Briefing' },
  { id: 'tracker', label: 'Evidence Tracker' },
  { id: 'evolution', label: 'Agent Evolution' },
  { id: 'parliament', label: 'Agent Parliament' },
]

export default function App() {
  const [activeTab, setActiveTab] = useState('raw')
  const [articles, setArticles] = useState([])
  const [loadingArticles, setLoadingArticles] = useState(false)
  const [backendStatus, setBackendStatus] = useState('checking')
  const [loadStatus, setLoadStatus] = useState('idle')

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

  const handleLoadData = async () => {
    setLoadStatus('loading')
    try {
      await Promise.all([fetchArticles(), checkBackend()])
      setLoadStatus('loaded')
      setTimeout(() => setLoadStatus('idle'), 1500)
    } catch {
      setLoadStatus('error')
      setTimeout(() => setLoadStatus('idle'), 2000)
    }
  }

  const loadButtonLabel = () => {
    switch (loadStatus) {
      case 'loading': return 'Loading...'
      case 'loaded': return 'Loaded'
      case 'error': return 'Load Failed'
      default: return 'Load Data'
    }
  }

  return (
    <div className="app-container">
      <header className="app-header">
        <h1>AIRS - Market Intelligence Agent</h1>
        <div className="app-header-actions">
          <button
            className={`btn ${loadStatus === 'idle' ? 'btn--primary' : ''}`}
            onClick={handleLoadData}
            disabled={loadStatus !== 'idle'}
          >
            {loadButtonLabel()}
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
        {activeTab === 'parliament' && <AgentParliament />}
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
