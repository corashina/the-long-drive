// ROADMAP "Advanced rendering" rows. They set up each case and save screenshots.
import {look} from './game.mjs';

export default {name:'Advanced rendering',rows:{
 5:{title:'No smear on the first frame after a teleport or C',async run(g){
  await g.sandbox('building','Benzinkut');await g.shot('advanced-rendering-5-teleport');
  await g.world();await g.sandbox('garage');await g.world();
  await g.sandbox('seat','Lada','driver');await g.tap('KeyC');await g.shot('advanced-rendering-5-camera');
  await g.tap('KeyC');await g.sandbox('stand');
  return look('Screenshots right after the teleport and the camera switch: no smear. Load: by hand.');
 }},
 7:{title:'Lamp shadows at night',async run(g){
  await g.sandbox('garage');await g.world();
  await g.eval(()=>window.__night());await g.sleep(3);
  await g.sandbox('seat','Lada','driver');await g.press(/light/i).catch(()=>{});await g.sandbox('stand');
  await g.eval(()=>window.__keys.look(1500,0,10));await g.sleep(1);await g.shot('advanced-rendering-7-night');
  await g.eval(()=>window.__noon());
  return look('Night at the garage with the Lada lights on. Check lamp shadows, acne and gaps. Road and house lamps: by hand.');
 }},
 17:{title:'Lada mirrors show the world behind',async run(g){
  await g.sandbox('garage');await g.world();await g.sandbox('seat','Lada','driver');await g.sleep(1);
  await g.eval(()=>window.__keys.look(-400,50,6));await g.sleep(1);await g.shot('advanced-rendering-17-left');
  await g.eval(()=>window.__keys.look(400,-100,6));await g.sleep(1);await g.shot('advanced-rendering-17-front');
  await g.sandbox('stand');
  return look('Check the mirrors show the world behind, not flipped.');
 }},
}};
