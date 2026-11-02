// Roadmap test pass rows of the Settings section.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createSettings,frameLimit} from '../../web/settings.js';
import {createFramePacer} from '../../web/frame-pacer.js';
import {CROSSHAIR,oldCrosshairColor} from '../../web/feedback.js';
import {NO_MANIFEST,reference} from './reference.mjs';

const memory=items=>()=>({getItem:key=>items.get(key)??null,setItem:(key,value)=>items.set(key,value)});

// Source `mainscript.CrosshairColor`: E for turn, rotate, drive, get out and use; F for push, pickup,
// attach, tube and mount; MMB for tune; else the default color.
test('tp_st5_old_crosshair_tints_for_e_f_and_mmb',()=>{
 const colors={default:'d',e:'e',f:'f',mmb:'m'};
 assert.equal(oldCrosshairColor(CROSSHAIR.DEFAULT,colors),'d');
 assert.equal(oldCrosshairColor(1<<CROSSHAIR.E_SHIFT,colors),'e');
 assert.equal(oldCrosshairColor(1<<CROSSHAIR.ADJUST_SHIFT,colors),'e');
 assert.equal(oldCrosshairColor(1<<CROSSHAIR.F_SHIFT,colors),'f');
 assert.equal(oldCrosshairColor(CROSSHAIR.CAN_PICKUP,colors),'f');
 assert.equal(oldCrosshairColor(CROSSHAIR.MMB,colors),'m');
 // E wins over F, F wins over MMB, as in the source if-else order.
 assert.equal(oldCrosshairColor(1<<CROSSHAIR.E_SHIFT|1<<CROSSHAIR.F_SHIFT|CROSSHAIR.MMB,colors),'e');
 assert.equal(oldCrosshairColor(1<<CROSSHAIR.F_SHIFT|CROSSHAIR.MMB,colors),'f');
});

// Source `mainscript.Update`: a locked cursor runs at `FFPSLimit` when `BEnableFpsLimit`, else unlimited.
// An unlocked cursor (menus) runs at 30. The port starts the limit off (see settings.js TOGGLE_DEFAULTS).
test('tp_st9_fresh_profile_is_unlimited_the_limit_holds_60_and_the_menu_runs_at_30',{skip:NO_MANIFEST},()=>{
 const settings=createSettings(memory(new Map()),reference().settings);
 assert.equal(reference().settings.sliders.FFPSLimit.default,60);
 assert.equal(settings.values.BEnableFpsLimit,false);
 assert.equal(frameLimit(settings.values,true),0);
 assert.equal(frameLimit(settings.values,false),30);
 settings.set('BEnableFpsLimit',true);
 assert.equal(frameLimit(settings.values,true),60);
 assert.equal(frameLimit(settings.values,false),30);
 // 60 on a 144 Hz display starts 60 frames per second.
 const pacer=createFramePacer();let count=0;
 for(let i=0;i<1440;i++)if(pacer.due(i*1000/144,60))count++;
 assert.ok(Math.abs(count/10-60)<=0.2,`${count/10} frames per second`);
});
