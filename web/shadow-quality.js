// "Shadows" display setting in browser storage: 0 high, 1 medium, 2 low. Default high.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
const KEY='the-long-slop.shadow-quality';
export function installShadowQuality(target,storage,select){
 let stored=NaN;try{stored=Number(storage().getItem(KEY)??NaN);}catch{}
 let value=[0,1,2].includes(stored)?stored:0;
 select.value=String(value);
 select.addEventListener('change',()=>{value=Number(select.value);try{storage().setItem(KEY,String(value));}catch{}});
 // Rust reads `window.gameShadowQuality()` each frame.
 target.gameShadowQuality=()=>value;
}
