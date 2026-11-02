// ROADMAP "Settings" rows. A run starts with a fresh browser profile.
import {look,check} from './game.mjs';

const range=v=>v.length?Math.min(...v)+' to '+Math.max(...v):'none';

export default {name:'Settings',rows:{
 5:{title:'Crosshair off, old crosshair and scale',async run(g){
  await g.shot('settings-5-crosshair');
  return look('Default crosshair in the screenshot. Crosshair off, old crosshair and scale: by hand.');
 }},
 9:{title:'FPS limit off on a fresh profile, 60 holds 59 to 61, menu at 30',seconds:240,async run(g){
  const free=await g.fps(10);
  await g.pause();
  const menu=await g.fps(8);
  await g.page.click('#settings');
  const off=!(await g.page.isChecked('#setting-BEnableFpsLimit'));
  await g.page.check('#setting-BEnableFpsLimit');
  const number=g.page.locator('#setting-FFPSLimit ~ input[type=number]');
  await number.fill('60');await number.press('Enter');
  await g.page.click('#back');await g.playing();
  const limited=(await g.fps(15)).slice(3);
  // Leave the limit off for the next rows.
  await g.pause();await g.page.click('#settings');await g.page.uncheck('#setting-BEnableFpsLimit');await g.page.click('#back');await g.playing();
  const ok=off&&limited.length&&limited.every(v=>v>=59&&v<=61)&&menu.slice(3).every(v=>v>=28&&v<=32);
  return check(ok,`Limit off at start: ${off}. Free FPS ${range(free)}. Menu FPS ${range(menu.slice(3))}. Limit 60: ${range(limited)}. Repeat the 60 check on a 144 Hz display.`);
 }},
}};
