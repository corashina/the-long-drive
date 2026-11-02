// "Mirrors" and "Mirror draw distance" display settings in browser storage. Default on and the source default distance.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
const ON='the-long-slop.mirrors',DISTANCE='the-long-slop.mirror-draw-distance',POSITIONS=1000;
// Source settingsslider with useHalf: the left half covers minimum..half, the right half half..maximum.
// The value rounds to `step`.
export function mirrorDistance(position,{minimum,maximum,half,step}){
 const t=position/POSITIONS*2;
 const value=t<1?minimum+(half-minimum)*t:half+(maximum-half)*(t-1);
 return Math.min(maximum,Math.max(minimum,Math.round(value/step)*step));
}
export function mirrorPosition(value,{minimum,maximum,half}){
 const t=value<half?(value-minimum)/(half-minimum):1+(value-half)/(maximum-half);
 return Math.round(Math.min(2,Math.max(0,t))*POSITIONS/2);
}
// Rust takes the setting with `window.gameMirrors(out)`: out[0] is on (0 or 1), out[1] the distance.
// It returns false until `range` runs and while nothing changed.
export function installMirrors(target,storage,toggle,slider,output){
 try{toggle.checked=storage().getItem(ON)!=='off';}catch{toggle.checked=true;}
 let pending=false,config=null,value=0;
 const show=()=>{output.textContent=value+' m';};
 toggle.addEventListener('change',()=>{try{storage().setItem(ON,toggle.checked?'on':'off');}catch{}pending=config!==null;});
 slider.addEventListener('input',()=>{
  if(!config)return;
  value=mirrorDistance(Number(slider.value),config);show();
  try{storage().setItem(DISTANCE,String(value));}catch{}
  pending=true;
 });
 target.gameMirrors=out=>{if(!pending)return false;pending=false;out[0]=toggle.checked?1:0;out[1]=value;return true;};
 const place=()=>{slider.value=String(mirrorPosition(value,config));show();pending=true;};
 return {range(range){
  config=range;
  let stored=NaN;try{stored=Number(storage().getItem(DISTANCE)??NaN);}catch{}
  value=Number.isFinite(stored)&&stored>=range.minimum&&stored<=range.maximum?stored:range.default;
  Object.assign(slider,{min:'0',max:String(POSITIONS),step:'1'});place();
 },
 // Graphics tab reset: on and the source default distance.
 reset(){
  toggle.checked=true;try{storage().setItem(ON,'on');}catch{}
  if(!config)return;
  value=config.default;try{storage().setItem(DISTANCE,String(value));}catch{}
  place();
 }};
}
