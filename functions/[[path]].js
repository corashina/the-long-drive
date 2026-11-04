// Serves the site files over the Pages size limit from R2. `_routes.json` sends only those paths here.
// `r2.json` of the same deployment maps each path to its R2 key, so rollbacks stay consistent.
async function find(request, env) {
  const keys = await (await env.ASSETS.fetch(new URL('/r2.json', request.url))).json();
  return keys[new URL(request.url).pathname];
}

function headers(object) {
  const result = new Headers({'Cache-Control': 'no-cache', ETag: object.httpEtag});
  object.writeHttpMetadata(result);
  result.set('Content-Length', String(object.size));
  return result;
}

export async function onRequestGet({request, env}) {
  const key = await find(request, env);
  const object = key && await env.LARGE_FILES.get(key, {onlyIf: request.headers});
  if (!object) return env.ASSETS.fetch(request);
  // R2 returns no body when the If-None-Match ETag still matches.
  return new Response(object.body ?? null, {status: object.body ? 200 : 304, headers: headers(object)});
}

export async function onRequestHead({request, env}) {
  const key = await find(request, env);
  const object = key && await env.LARGE_FILES.head(key);
  if (!object) return env.ASSETS.fetch(request);
  return new Response(null, {headers: headers(object)});
}
