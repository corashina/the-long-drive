// Game options, as `gamemodesettingsscript`. Defaults are its `defS`. They are global, not per save.
// The settings store keeps them. `startCars` is the count of manifest start car fields.
const INT_MIN=-2147483648,INT_MAX=2147483647;
export const gameFields=startCars=>({
 biome:{default:0,valid:v=>Number.isInteger(v)&&v>=0&&v<=3},
 // -1 is Random. Other values index the manifest start car fields.
 startCar:{default:-1,valid:v=>Number.isInteger(v)&&v>=-1&&v<startCars},
 peaceful:{default:false,valid:v=>typeof v==='boolean'},
 survival:{default:true,valid:v=>typeof v==='boolean'},
 homesick:{default:false,valid:v=>typeof v==='boolean'},
});
// Simulation bits: 1 peaceful, 2 survival. Homesick has no world effect in the original.
export const optionBits=options=>Number(options.peaceful)|Number(options.survival)<<1;
// `mainmenuscript.chSeed`: C# int.Parse, else the int limit of the sign, else 0 for an empty field.
export function parseSeed(text){
 if(/^\s*[+-]?\d+\s*$/.test(text)){const v=Number(text);if(v>=INT_MIN&&v<=INT_MAX)return v;}
 return text.length>0?(text[0]==='-'?INT_MIN:INT_MAX):0;
}
// `chnewSeed`: Random.Range(int.MinValue, int.MaxValue).
export const randomSeed=(random=Math.random)=>Math.floor(INT_MIN+random()*(INT_MAX-INT_MIN));
export function installGameMode(target,select,settings){
 select.value=String(settings.values.biome);
 select.addEventListener('change',()=>settings.set('biome',Number(select.value)));
 target.gameBiome=()=>settings.values.biome;
}
// `toggles` maps option names to checkboxes. `changed` gets the options after each change.
export function installGameOptions(target,toggles,settings,changed=()=>{}){
 const v=settings.values;let pending=null;
 for(const [name,node] of Object.entries(toggles)){
  node.checked=v[name];
  node.addEventListener('change',()=>{settings.set(name,node.checked);pending=optionBits(v);changed(v);});
 }
 target.gameOptions=()=>{const bits=pending;pending=null;return bits??undefined;};
 return {bits:()=>optionBits(v)};
}
// The menu rolls a new seed at each start, as `mainmenuscript.Start`. `initial` is the seed of a new game reload.
export function installSeed(target,input,random=Math.random,initial=randomSeed(random)){
 let seed=initial;input.value=String(seed);
 input.addEventListener('change',()=>{seed=parseSeed(input.value);input.value=String(seed);});
 target.gameSeed=()=>seed;
 return {reroll(){seed=randomSeed(random);input.value=String(seed);}};
}
// `names` are the manifest shell start car names, in start field order.
export function installStartCar(target,select,names,settings){
 names.forEach((name,i)=>select.add(new Option(name,String(i))));
 select.value=String(settings.values.startCar);
 select.addEventListener('change',()=>settings.set('startCar',Number(select.value)));
 target.gameStartCar=()=>settings.values.startCar;
}
