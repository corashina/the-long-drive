// Gzip of byte arrays with the browser streams. The gzip CRC catches corrupt data on gunzip.
const transform=(data,stream)=>new Response(new Blob([data]).stream().pipeThrough(stream)).arrayBuffer().then(b=>new Uint8Array(b));
export const gzip=data=>transform(data,new CompressionStream('gzip'));
export const gunzip=data=>transform(data,new DecompressionStream('gzip'));
