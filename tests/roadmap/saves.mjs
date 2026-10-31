// ROADMAP "Saves" rows.
import {pass,check,skip} from './game.mjs';



export default {name:'Saves',rows:{
 1:{title:'Load, Load last and Load last after a page reload',seconds:900,async run(g){
  const steps=[];
  await g.saveLoad('roadmap saves');steps.push('Save, Load last: ok');
  await g.pause();await g.page.click('#load');
  await g.page.click('.save-tile:not([hidden]) >> text=roadmap saves');
  await g.page.click('#save-load');await g.page.click('#save-load-yes');
  await g.playing();steps.push('Load: ok');
  await g.open();
  await g.page.click('#start-load-last');
  await g.playing();steps.push('Reload page, Load last: ok');
  return pass(steps.join('. ')+'. New game after death: not checked.');
 }},
 2:{title:'Load last save at start',seconds:900,async run(g){
  await g.saveLoad('roadmap auto');
  await g.pause();await g.page.click('#save');
  await g.page.check('#auto-load');
  await g.open();
  const started=await g.waitFor(()=>window.gameState.started,300,'auto load').then(()=>true,()=>false);
  if(started)await g.playing();
  // Leave the setting off for the next rows and runs.
  await g.pause().catch(()=>{});
  await g.page.click('#save').catch(()=>{});await g.page.uncheck('#auto-load').catch(()=>{});
  await g.page.click('#save-back').catch(()=>{});await g.playing().catch(()=>{});
  return check(started,started?'The last save loaded after the reload without a click.':'No game started within 300 s of the reload.');
 }},
}};
