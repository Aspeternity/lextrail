import { resolve } from 'node:path';
import vue from '@vitejs/plugin-vue';
import { defineConfig } from 'vite';

const src = resolve(import.meta.dirname, 'src');

// Independent entries: the popup stays a separate lightweight page (D010); the settings
// window has its own (D047).
export default defineConfig({
  root: src,
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  build: {
    outDir: resolve(import.meta.dirname, 'dist'),
    emptyOutDir: true,
    target: 'es2022',
    rollupOptions: {
      input: {
        popup: resolve(src, 'popup/index.html'),
        main: resolve(src, 'main/index.html'),
        settings: resolve(src, 'settings/index.html'),
      },
    },
  },
});
