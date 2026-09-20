import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router-dom'
import App from './App'
import './app.css'

// Preserve reset and DNS links issued before the console moved away from hash routing.
const legacyRoute = window.location.hash.slice(1)
if (legacyRoute.startsWith('/')) window.history.replaceState(null, '', legacyRoute)

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </StrictMode>
)
