// ROADMAP "Languages and help text" rows. Each row sets English back at its end.
import {pass,look,check} from './game.mjs';

async function misc(g){await g.pause();await g.page.click('#settings');await g.page.click('#settings-tab-misc');}
async function pick(g,label){await g.page.selectOption('#language',{label});await g.sleep(0.5);}
async function done(g){await g.page.click('#back');await g.playing();}

export default {name:'Languages and help text',rows:{
 2:{title:'Language list, Deutsch changes menus at once and stays after reload',seconds:900,async run(g){
  await misc(g);
  const options=await g.page.$$eval('#language option',o=>o.map(x=>x.textContent));
  const english=await g.page.textContent('#back');
  await pick(g,'Deutsch');
  const german=await g.page.textContent('#back'),tab=await g.page.textContent('#settings-tab-misc');
  await g.shot('languages-2-deutsch');
  await done(g);
  await g.open();
  const kept=await g.page.textContent('#new-game');
  await g.newGame();
  await misc(g);await pick(g,options[0]);await done(g);
  const ok=options[0]==='English'&&options.length===10&&german!==english&&kept!=='New game';
  return check(ok,`Options: ${options.join(', ')}. Back: "${english}" > "${german}", Misc tab "${tab}". After reload, New game reads "${kept}". Key list and save screen texts: see the screenshot.`);
 }},
 3:{title:'Русский, 日本語 and 中国人 show without empty boxes',async run(g){
  await misc(g);
  const options=await g.page.$$eval('#language option',o=>o.map(x=>x.textContent));
  const shots=[];
  for(const label of ['Русский','日本語','中国人']){
   if(!options.includes(label)){shots.push(label+' missing');continue;}
   await pick(g,label);await g.page.click('#settings-tab-controls');await g.shot('languages-3-'+label);shots.push(label+' ok');
  }
  await g.page.click('#settings-tab-misc');await pick(g,options[0]);await done(g);
  return look(shots.join(', ')+'. Check the screenshots for empty boxes.');
 }},
 4:{title:'In game hints use German words',async run(g){
  await misc(g);await pick(g,'Deutsch');await done(g);
  await g.sandbox('garage');await g.world();
  const seen=await g.eval(()=>window.__sweep([-10,15],18));
  await g.shot('languages-4');
  await misc(g);await g.page.selectOption('#language',{index:0});await done(g);
  return look('HUD lines seen while turning: '+seen.slice(0,12).join(' || ')+'. Check the words are German.');
 }},
}};
