import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { HashRouter } from 'react-router-dom'
import App from './App'
import '../prototype/assets/css/tokens.css'
import '../prototype/assets/css/reset.css'
import '../prototype/assets/css/base.css'
import '../prototype/assets/css/layout.css'
import '../prototype/assets/css/components.css'
import './app.css'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <HashRouter>
      <App />
    </HashRouter>
  </StrictMode>
)
