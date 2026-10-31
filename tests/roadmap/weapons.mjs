// ROADMAP "Weapons and damage" rows.
import {look,fail,check,distance} from './game.mjs';

const beers=async g=>(await g.state(20)).items.filter(i=>i.name==='beer');
// Spawns a beer and answers its id and position once it settles.
async function beer(g,forward){
 const ids=new Set((await beers(g)).map(i=>i.id));
 await g.sandbox('spawn','beer',forward);await g.sleep(3);
 return (await beers(g)).find(i=>!ids.has(i.id));
}
const gone=async(g,id)=>!(await beers(g)).some(i=>i.id===id);

export default {name:'Weapons and damage',rows:{
 1:{title:'Soft drops keep bottles whole',async run(g){
  await g.sandbox('garage');await g.world();
  const b=await beer(g,2);
  if(!b)return fail('The spawned beer is not in state().items.');
  await g.sleep(3);
  return look(`A beer dropped from 0.5 m: ${await gone(g,b.id)?'broke':'stays whole'}. Hard throws, pieces and sound: by hand.`);
 }},
 6:{title:'Each gun breaks a bottle',seconds:600,async run(g){
  await g.sandbox('garage');await g.world();
  const names=await g.sandbox('equip','?').then(()=>[],e=>e.message.replace(/^.*Names: /,'').split(', '));
  const lines=[];
  for(const name of names){
   await g.sandbox('equip',name).catch(e=>lines.push(name+': '+e.message));
   const b=await beer(g,4);if(!b){lines.push(name+': no beer');continue;}
   await g.sandbox('aim',...b.position);await g.sleep(0.5);await g.click(0);await g.sleep(2);
   await g.shot('weapons-6-'+name);
   lines.push(name+': '+(await gone(g,b.id)?'broke':'whole'));
  }
  const broke=lines.filter(l=>/broke$/.test(l)).length;
  return broke===names.length&&names.length?look(lines.join('; ')+'. Check flash, tracer and particles in the screenshots. Sounds and recoil: by ear and eye.'):fail(lines.join('; ')+'.');
 }},
}};
