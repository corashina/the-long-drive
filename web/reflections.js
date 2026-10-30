// "Reflections" display setting in browser storage. Off turns the live reflection probes off. Default on.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
const KEY='the-long-slop.reflections';
export function readReflections(storage){try{return storage().getItem(KEY)!=='off';}catch{return true;}}
export function writeReflections(storage,on){try{storage().setItem(KEY,on?'on':'off');}catch{}}
// Rust takes the setting with `window.gameReflections()` at start and after each change.
export function installReflections(target,storage,input){
 input.checked=readReflections(storage);
 let pending=true;
 input.addEventListener('change',()=>{writeReflections(storage,input.checked);pending=true;});
 target.gameReflections=()=>{if(!pending)return undefined;pending=false;return input.checked;};
}
