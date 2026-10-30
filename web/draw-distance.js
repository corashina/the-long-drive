// `storage` returns the Storage. The browser throws on access when storage is blocked.
const KEY='the-long-slop.draw-distance',STEP=100;
export function installDrawDistance(storage,input,output,{minimum,maximum,fallback}){
 const snap=value=>Math.min(maximum,minimum+Math.round((value-minimum)/STEP)*STEP);
 let stored=NaN;try{stored=Number(storage().getItem(KEY)??NaN);}catch{}
 let value=snap(Number.isFinite(stored)&&stored>=minimum&&stored<=maximum?stored:fallback);
 const show=()=>{output.textContent=value+' m';};
 Object.assign(input,{min:String(minimum),max:String(maximum),step:String(STEP),value:String(value)});show();
 const set=next=>{value=snap(next);input.value=String(value);show();};
 input.addEventListener('input',()=>set(Number(input.value)));
 // Source `onlyFromMainMenu`: reset changes the value only while the slider is unlocked.
 // Store only a value a world accepted, so a refused value never comes back after a reload.
 return {value:()=>value,lock(locked){input.disabled=locked;},reset(){if(!input.disabled)set(fallback);},store(){try{storage().setItem(KEY,String(value));}catch{}},clear(){try{storage().removeItem(KEY);}catch{}}};
}
