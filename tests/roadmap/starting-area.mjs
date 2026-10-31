// ROADMAP "Starting area" rows.
import {pass,look} from './game.mjs';

export default {name:'Starting area',rows:{
 8:{title:'Binoculars zoom with the mask',async run(g){
  await g.sandbox('garage');await g.world();
  if((await g.game()).seated)await g.sandbox('stand');
  await g.sandbox('hand','binoculars');await g.sleep(1);
  await g.eval(()=>window.__keys.mdown(0));await g.sleep(1);
  await g.shot('starting-area-8-zoom');
  await g.eval(()=>{window.__keys.wheel(-120);});await g.sleep(0.5);await g.shot('starting-area-8-scroll');
  await g.eval(()=>window.__keys.mup(0));await g.sleep(0.5);await g.shot('starting-area-8-release');
  return look('Binoculars in the hand. Check the lens view with the two-circle mask, the scroll zoom and the release.');
 }},
 12:{title:'B many times on foot and in the Lada does not stop the game',seconds:240,async run(g){
  await g.sandbox('garage');await g.world();
  if((await g.game()).seated)await g.sandbox('stand');
  for(let i=0;i<40;i++){await g.tap('KeyB',80);await g.sleep(0.2);}
  await g.failure();await g.shot('starting-area-12-foot');
  await g.sandbox('seat','Lada','driver');
  for(let i=0;i<40;i++){await g.tap('KeyB',80);await g.sleep(0.2);}
  await g.failure();await g.shot('starting-area-12-lada');
  await g.sandbox('stand');
  return look('80 B presses, the game still runs. Check the turd in the screenshots. Knife cut: by hand.');
 }},
}};
