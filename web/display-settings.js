// Display settings: rows of the settings store on the Graphics tab.
// This file is the one owner of their names, defaults and inputs. Rust `DisplaySettings` has the same fields.
// A row has `values` (a select), `range` (the mirror distance slider) or neither (a checkbox with a boolean default).
// The input of row `name` has the id `display-` and the name with `-` for `_`.
const LEVELS=[0,1,2];
export const ROWS={
 // Scene resolution percent. 0 is Auto. Then Rust steps the scale on the F3 GPU time.
 scene_scale:{values:[0,100,85,75,67,50],default:0},
 // 0 high, 1 medium, 2 low.
 shadows:{values:LEVELS,default:0},
 // 0 high, 1 medium, 2 low. Applies after a reload.
 grass:{values:LEVELS,default:0},
 sunglasses:{default:true},
 // Source settings menu defaults: heat haze on, old heat haze off, LUT Default, grayscale off.
 heat_haze:{default:true},
 old_heat_haze:{default:false},
 lut:{values:['default','c64','sepia'],default:'default'},
 grayscale:{default:false},
 // Source `ToggleDepthOfField.def` is off.
 depth_of_field:{default:false},
 // 0 high (2 mirror draws per frame), 1 low (1 mirror draw per frame), 2 off.
 mirrors:{values:LEVELS,default:0},
 // Source `FMirrorDrawDistance` in meters. The default comes from the export.
 mirror_distance:{range:true},
 motion_blur:{default:true},
 lamp_shadows:{default:true},
 // 0 high (4x MSAA and TAA), 1 low (TAA only), 2 off. Applies after a reload.
 // The default is TAA only. 4x MSAA multiplies the HDR color, depth and resolve bandwidth. The Ultra preset sets high.
 antialiasing:{values:LEVELS,default:1},
 // 0 high (4 AO scales), 1 low (the 2 finest scales, 12 fewer passes), 2 off.
 ambient_occlusion:{values:LEVELS,default:0},
 // 0 high (a probe job each frame), 1 low (a probe job every 2nd frame), 2 off.
 reflections:{values:LEVELS,default:0},
};
const NAMES=Object.keys(ROWS);
const POSITIONS=1000;
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
// Settings store fields (`createSettings`). `mirror` is the exported `world.mirrors.draw_distance` range.
// `defaults` replaces row defaults by name: the rows of the first quality preset.
export function displayFields(mirror,defaults){
 const fields={};
 for(const [name,row] of Object.entries(ROWS)){
  let valid;
  if(row.range)valid=v=>Number.isFinite(v)&&v>=mirror.minimum&&v<=mirror.maximum;
  else if(row.values)valid=v=>row.values.includes(v);
  else valid=v=>typeof v==='boolean';
  fields[name]={tab:'graphics',default:row.range?mirror.default:defaults[name]??row.default,valid};
 }
 return fields;
}
// The Rust `DisplaySettings` JSON.
export function displayRecord(values){
 const record={};
 for(const name of NAMES)record[name]=values[name];
 return JSON.stringify(record);
}
// Binds the Graphics tab inputs to the store.
// Rust takes the record with `window.gameDisplaySettings()` at start and after each change.
export function installDisplaySettings(target,settings,doc,mirror){
 const paints=[];
 for(const [name,row] of Object.entries(ROWS)){
  const input=doc.querySelector('#display-'+name.replaceAll('_','-'));
  if(row.range){
   const output=doc.querySelector('output[for="'+input.id+'"]');
   Object.assign(input,{min:'0',max:String(POSITIONS),step:'1'});
   input.addEventListener('input',()=>settings.set(name,mirrorDistance(Number(input.value),mirror)));
   paints.push(v=>{input.value=String(mirrorPosition(v[name],mirror));output.textContent=v[name]+' m';});
  }else if(row.values){
   const parse=typeof row.default==='number'?Number:String;
   input.addEventListener('change',()=>settings.set(name,parse(input.value)));
   paints.push(v=>{input.value=String(v[name]);});
  }else{
   input.addEventListener('change',()=>settings.set(name,input.checked));
   paints.push(v=>{input.checked=v[name];});
  }
 }
 let record=displayRecord(settings.values),pending=true;
 const changed=v=>{
  for(const paint of paints)paint(v);
  const next=displayRecord(v);
  if(next!==record){record=next;pending=true;}
 };
 changed(settings.values);settings.listen(changed);
 target.gameDisplaySettings=()=>{if(!pending)return undefined;pending=false;return record;};
}
