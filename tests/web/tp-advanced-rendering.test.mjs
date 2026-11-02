// Roadmap test pass rows of the Advanced rendering section.
import test from 'node:test';
import assert from 'node:assert/strict';
import {installAntialiasing} from '../../web/antialiasing.js';
import {installMirrors} from '../../web/mirrors.js';
import {installDepthOfField} from '../../web/depth-of-field.js';
import {NO_MANIFEST,reference} from './reference.mjs';

const memory=items=>()=>({getItem:key=>items.get(key)??null,setItem:(key,value)=>items.set(key,value)});
// A settings control: `fire` runs its listeners.
function control(fields){const listeners=[];return {...fields,addEventListener:(_,f)=>listeners.push(f),fire(){for(const f of listeners)f();}};}

// The setting applies after a reload: Rust reads `gameAntialiasing` once. Off drops the TAA history
// (view_frame `antialiasing.then_some(taa)`), so the chain has no TAA pass.
test('tp_ar6_antialiasing_starts_on_and_off_applies_after_a_reload',()=>{
 const items=new Map(),target={},input=control({checked:false});
 installAntialiasing(target,memory(items),input);
 assert.equal(input.checked,true);assert.equal(target.gameAntialiasing(),true);
 input.checked=false;input.fire();
 assert.equal(target.gameAntialiasing(),true);
 const reloaded={},again=control({checked:true});
 installAntialiasing(reloaded,memory(items),again);
 assert.equal(again.checked,false);assert.equal(reloaded.gameAntialiasing(),false);
});

// Source `mainscript.MirrorsUpdate`: `BMirrors` off turns every mirror off. Mirrors past `maxDist` (4 m)
// get `InverseLerp(maxDist, minDist, d)` 0 and turn off. The 5 nearest mirrors render.
test('tp_ar18_ar19_mirrors_off_and_the_draw_distance_reach_rust_and_stay_after_a_reload',{skip:NO_MANIFEST},()=>{
 const globals=reference().world.mirrors;
 assert.deepEqual([globals.min_distance,globals.max_distance,globals.min_fov,globals.max_fov],[0,4,10,55]);
 const range=globals.draw_distance,items=new Map(),out=[0,0];
 const install=()=>{const target={},toggle=control({checked:false}),slider=control({value:'0'}),output={};const mirrors=installMirrors(target,memory(items),toggle,slider,output);mirrors.range(range);return {target,toggle,slider,output};};
 let page=install();
 assert.equal(page.target.gameMirrors(out),true);assert.deepEqual(out,[1,range.default]);
 assert.equal(page.target.gameMirrors(out),false);
 page.slider.value='750';page.slider.fire();
 page.toggle.checked=false;page.toggle.fire();
 assert.equal(page.target.gameMirrors(out),true);
 const distance=out[1];
 assert.ok(distance>range.half&&distance<range.maximum,String(distance));
 assert.deepEqual(out,[0,distance]);
 page=install();
 assert.equal(page.toggle.checked,false);
 assert.equal(page.target.gameMirrors(out),true);assert.deepEqual(out,[0,distance]);
 assert.equal(page.output.textContent,distance+' m');
});

// Source `ToggleDepthOfField.def` is off.
test('tp_ar21_depth_of_field_starts_off_and_stays_on_after_a_reload',()=>{
 const items=new Map(),target={},input=control({checked:true});
 installDepthOfField(target,memory(items),input);
 assert.equal(input.checked,false);assert.equal(target.gameDepthOfField(),false);assert.equal(target.gameDepthOfField(),undefined);
 input.checked=true;input.fire();assert.equal(target.gameDepthOfField(),true);
 const reloaded={},again=control({checked:false});
 installDepthOfField(reloaded,memory(items),again);
 assert.equal(again.checked,true);assert.equal(reloaded.gameDepthOfField(),true);
});
