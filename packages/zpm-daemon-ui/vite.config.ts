import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig(({mode}) => {
  const daemonPort = process.env.DAEMON_PORT ?? ``;
  const daemonToken = process.env.DAEMON_TOKEN ?? ``;

  return {
    plugins: [
      react(),
      tailwindcss(),
    ],
    define: {
      __DAEMON_PORT__: JSON.stringify(daemonPort),
      __DAEMON_TOKEN__: JSON.stringify(daemonToken),
    },
  };
});
