import test from 'node:test';
import assert from 'node:assert/strict';
import {createSettings} from '../../web/settings.js';
import {displayFields} from '../../web/display-settings.js';
import {PRESETS,installQualityPresets,qualityField} from '../../web/quality-presets.js';

const mirror={minimum:10,maximum:500,half:150,step:5,default:150};
const memory=items=>({getItem:k=>items[k]??null,setItem:(k,v)=>{items[k]=v;}});

test('preset_sets_rows_and_a_hand_change_sets_custom',()=>{
 const items={};
 const settings=createSettings(()=>memory(items),{sliders:{},toggles:{}},{...displayFields(mirror),quality:qualityField});
 let change;
 const select={value:'',addEventListener:(_,f)=>{change=f;}};
 installQualityPresets(settings,select);
 assert.equal(select.value,'high');
 select.value='low';change();
 for(const [name,value] of Object.entries(PRESETS.low))assert.equal(settings.values[name],value);
 assert.equal(JSON.parse(items['the-long-slop.settings']).quality,'low');
 settings.set('shadows',0);
 assert.equal(settings.values.quality,'custom');
 assert.equal(select.value,'custom');
 settings.reset('graphics',true);
 assert.equal(settings.values.quality,'high');
});
