// "Depth of field" display setting (source `BDof`) in browser storage. Default off, like the source toggle.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
const KEY='the-long-slop.depth-of-field';
export function readDepthOfField(storage){try{return storage().getItem(KEY)==='on';}catch{return false;}}
export function writeDepthOfField(storage,on){try{storage().setItem(KEY,on?'on':'off');}catch{}}
// Rust takes the setting with `window.gameDepthOfField()` at start and after each change.
export function installDepthOfField(target,storage,input){
 input.checked=readDepthOfField(storage);
 let pending=true;
 input.addEventListener('change',()=>{writeDepthOfField(storage,input.checked);pending=true;});
 target.gameDepthOfField=()=>{if(!pending)return undefined;pending=false;return input.checked;};
}
