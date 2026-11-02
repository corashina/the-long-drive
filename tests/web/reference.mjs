// The exported manifest for roadmap tests (`tp_*`). It is not in the repository.
// Copy it to web/assets/reference/manifest.json. Tests that need it skip without it.
import {existsSync,readFileSync} from 'node:fs';
const PATH=new URL('../../web/assets/reference/manifest.json',import.meta.url);
export const NO_MANIFEST=existsSync(PATH)?false:'no web/assets/reference/manifest.json';
let manifest;
export function reference(){return manifest??=JSON.parse(readFileSync(PATH,'utf8'));}
