// Stores one game event from `web/analytics.js` in D1. The site is public, so anyone can post here.
const TYPES = new Set(['start', 'ready', 'heartbeat', 'end', 'error']);
const MAX_BYTES = 2048;

export async function onRequestPost({request, env}) {
  const text = await request.text();
  if (text.length > MAX_BYTES) return new Response(null, {status: 413});
  let event;
  try {
    event = JSON.parse(text);
  } catch {
    return new Response(null, {status: 400});
  }
  if (!TYPES.has(event?.type) || typeof event.session !== 'string' || event.session.length > 64 || typeof event.build !== 'string') {
    return new Response(null, {status: 400});
  }
  await env.EVENTS.prepare('INSERT INTO events (time, session, country, agent, type, build, data) VALUES (?, ?, ?, ?, ?, ?, ?)')
    .bind(Date.now(), event.session, request.cf?.country ?? null, (request.headers.get('User-Agent') ?? '').slice(0, 256),
      event.type, event.build.slice(0, 64), JSON.stringify(event.data ?? {}))
    .run();
  return new Response(null, {status: 204});
}
