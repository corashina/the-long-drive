// "Motion blur" display setting in browser storage. Off skips the blur pass. Default on.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
const KEY='the-long-slop.motion-blur';
export function readMotionBlur(storage){try{return storage().getItem(KEY)!=='off';}catch{return true;}}
export function writeMotionBlur(storage,on){try{storage().setItem(KEY,on?'on':'off');}catch{}}
// Rust takes the setting with `window.gameMotionBlur()` at start and after each change.
export function installMotionBlur(target,storage,input){
 input.checked=readMotionBlur(storage);
 let pending=true;
 input.addEventListener('change',()=>{writeMotionBlur(storage,input.checked);pending=true;});
 target.gameMotionBlur=()=>{if(!pending)return undefined;pending=false;return input.checked;};
}
