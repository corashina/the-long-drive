// ROADMAP "Multiplayer" rows that one tab can check.
import {check} from './game.mjs';

export default {name:'Multiplayer',rows:{
 7:{title:'A wrong code shows its message',async run(g){
  await g.pause();await g.page.click('#multiplayer');
  await g.page.fill('#net-room','ZZZZZZ');await g.page.click('#net-join-start');
  await g.waitFor(()=>/\S/.test(document.querySelector('#net-status').textContent)&&!/Connecting|Joining/i.test(document.querySelector('#net-status').textContent),30,'join status');
  const status=await g.page.textContent('#net-status');
  await g.shot('multiplayer-7');
  await g.page.click('#multiplayer-back');await g.playing();
  return check(/not found/i.test(status),`Wrong code: "${status}". Wrong password, full room and version: two tabs by hand.`);
 }},
}};
