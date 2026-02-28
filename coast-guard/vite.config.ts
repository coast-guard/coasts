import { createLogger, defineConfig } from 'vite';
import react from '@vitejs/plugin-react-swc';
import tailwindcss from '@tailwindcss/vite';

const logger = createLogger();
const _origError = logger.error.bind(logger);
logger.error = (msg, options) => {
  if (typeof msg === 'string' && msg.includes('ws proxy socket error')) return;
  _origError(msg, options);
};

export default defineConfig({
  customLogger: logger,
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://localhost:31415',
        changeOrigin: true,
        ws: true,
        configure: (proxy) => {
          proxy.on('error', () => {});
          proxy.on('proxyReqWs', (_proxyReq, _req, socket) => {
            socket.on('error', () => {});
          });
        },
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
});
