// `storage` and `session` return the Storage. The browser throws on access when storage is blocked.
const KEY='the-long-slop.draw-distance',PENDING='tls-draw-distance',STEP=100;
export function installDrawDistance(storage,session,input,output,{minimum,maximum,fallback}){
 const snap=value=>Math.min(maximum,minimum+Math.round((value-minimum)/STEP)*STEP);
 const valid=value=>Number.isFinite(value)&&value>=minimum&&value<=maximum;
 let stored=NaN,pending=NaN;try{stored=Number(storage().getItem(KEY)??NaN);}catch{}
 // A value applied in a game (`pend`) starts the next page load once. It is stored when the world accepts it.
 try{pending=Number(session().getItem(PENDING)??NaN);session().removeItem(PENDING);}catch{}
 let value=snap(valid(pending)?pending:valid(stored)?stored:fallback);
 const show=()=>{output.textContent=value+' m';};
 Object.assign(input,{min:String(minimum),max:String(maximum),step:String(STEP),value:String(value)});show();
 const set=next=>{value=snap(next);input.value=String(value);show();};
 input.addEventListener('input',()=>set(Number(input.value)));
 // Source `onlyFromMainMenu`: reset changes the value only while the slider is unlocked.
 // Store only a value a world accepted, so a refused value never comes back after a reload.
 return {value:()=>value,lock(locked){input.disabled=locked;},reset(){if(!input.disabled)set(fallback);},store(){try{storage().setItem(KEY,String(value));}catch{}},pend(){try{session().setItem(PENDING,String(value));}catch{}},clear(){try{storage().removeItem(KEY);}catch{}}};
}
