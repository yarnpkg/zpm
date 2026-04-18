import tailwindcss    from '@tailwindcss/vite';
import react          from '@vitejs/plugin-react';
import {defineConfig} from 'vite';

// eslint-disable-next-line arca/no-default-export
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
