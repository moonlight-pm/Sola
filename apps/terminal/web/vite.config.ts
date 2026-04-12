import { defineConfig } from 'vite';

export default defineConfig({
  build: {
    outDir: 'dist',
    lib: {
      entry: 'src/main.ts',
      formats: ['es'],
      fileName: () => 'app.js',
    },
    cssCodeSplit: false,
    rollupOptions: {
      output: {
        assetFileNames: 'app.[ext]',
      },
    },
  },
});
