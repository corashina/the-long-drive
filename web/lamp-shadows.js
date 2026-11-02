// "Lamp shadows" display setting in browser storage. Off removes the shadows of point and spot lights. Default on.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
const KEY='the-long-slop.lamp-shadows';
export function readLampShadows(storage){try{return storage().getItem(KEY)!=='off';}catch{return true;}}
export function writeLampShadows(storage,on){try{storage().setItem(KEY,on?'on':'off');}catch{}}
// Rust takes the setting with `window.gameLampShadows()` at start and after each change.
export function installLampShadows(target,storage,input){
 input.checked=readLampShadows(storage);
 let pending=true;
 input.addEventListener('change',()=>{writeLampShadows(storage,input.checked);pending=true;});
 target.gameLampShadows=()=>{if(!pending)return undefined;pending=false;return input.checked;};
}
