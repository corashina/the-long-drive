// "Ambient occlusion" display setting in browser storage. Off skips the AO passes. Default on.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
const KEY='the-long-slop.ambient-occlusion';
export function readAmbientOcclusion(storage){try{return storage().getItem(KEY)!=='off';}catch{return true;}}
export function writeAmbientOcclusion(storage,on){try{storage().setItem(KEY,on?'on':'off');}catch{}}
// Rust takes the setting with `window.gameAmbientOcclusion()` at start and after each change.
export function installAmbientOcclusion(target,storage,input){
 input.checked=readAmbientOcclusion(storage);
 let pending=true;
 input.addEventListener('change',()=>{writeAmbientOcclusion(storage,input.checked);pending=true;});
 target.gameAmbientOcclusion=()=>{if(!pending)return undefined;pending=false;return input.checked;};
}
