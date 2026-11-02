// "Grass" display setting in browser storage: 0 high, 1 medium, 2 low. Default high.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
const KEY='the-long-slop.grass-quality';
// Rust reads `window.gameGrassQuality()` when it builds grass. A change applies after a reload.
export function installGrassQuality(target,storage,select){
 let stored=NaN;try{stored=Number(storage().getItem(KEY)??NaN);}catch{}
 const loaded=[0,1,2].includes(stored)?stored:0;
 select.value=String(loaded);
 select.addEventListener('change',()=>{try{storage().setItem(KEY,select.value);}catch{}});
 target.gameGrassQuality=()=>loaded;
}
