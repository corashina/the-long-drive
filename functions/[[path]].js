// Serves the site files over the Pages size limit from R2. `_routes.json` sends only those paths here.
export async function onRequestGet({request, env}) {
  const object = await env.LARGE_FILES.get(new URL(request.url).pathname.slice(1), {onlyIf: request.headers});
  if (!object) return env.ASSETS.fetch(request);
  const headers = new Headers({'Cache-Control': 'no-cache', ETag: object.httpEtag});
  object.writeHttpMetadata(headers);
  // R2 returns no body when the If-None-Match ETag still matches.
  return new Response(object.body ?? null, {status: object.body ? 200 : 304, headers});
}
