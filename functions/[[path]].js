// Serves the site files over the Pages size limit from R2. `_routes.json` sends only those paths here.
const key = request => new URL(request.url).pathname.slice(1);

function headers(object) {
  const result = new Headers({'Cache-Control': 'no-cache', ETag: object.httpEtag});
  object.writeHttpMetadata(result);
  result.set('Content-Length', String(object.size));
  return result;
}

export async function onRequestGet({request, env}) {
  const object = await env.LARGE_FILES.get(key(request), {onlyIf: request.headers});
  if (!object) return env.ASSETS.fetch(request);
  // R2 returns no body when the If-None-Match ETag still matches.
  return new Response(object.body ?? null, {status: object.body ? 200 : 304, headers: headers(object)});
}

export async function onRequestHead({request, env}) {
  const object = await env.LARGE_FILES.head(key(request));
  if (!object) return env.ASSETS.fetch(request);
  return new Response(null, {headers: headers(object)});
}
