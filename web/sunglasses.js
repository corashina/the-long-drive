// "Sunglasses effects" display setting in browser storage. Off removes the lens grading. Default on.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
const KEY='the-long-slop.sunglasses-effects';
export function readSunglassesEffects(storage){try{return storage().getItem(KEY)!=='off';}catch{return true;}}
export function writeSunglassesEffects(storage,on){try{storage().setItem(KEY,on?'on':'off');}catch{}}
// Rust takes the setting with `window.gameSunglassesEffects()` at start and after each change.
export function installSunglassesEffects(target,storage,input){
 input.checked=readSunglassesEffects(storage);
 let pending=true;
 input.addEventListener('change',()=>{writeSunglassesEffects(storage,input.checked);pending=true;});
 target.gameSunglassesEffects=()=>{if(!pending)return undefined;pending=false;return input.checked;};
}
