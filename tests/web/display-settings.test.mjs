import test from 'node:test';
import assert from 'node:assert/strict';
import {createSettings} from '../../web/settings.js';
import {displayFields,displayRecord} from '../../web/display-settings.js';

const reference={sliders:{},toggles:{}};
const mirror={minimum:10,maximum:500,half:150,step:5,default:150};
const memory=items=>({getItem:k=>items[k]??null,setItem:(k,v)=>{items[k]=v;}});
// Same record as the Rust `DisplaySettings` parse test.
const DEFAULTS='{"scene_scale":0,"shadows":0,"grass":0,"sunglasses":true,"heat_haze":true,"old_heat_haze":false,"lut":"default","grayscale":false,"depth_of_field":false,"mirrors":0,"mirror_distance":150,"lamp_shadows":true,"antialiasing":1,"ambient_occlusion":0,"reflections":0}';

test('display_rows_store_check_and_reset_with_the_graphics_tab',()=>{
 const items={'the-long-slop.settings':JSON.stringify({shadows:2,scene_scale:90,lamp_shadows:false,mirror_distance:1000})};
 const settings=createSettings(()=>memory(items),reference,displayFields(mirror,{}));
 assert.equal(settings.values.shadows,2);
 assert.equal(settings.values.lamp_shadows,false);
 assert.equal(settings.values.scene_scale,0);
 assert.equal(settings.values.mirror_distance,150);
 settings.set('scene_scale',100);settings.set('lut','sepia');settings.set('lut','bad');
 assert.equal(JSON.parse(items['the-long-slop.settings']).scene_scale,100);
 assert.equal(settings.values.lut,'sepia');
 settings.reset('controls',true);
 assert.equal(settings.values.lut,'sepia');
 settings.reset('graphics',true);
 assert.equal(displayRecord(settings.values),DEFAULTS);
});
