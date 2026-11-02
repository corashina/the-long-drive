// "Heat haze", "Old heat haze", "LUT" and "Grayscale" display settings in browser storage, sent to Rust as `ScreenEffects` JSON.
// Defaults follow the source settings menu: heat haze on, old heat haze off, LUT Default, grayscale off.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
const KEY='the-long-slop.screen-effects';
const LUTS=['default','c64','sepia'];
export const DEFAULT_SCREEN_EFFECTS=Object.freeze({heat_haze:true,old_heat_haze:false,lut:'default',grayscale:false});
// The Rust `ScreenEffects` JSON shape.
export function validScreenEffects(v){
 return v!==null&&typeof v==='object'&&!Array.isArray(v)&&Object.keys(v).length===4
  &&typeof v.heat_haze==='boolean'&&typeof v.old_heat_haze==='boolean'&&LUTS.includes(v.lut)&&typeof v.grayscale==='boolean';
}
export function readScreenEffects(storage){
 let stored;try{stored=JSON.parse(storage().getItem(KEY));}catch{}
 return validScreenEffects(stored)?stored:{...DEFAULT_SCREEN_EFFECTS};
}
// `inputs` holds the heat haze and old heat haze checkboxes, the LUT select and the grayscale checkbox.
// Rust takes the settings with `window.gameScreenEffects()` at start and after each change.
export function installScreenEffects(target,storage,{heatHaze,oldHeatHaze,lut,grayscale}){
 const effects=readScreenEffects(storage);
 heatHaze.checked=effects.heat_haze;oldHeatHaze.checked=effects.old_heat_haze;lut.value=effects.lut;grayscale.checked=effects.grayscale;
 let pending=true;
 const changed=()=>{
  effects.heat_haze=heatHaze.checked;effects.old_heat_haze=oldHeatHaze.checked;effects.lut=lut.value;effects.grayscale=grayscale.checked;
  try{storage().setItem(KEY,JSON.stringify(effects));}catch{}
  pending=true;
 };
 for(const input of [heatHaze,oldHeatHaze,lut,grayscale])input.addEventListener('change',changed);
 target.gameScreenEffects=()=>{if(!pending)return undefined;pending=false;return JSON.stringify(effects);};
}
