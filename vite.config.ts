import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';

const port = process.env.VITE_PORT ? parseInt(process.env.VITE_PORT, 10) : 28420;

export default defineConfig({
  plugins: [react()],
  // 必须使用相对根路径，确保打包后在任意电脑和任何路径下运行都不会发生资源 404
  base: './',
  clearScreen: false,
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port,
    strictPort: true,
    host: '127.0.0.1',
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  build: {
    outDir: 'dist',
    assetsDir: 'assets',
    emptyOutDir: true,
    sourcemap: false,
    rollupOptions: {
      output: {
        manualChunks: undefined,
      },
    },
  },
});
