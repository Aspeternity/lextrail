import { invoke } from '@tauri-apps/api/core';
import { createApp } from 'vue';
import App from './App.vue';
import './style.css';

// Esc closes the popup. The window is hidden, not destroyed, so it is reused (D010).
window.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') {
    event.preventDefault();
    void invoke('hide_popup');
  }
});

createApp(App).mount('#app');
