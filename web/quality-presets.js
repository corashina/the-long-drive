// Quality presets: one Graphics tab select that sets display rows of `ROWS`.
// High is the row defaults. Medium and Low lower the most expensive rows first.
// A row change by hand that leaves the preset values sets the preset to Custom.
import {ROWS} from './display-settings.js';
const NAMES=['shadows','lamp_shadows','ambient_occlusion','antialiasing','motion_blur','reflections','mirrors','grass','scene_scale'];
const high={};
for(const name of NAMES)high[name]=ROWS[name].default;
export const PRESETS={
 high,
 medium:{...high,shadows:1,lamp_shadows:false,antialiasing:1,grass:1},
 low:{shadows:2,lamp_shadows:false,ambient_occlusion:false,antialiasing:2,motion_blur:false,reflections:false,mirrors:false,grass:2,scene_scale:75},
};
const CUSTOM='custom';
// Settings store field (`createSettings`). Graphics reset sets High with the row defaults.
export const qualityField={tab:'graphics',default:'high',valid:v=>v===CUSTOM||Object.hasOwn(PRESETS,v)};
const matches=(values,preset)=>NAMES.every(name=>values[name]===preset[name]);
// Binds the quality select to the store.
export function installQualityPresets(settings,select){
 let applying=false;
 const apply=quality=>{
  applying=true;
  if(quality!==CUSTOM)for(const name of NAMES)settings.set(name,PRESETS[quality][name]);
  applying=false;
  settings.set('quality',quality);
 };
 const changed=v=>{
  if(!applying&&v.quality!==CUSTOM&&!matches(v,PRESETS[v.quality]))settings.set('quality',CUSTOM);
  select.value=v.quality;
 };
 select.addEventListener('change',()=>apply(select.value));
 changed(settings.values);settings.listen(changed);
}
