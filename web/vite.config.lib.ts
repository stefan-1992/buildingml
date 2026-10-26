import { defineConfig } from 'vite'
import path from 'path'

/**
 * Vite config for building shared renderer as an IIFE bundle.
 * Used by VS Code extension webview.
 * Uses index-iife.ts which excludes ThreeJSRenderer and web-ifc
 * to avoid CSP violations in the VS Code webview sandbox.
 *
 * Usage: npx vite build --config vite.config.lib.ts
 */
export default defineConfig({
  resolve: {
    alias: {
      '@shared': path.resolve(__dirname, 'src/shared'),
    },
  },
  build: {
    lib: {
      entry: path.resolve(__dirname, 'src/shared/renderer/index-iife.ts'),
      name: 'BuildingMLRenderer',
      formats: ['iife'],
      fileName: () => 'shared-renderer.js',
    },
    outDir: path.resolve(__dirname, '../editors/vscode/media'),
    emptyOutDir: false,
    minify: true,
    sourcemap: false,
  },
})
