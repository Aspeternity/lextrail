import { createApp } from 'vue';
import App from './App.vue';
import { call } from './ipc';
import './style.css';

// Esc closes the popup. The window is hidden, not destroyed, so it is reused (D010).
window.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') {
    event.preventDefault();
    call('hide_popup');
  }
});

createApp(App).mount('#app');
