// Quality presets: one Graphics tab select that sets display rows of `ROWS`.
// High is the row defaults. Ultra adds 4x MSAA to High. Medium and Low lower the most expensive rows first.
// A row change by hand that leaves the preset values sets the preset to Custom.
import {ROWS} from './display-settings.js';
const NAMES=['shadows','lamp_shadows','ambient_occlusion','antialiasing','reflections','mirrors','grass','rain','scene_scale'];
const high={};
for(const name of NAMES)high[name]=ROWS[name].default;
export const PRESETS={
 ultra:{...high,antialiasing:0},
 high,
 medium:{...high,shadows:1,ambient_occlusion:1,lamp_shadows:false,antialiasing:1,reflections:1,mirrors:1,grass:1,rain:1},
 low:{shadows:2,lamp_shadows:false,ambient_occlusion:1,antialiasing:2,reflections:2,mirrors:2,grass:2,rain:2,scene_scale:75},
};
const CUSTOM='custom';
// Integrated GPU vendors of `GPUAdapterInfo`. Intel Arc architectures (`gen-12hp`, `xe-hpg`, `xe2-hpg`) are discrete.
const INTEGRATED=new Set(['intel','apple','qualcomm','arm','imagination']);
// The first preset of this device. `info` is the `GPUAdapterInfo`. `device` is `navigator`.
// Low on a software adapter, 4 GB memory or less, or 4 logical cores or less. Medium on an integrated GPU, else High.
// Firefox and Safari have no `deviceMemory`, so only the core count applies there.
export function firstQuality(info,device){
 if(info.isFallbackAdapter||device.deviceMemory<=4||device.hardwareConcurrency<=4)return 'low';
 if(INTEGRATED.has(info.vendor)&&!info.architecture.includes('hp'))return 'medium';
 return 'high';
}
// Settings store field (`createSettings`). Its default is `quality`, the first preset.
// The display rows take their defaults from the same preset (`displayFields`), so a Graphics reset sets that preset.
export const qualityField=quality=>({tab:'graphics',default:quality,valid:v=>v===CUSTOM||Object.hasOwn(PRESETS,v)});
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
