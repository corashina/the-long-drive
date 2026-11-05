import test from 'node:test';
import assert from 'node:assert/strict';
import {createSettings} from '../../web/settings.js';
import {displayFields} from '../../web/display-settings.js';
import {PRESETS,firstQuality,installQualityPresets,qualityField} from '../../web/quality-presets.js';

const mirror={minimum:10,maximum:500,half:150,step:5,default:150};
const memory=items=>({getItem:k=>items[k]??null,setItem:(k,v)=>{items[k]=v;}});
const store=(items,quality)=>createSettings(()=>memory(items),{sliders:{},toggles:{}},{...displayFields(mirror,PRESETS[quality]),quality:qualityField(quality)});
const select=()=>{const s={value:'',addEventListener:(_,f)=>{s.change=f;}};return s;};

test('preset_sets_rows_and_a_hand_change_sets_custom',()=>{
 const items={},settings=store(items,'high'),input=select();
 installQualityPresets(settings,input);
 assert.equal(input.value,'high');
 input.value='low';input.change();
 for(const [name,value] of Object.entries(PRESETS.low))assert.equal(settings.values[name],value);
 assert.equal(JSON.parse(items['the-long-slop.settings']).quality,'low');
 settings.set('shadows',0);
 assert.equal(settings.values.quality,'custom');
 assert.equal(input.value,'custom');
 settings.reset('graphics',true);
 assert.equal(settings.values.quality,'high');
});

test('first_preset_comes_from_the_device_and_a_stored_choice_wins',()=>{
 const info=(vendor,architecture,isFallbackAdapter=false)=>({vendor,architecture,isFallbackAdapter});
 const strong={deviceMemory:8,hardwareConcurrency:8};
 assert.equal(firstQuality(info('nvidia','ampere'),strong),'high');
 assert.equal(firstQuality(info('intel','gen-12lp'),strong),'medium');
 assert.equal(firstQuality(info('intel','xe-hpg'),strong),'high');
 assert.equal(firstQuality(info('apple','metal-3'),strong),'medium');
 assert.equal(firstQuality(info('',''),strong),'high');
 assert.equal(firstQuality(info('google','swiftshader',true),strong),'low');
 assert.equal(firstQuality(info('nvidia','ampere'),{deviceMemory:4,hardwareConcurrency:8}),'low');
 assert.equal(firstQuality(info('nvidia','ampere'),{deviceMemory:8,hardwareConcurrency:4}),'low');
 assert.equal(firstQuality(info('nvidia','ampere'),{hardwareConcurrency:8}),'high');
 const fresh=store({},'medium'),input=select();
 installQualityPresets(fresh,input);
 assert.equal(input.value,'medium');
 for(const [name,value] of Object.entries(PRESETS.medium))assert.equal(fresh.values[name],value);
 fresh.set('shadows',0);fresh.reset('graphics',true);
 assert.equal(fresh.values.quality,'medium');
 const stored=store({'the-long-slop.settings':JSON.stringify({...PRESETS.high,quality:'high'})},'low');
 installQualityPresets(stored,select());
 assert.equal(stored.values.quality,'high');
 assert.equal(stored.values.shadows,0);
});
