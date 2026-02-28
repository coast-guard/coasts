import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './i18n';
import App from './App';
import './index.css';
import { api } from './api/endpoints';

// Global click tracking: uses capture phase so stopPropagation in modals
// doesn't prevent tracking.
document.addEventListener('click', (e) => {
  const el = (e.target as HTMLElement).closest('button, a');
  if (!el) return;

  const label =
    el.getAttribute('aria-label') ||
    el.textContent?.trim().slice(0, 60) ||
    el.getAttribute('title') ||
    '';
  if (!label) return;

  api.track(label);
}, true);

const root = document.getElementById('root');
if (root == null) throw new Error('#root element not found');

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
