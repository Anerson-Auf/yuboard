const path = require('path');

const root = path.resolve(__dirname, '..');

module.exports = {
  apps: [
    {
      name: 'flowboard-web',
      cwd: root,
      script: 'npm',
      args: 'run start -- --host 127.0.0.1 --port 3100',
      interpreter: 'none',
      env: {
        NODE_ENV: 'production',
      },
      max_restarts: 8,
      min_uptime: '10s',
      restart_delay: 1500,
    },
    {
      name: 'flowboard-api',
      cwd: root,
      script: './target/release/flowboard-api',
      interpreter: 'none',
      max_restarts: 8,
      min_uptime: '10s',
      restart_delay: 1500,
    },
  ],
};
