// ROADMAP "Player body and outfit" rows. Most need eyes: the rows set up the case and save screenshots.
import {look,skip} from './game.mjs';

// Wearable names of each type from the `wear` name list.
async function wearables(g){
 const names=await g.sandbox('wear','?').then(()=>[],e=>e.message.replace(/^.*Names: /,'').split(', '));
 return ['glass','hat','cap','helmet','mask','cigar'].map(t=>names.find(n=>n.toLowerCase().includes(t))).filter(Boolean);
}
async function foot(g){await g.sandbox('garage');await g.world();if((await g.game()).seated)await g.sandbox('stand');}

export default {name:'Player body and outfit',rows:{
 1:{title:'Look down standing, crouching, carrying and seated',async run(g){
  await foot(g);
  await g.eval(()=>window.__keys.look(0,300,6));await g.sleep(0.5);await g.shot('player-body-1-stand');
  const duck=await g.key(/crouch|duck/i).catch(()=>null);
  if(duck){await g.eval(c=>window.__keys.down(c),duck);await g.sleep(0.8);await g.shot('player-body-1-crouch');await g.eval(c=>window.__keys.up(c),duck);}
  await g.sandbox('hand','beer');await g.sleep(1);await g.shot('player-body-1-carry');
  await g.sandbox('seat','Lada','driver');await g.sleep(1);await g.eval(()=>window.__keys.look(0,200,6));await g.sleep(0.5);await g.shot('player-body-1-driver');
  await g.sandbox('stand');await g.sandbox('seat','Lada','passenger');await g.sleep(1);await g.eval(()=>window.__keys.look(0,200,6));await g.sleep(0.5);await g.shot('player-body-1-passenger');
  await g.sandbox('stand');await g.eval(()=>window.__keys.look(0,-400,6));
  return look('Screenshots: standing, crouch, carrying, driver, passenger. Check legs, arms and hands. Crouch: from the bound key, if found. WC and sled: by hand.');
 }},
 13:{title:'Put on each wearable type',async run(g){
  await foot(g);const names=await wearables(g),done=[];
  for(const n of names){const r=await g.sandbox('wear',n).then(r=>r.message,e=>e.message);done.push(n+': '+r);}
  await g.shot('player-body-13');
  return look(done.join('; ')+'. Tab and X take off: by hand. The cigarette comes off first.');
 }},
 15:{title:'Save and load keeps worn items',seconds:400,async run(g){
  await foot(g);const names=await wearables(g);
  for(const n of names)await g.sandbox('wear',n).catch(()=>{});
  await g.saveLoad('roadmap worn');
  await g.eval(()=>window.__keys.look(0,300,6));await g.sleep(0.5);await g.shot('player-body-15-shadow');await g.eval(()=>window.__keys.look(0,-300,6));
  return look(`Wore ${names.join(', ')}, then save and load. Check the worn shadows in the screenshot. Die drops: by hand.`);
 }},
 16:{title:'Appearance preview from the pause menu',async run(g){
  await g.pause();await g.page.click('#appearance');await g.sleep(2);await g.shot('player-body-16-appearance');
  await g.page.selectOption('#appearance-character','1');await g.sleep(1);await g.shot('player-body-16-female');
  await g.page.selectOption('#appearance-character','0');await g.page.click('#appearance-back');await g.playing();
  return look('Check the figure pose, framing, lights and colors for Male and Female. Start menu and reload: by hand.');
 }},
 20:{title:'Each lens color tints the view',async run(g){
  await foot(g);
  const names=await g.sandbox('wear','?').then(()=>[],e=>e.message.replace(/^.*Names: /,'').split(', '));
  const glasses=names.filter(n=>/glass/i.test(n));
  for(const n of glasses){await g.sandbox('wear',n).catch(()=>{});await g.sleep(1);await g.shot('player-body-20-'+n);}
  return glasses.length?look('Lenses: '+glasses.join(', ')+'. Compare each tint with its lens color.'):skip('No glasses in the wear names.');
 }},
}};
