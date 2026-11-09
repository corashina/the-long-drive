// Prints game analytics from D1. Uses the Wrangler login of `deploy.mjs`.
// `node scripts/analytics.mjs [days]`. The default is 30 days.
import {spawnSync} from 'node:child_process';
import {WRANGLER} from './deploy.mjs';

const days = Number(process.argv[2] ?? 30);
if (!(days > 0)) throw Error('Days must be a positive number.');
const since = Date.now() - days * 86400000;
const QUERIES = {
  Sessions: `SELECT session, country, COUNT(DISTINCT build) AS builds,
    ROUND(SUM(CASE WHEN type IN ('heartbeat', 'end') THEN json_extract(data, '$.playSec') ELSE 0 END) / 3600.0, 1) AS play_hours,
    ROUND(AVG(CASE WHEN type IN ('heartbeat', 'end') THEN json_extract(data, '$.fps') END), 1) AS avg_fps,
    datetime(MAX(time) / 1000, 'unixepoch') AS last_seen
    FROM events WHERE time >= ${since} GROUP BY session ORDER BY last_seen DESC LIMIT 50`,
  'Load time and GPU': `SELECT build, json_extract(data, '$.gpu') AS gpu, json_extract(data, '$.quality') AS quality,
    COUNT(*) AS loads, ROUND(AVG(json_extract(data, '$.loadMs')) / 1000, 1) AS avg_load_s
    FROM events WHERE type = 'ready' AND time >= ${since} GROUP BY build, gpu, quality ORDER BY build DESC`,
  'Start-up failures': `SELECT datetime(time / 1000, 'unixepoch') AS time, session, build, json_extract(data, '$.message') AS message
    FROM events WHERE type = 'error' AND time >= ${since} ORDER BY time DESC LIMIT 20`,
};

for (const [title, sql] of Object.entries(QUERIES)) {
  const result = spawnSync('npx', ['--yes', WRANGLER, 'd1', 'execute', 'thelongdrive-events', '--remote', '--json', '--command', sql.replace(/\s+/g, ' ')],
    {encoding: 'utf8', shell: process.platform === 'win32'});
  if (result.status !== 0) throw Error(`${title}: ${result.stderr || result.stdout}`);
  console.log(`\n${title} (last ${days} days)`);
  console.table(JSON.parse(result.stdout)[0].results);
}
