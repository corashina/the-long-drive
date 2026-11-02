// "Antialiasing" display setting in browser storage. On is 4x MSAA. Off is 1 sample. Default on.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
const KEY='the-long-slop.antialiasing';
export function readAntialiasing(storage){try{return storage().getItem(KEY)!=='off';}catch{return true;}}
export function writeAntialiasing(storage,on){try{storage().setItem(KEY,on?'on':'off');}catch{}}
// Rust reads `window.gameAntialiasing()` once at startup. A change applies after a reload.
export function installAntialiasing(target,storage,input){
 const loaded=readAntialiasing(storage);
 input.checked=loaded;
 input.addEventListener('change',()=>{writeAntialiasing(storage,input.checked);});
 target.gameAntialiasing=()=>loaded;
}
