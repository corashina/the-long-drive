import test from 'node:test';
import assert from 'node:assert/strict';
import {createSettings,defaultSettings,sliderSetting} from '../../web/settings.js';

const slider=(tab,fields)=>({tab,default:1,minimum:0,maximum:10,slider:[0.1,5],whole:false,round:true,round_typed:true,round_digit:2,multiplier:1,menu_only:false,...fields});
const reference={
 sliders:{FMouseSensXY:slider('controls',{default:2}),FSound:slider('misc',{default:100,maximum:100,round_digit:0,multiplier:.01}),FDraw:slider('misc',{menu_only:true})},
 toggles:{BAutoSave:{tab:'misc',default:true,children:['BDisableAutoSaveInCar']},BDisableAutoSaveInCar:{tab:'misc',default:true,children:[]},BHelp:{tab:'misc',default:true,children:[]}},
};
const memory=items=>()=>({getItem:key=>items.get(key)??null,setItem:(key,value)=>items.set(key,value)});

test('defaults_use_the_multiplier_and_bad_stored_values_fall_back',()=>{
 assert.deepEqual(defaultSettings(reference),{FMouseSensXY:2,FSound:1,FDraw:1,BAutoSave:true,BDisableAutoSaveInCar:true,BHelp:true});
 const items=new Map([['the-long-slop.settings',JSON.stringify({FMouseSensXY:11,FSound:.5,BHelp:'no'})]]);
 const v=createSettings(memory(items),reference).values;
 assert.equal(v.FMouseSensXY,2);assert.equal(v.FSound,.5);assert.equal(v.BHelp,true);
 assert.equal(createSettings(()=>{throw Error('blocked');},reference).values.FSound,1);
});

test('a_parent_toggle_off_turns_its_children_off',()=>{
 const items=new Map(),settings=createSettings(memory(items),reference);
 settings.set('BAutoSave',false);
 assert.equal(settings.values.BDisableAutoSaveInCar,false);
 assert.equal(JSON.parse(items.get('the-long-slop.settings')).BDisableAutoSaveInCar,false);
});

test('tab_reset_skips_menu_only_sliders_in_game',()=>{
 const settings=createSettings(memory(new Map()),reference);
 settings.set('FDraw',5);settings.set('FSound',.2);settings.set('BAutoSave',false);settings.set('FMouseSensXY',3);
 settings.reset('misc',false);
 assert.equal(settings.values.FDraw,5);assert.equal(settings.values.FSound,1);
 assert.equal(settings.values.BAutoSave,true);assert.equal(settings.values.BDisableAutoSaveInCar,true);
 assert.equal(settings.values.FMouseSensXY,3);
 settings.reset('misc',true);
 assert.equal(settings.values.FDraw,1);
});

test('slider_changes_clamp_and_round_like_the_source',()=>{
 const s=reference.sliders.FSound;
 assert.equal(sliderSetting(s,55.6,true),.56);
 assert.equal(sliderSetting(s,150,false),1);
 assert.equal(sliderSetting({...s,round:false,round_typed:false},55.6,true),.556);
});

test('the_fps_limit_starts_off_because_the_browser_runs_on_the_display_refresh',()=>{
 const limited={sliders:{},toggles:{BEnableFpsLimit:{tab:'graphics',default:true,children:[]}}};
 const settings=createSettings(memory(new Map()),limited);
 assert.equal(settings.values.BEnableFpsLimit,false);
 settings.set('BEnableFpsLimit',true);settings.reset('graphics',true);
 assert.equal(settings.values.BEnableFpsLimit,false);
});
