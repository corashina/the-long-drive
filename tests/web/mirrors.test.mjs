import test from 'node:test';
import assert from 'node:assert/strict';
import {mirrorDistance,mirrorPosition} from '../../web/mirrors.js';

const range={minimum:10,maximum:10000,half:1000,step:10};
test('half_mapping_round_trips',()=>{
 assert.equal(mirrorDistance(0,range),10);
 assert.equal(mirrorDistance(500,range),1000);
 assert.equal(mirrorDistance(1000,range),10000);
 for(const value of [10,100,1000,5000,10000])assert.equal(mirrorDistance(mirrorPosition(value,range),range),value);
});
