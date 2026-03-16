import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  // In dev mode, proxy API and WS requests to the Rust server so
  // `npm run dev` works without CORS issues.
  server: {
    proxy: {
      '/api': 'http://localhost:9000',
      '/ws': {
        target:       'ws://localhost:9000',
        ws:           true,
        changeOrigin: true,
      },
    },
  },
})
