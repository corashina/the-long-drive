// Serves the R2 files of the site. `_routes.json` sends only those paths here.
// `r2.json` of the same deployment maps each path to its R2 keys, so rollbacks stay consistent.
// `br` is the Brotli file. `raw` is set only for files over the Pages size limit. Other raw files are site files.
// Each path has a content hash (`deploy.mjs` checks this), so browsers cache the files as immutable.
async function find(request, env) {
  const keys = await (await env.ASSETS.fetch(new URL('/r2.json', request.url))).json();
  return keys[new URL(request.url).pathname];
}

// True when the client lists `br` without q=0. Cloudflare rewrites Accept-Encoding, so read the client value.
const acceptsBrotli = request => (request.cf?.clientAcceptEncoding ?? request.headers.get('Accept-Encoding') ?? '').split(',').some(part => {
  const [name, ...parameters] = part.split(';').map(value => value.trim().toLowerCase());
  return name === 'br' && !parameters.some(parameter => /^q=0(\.0*)?$/.test(parameter));
});

function headers(object, encoded) {
  const result = new Headers({'Cache-Control': 'public, max-age=31536000, immutable', ETag: object.httpEtag, Vary: 'Accept-Encoding'});
  object.writeHttpMetadata(result);
  result.set('Content-Length', String(object.size));
  if (encoded) result.set('Content-Encoding', 'br');
  return result;
}

async function serve(request, env, read) {
  const entry = await find(request, env);
  const encoded = Boolean(entry?.br) && acceptsBrotli(request);
  const key = encoded ? entry.br : entry?.raw;
  const object = key && await read(key);
  if (!object) {
    const asset = await env.ASSETS.fetch(request);
    const response = new Response(asset.body, asset);
    response.headers.append('Vary', 'Accept-Encoding');
    return response;
  }
  // R2 returns no body when the If-None-Match ETag still matches.
  const body = request.method === 'GET' ? object.body ?? null : null;
  const status = request.method === 'GET' && !object.body ? 304 : 200;
  // `manual` sends the stored Brotli bytes as they are. Cloudflare does not compress them again.
  return new Response(body, {status, headers: headers(object, encoded), encodeBody: encoded ? 'manual' : 'automatic'});
}

export const onRequestGet = ({request, env}) => serve(request, env, key => env.LARGE_FILES.get(key, {onlyIf: request.headers}));

export const onRequestHead = ({request, env}) => serve(request, env, key => env.LARGE_FILES.head(key));
