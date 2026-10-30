// Driving settings from browser storage, sent to Rust as `DrivingSettings` JSON.
// This file is the one owner of the settings and their storage key.
// `storage` returns the Storage. The browser throws on access when storage is blocked.
import {PADS,AXES,BUTTONS} from './gamepad.js';
const KEY='the-long-slop.driving-settings';
const AXIS_FIELDS=['bound','enabled','invert','middle','dead','sens','max'];
// True when `value` is an object with exactly these fields, like Rust `deny_unknown_fields`.
const shaped=(value,fields)=>value!==null&&typeof value==='object'&&!Array.isArray(value)&&Object.keys(value).length===fields.length&&fields.every(f=>Object.hasOwn(value,f));
const index=(value,limit)=>Number.isInteger(value)&&value>=0&&value<limit;
const within=(value,range)=>Number.isFinite(value)&&value>=range.minimum&&value<=range.maximum;
// `driving` is the exported `driving` record.
export function defaultSettings(driving){
 const mouse=driving.mouse_drive,wheel=driving.steering_wheel;
 return {mouse:[mouse.sensitivity_x.default,mouse.sensitivity_y.default],h_shifter:wheel.h_shifter,
  axes:wheel.axes.map(a=>({bound:null,enabled:a.enabled,invert:a.invert,middle:a.middle?.default??0,dead:a.dead.default,sens:a.sens.default,max:a.max.default})),
  buttons:Array(11).fill(null)};
}
// The Rust `DrivingSettings::valid` rules and the JSON shape.
export function validSettings(value,driving){
 const mouse=driving.mouse_drive,axes=driving.steering_wheel.axes;
 return shaped(value,['mouse','h_shifter','axes','buttons'])
  &&Array.isArray(value.mouse)&&value.mouse.length===2&&within(value.mouse[0],mouse.sensitivity_x)&&within(value.mouse[1],mouse.sensitivity_y)
  &&typeof value.h_shifter==='boolean'
  &&Array.isArray(value.axes)&&value.axes.length===axes.length&&value.axes.every((a,i)=>shaped(a,AXIS_FIELDS)
   &&(a.bound===null||shaped(a.bound,['pad','axis'])&&index(a.bound.pad,PADS)&&index(a.bound.axis,AXES))
   &&typeof a.enabled==='boolean'&&typeof a.invert==='boolean'
   &&(axes[i].middle?within(a.middle,axes[i].middle):a.middle===0)
   &&within(a.dead,axes[i].dead)&&within(a.sens,axes[i].sens)&&within(a.max,axes[i].max))
  &&Array.isArray(value.buttons)&&value.buttons.length===11&&value.buttons.every(b=>b===null||shaped(b,['pad','button'])&&index(b.pad,PADS)&&index(b.button,BUTTONS));
}
// `inputs` and `outputs` are the mouse X and Y sliders.
export function installDrivingSettings(storage,inputs,outputs,driving){
 const ranges=[driving.mouse_drive.sensitivity_x,driving.mouse_drive.sensitivity_y];
 let stored;try{stored=JSON.parse(storage().getItem(KEY));}catch{}
 const settings=validSettings(stored,driving)?stored:defaultSettings(driving);
 let pending=true;
 // Call after each change to `settings`.
 const changed=()=>{pending=true;try{storage().setItem(KEY,JSON.stringify(settings));}catch{}};
 const shows=ranges.map((range,i)=>{
  const input=inputs[i],show=()=>{input.value=String(settings.mouse[i]);outputs[i].textContent=settings.mouse[i].toFixed(2);};
  Object.assign(input,{min:String(range.minimum),max:String(range.maximum),step:'any',value:String(settings.mouse[i])});show();
  input.addEventListener('input',()=>{settings.mouse[i]=Math.min(range.maximum,Math.max(range.minimum,Number(input.value)));show();changed();});
  return show;
 });
 // Source `tabControls` reset of `FMouseDriveSensX` and `FMouseDriveSensY`.
 const resetMouse=()=>{settings.mouse=ranges.map(range=>range.default);shows.forEach(show=>show());changed();};
 // Rust takes the settings at start and after each change.
 return {settings,changed,resetMouse,take(){if(!pending)return undefined;pending=false;return JSON.stringify(settings);}};
}
