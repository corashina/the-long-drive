import test from 'node:test';
import assert from 'node:assert/strict';
import {gameFields,optionBits,parseSeed} from '../../web/game-mode.js';
import {defaultSettings} from '../../web/settings.js';

test('options_default_to_the_original',()=>{
 const v=defaultSettings({sliders:{},toggles:{}},gameFields(2));
 assert.deepEqual(v,{biome:0,startCar:-1,peaceful:false,survival:true,homesick:false});
 assert.equal(optionBits(v),2);
 assert.equal(gameFields(2).startCar.valid(2),false);
});
test('seed_parses_like_the_original_menu',()=>{
 assert.equal(parseSeed('123'),123);
 assert.equal(parseSeed('-5'),-5);
 assert.equal(parseSeed(''),0);
 assert.equal(parseSeed('99999999999'),2147483647);
 assert.equal(parseSeed('-abc'),-2147483648);
 assert.equal(parseSeed('abc'),2147483647);
});
