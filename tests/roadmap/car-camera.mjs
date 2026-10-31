// ROADMAP "Car camera and crash damage" rows. Camera names come from `gameState.camera`.
import {look,check} from './game.mjs';

const camera=async g=>(await g.game()).camera;
async function lada(g,seat='driver'){
 await g.sandbox('garage');await g.world();
 if((await g.game()).seated)await g.sandbox('stand');
 await g.sandbox('seat','Lada',seat);await g.sleep(1);
}

export default {name:'Car camera and crash damage',rows:{
 1:{title:'Tap C: outside view orbits the car',async run(g){
  await lada(g);await g.tap('KeyC');await g.sleep(1);
  const view=await camera(g);
  await g.eval(()=>window.__keys.look(400,-200,8));await g.sleep(0.5);await g.shot('car-camera-1');
  await g.tap('KeyC');await g.sleep(0.5);
  return view==='Outside'?look('Tap C: '+view+'. Check the orbit in the screenshot. Pitch limit: by hand.'):check(false,'Tap C: '+view+', expected Outside.');
 }},
 5:{title:'Tap C returns to the cockpit, also on the passenger seat',async run(g){
  const seen=[];
  for(const seat of ['driver','passenger']){
   await lada(g,seat);await g.tap('KeyC');await g.sleep(0.5);const out=await camera(g);
   await g.tap('KeyC');await g.sleep(0.5);seen.push(`${seat}: ${out} > ${await camera(g)}`);
  }
  return check(seen.every(s=>/Outside > First person$/.test(s)),seen.join('; ')+'. Sled and bike seats: by hand.');
 }},
 6:{title:'Hold C: the camera stays at its spot',async run(g){
  await lada(g);await g.tap('KeyC',600);await g.sleep(0.5);
  const held=await camera(g);await g.shot('car-camera-6');
  await g.tap('KeyC');await g.sleep(0.5);const back=await camera(g);
  return look(`Hold C: ${held}. Tap C: ${back}. Check the follow and turn by hand.`);
 }},
 7:{title:'M: map view from a seat and back',async run(g){
  await lada(g);await g.tap('KeyM');await g.sleep(1);
  const map=await camera(g);await g.shot('car-camera-7');
  await g.tap('KeyM');await g.sleep(1);const back=await camera(g);
  return check(map==='Map'&&back==='First person',`M: ${map}. M again: ${back}. Height, pan and recenter: by hand.`);
 }},
 8:{title:'Other views: E does not get out, HUD hides',async run(g){
  await lada(g);await g.tap('KeyC');await g.sleep(0.5);
  await g.tap('KeyE');await g.sleep(1);
  const s=await g.game(),hud=await g.eval(()=>document.querySelector('#hud').hidden);
  await g.tap('KeyC');await g.sleep(0.5);
  return check(s.seated&&hud,`In ${s.camera}: seated after E ${s.seated}, HUD hidden ${hud}. Worn items, sunglasses tint, sleep: by hand.`);
 }},
}};
