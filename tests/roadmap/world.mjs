// ROADMAP "World and terrain" rows.
import {pass,fail,look,check,distance} from './game.mjs';

const item=async(g,name,near)=>(await g.state(30)).items.filter(i=>i.name===name).sort((a,b)=>distance(a.position,near)-distance(b.position,near))[0];

export default {name:'World and terrain',rows:{
 5:{title:'Sandstorm near the player, save and load inside it',seconds:500,async run(g){
  await g.sandbox('garage');await g.world();
  await g.sandbox('storm',50);
  let influence=0;const end=Date.now()+60_000;
  while(Date.now()<end&&influence<0.2){influence=(await g.state(1)).storm_influence;await g.sleep(1);}
  await g.shot('world-5-storm');
  const before=(await g.state(1)).storms.length;
  await g.saveLoad('roadmap storm');
  const s=await g.state(1);
  const evidence=`Storm influence ${influence.toFixed(2)}. Storms ${before} before save, ${s.storms.length} after load, influence ${s.storm_influence.toFixed(2)}.`;
  if(influence<0.2||!s.storms.length)return fail(evidence);
  return look(evidence+' Check sky, fog, dust in the screenshot. Sound, wind and indoor stop: by hand.');
 }},
 15:{title:'A dropped item stays after 10 km out and back, save and load',seconds:600,async run(g){
  await g.sandbox('garage');await g.world();
  const start=(await g.state(1)).player;
  await g.sandbox('teleport',start[0]+30,start[2]);await g.world();
  await g.sandbox('spawn','beer',3);await g.sleep(5);
  const here=(await g.state(1)).player,first=await item(g,'beer',here);
  if(!first)return fail('The spawned beer is not in state().items.');
  await g.sandbox('teleport',here[0]+10_000,here[2]);await g.world();
  await g.sandbox('teleport',here[0],here[2]);await g.world();await g.sleep(3);
  const back=await item(g,'beer',first.position);
  await g.saveLoad('roadmap item');
  const loaded=await item(g,'beer',first.position);
  const moved=i=>i?distance(i.position,first.position).toFixed(2)+' m':'missing';
  return check(back&&loaded&&distance(back.position,first.position)<1&&distance(loaded.position,first.position)<1,
   `Beer moved ${moved(back)} after 10 km out and back, ${moved(loaded)} after save and load.`);
 }},
 17:{title:'A far move loads without a material error',seconds:400,async run(g){
  const moved=await g.sandbox('building','Benzinkut01');
  await g.world(300);await g.sleep(10);await g.failure();
  await g.shot('world-17');
  return pass(moved.message+' The area loaded. No game failure.');
 }},
}};
