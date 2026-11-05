// Stores one game event from `web/analytics.js` in D1. Access lets only signed-in players reach this path.
const TYPES = new Set(['start', 'ready', 'heartbeat', 'end', 'error']);
const MAX_BYTES = 2048;

// Access signs the JWT and checks it before the request gets here, so the payload is trusted.
function email(request) {
  const payload = request.headers.get('Cf-Access-Jwt-Assertion')?.split('.')[1];
  if (!payload) return null;
  try {
    return JSON.parse(atob(payload.replace(/-/g, '+').replace(/_/g, '/'))).email ?? null;
  } catch {
    return null;
  }
}

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
  await env.EVENTS.prepare('INSERT INTO events (time, session, email, country, agent, type, build, data) VALUES (?, ?, ?, ?, ?, ?, ?, ?)')
    .bind(Date.now(), event.session, email(request), request.cf?.country ?? null, (request.headers.get('User-Agent') ?? '').slice(0, 256),
      event.type, event.build.slice(0, 64), JSON.stringify(event.data ?? {}))
    .run();
  return new Response(null, {status: 204});
}
