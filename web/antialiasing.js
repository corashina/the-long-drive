// "Antialiasing" display setting in browser storage: 0 high (4x MSAA and TAA), 1 low (TAA only), 2 off. Default high.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
const KEY='the-long-slop.antialiasing';
// Rust reads `window.gameAntialiasing()` once at startup. A change applies after a reload.
export function installAntialiasing(target,storage,select){
 let stored=NaN;try{stored=Number(storage().getItem(KEY)??NaN);}catch{}
 const loaded=[0,1,2].includes(stored)?stored:0;
 select.value=String(loaded);
 select.addEventListener('change',()=>{try{storage().setItem(KEY,select.value);}catch{}});
 target.gameAntialiasing=()=>loaded;
}
