// ROADMAP "Creatures" rows. The date puts the run in the bat season.
import {pass,fail,look,skip,check,distance} from './game.mjs';

let start=null,moves=0;
// Moves the player 150 m further from the start spawn each call, so old rabbits stay behind.
async function away(g){
 start??=(await g.state(1)).player;moves++;
 await g.sandbox('teleport',start[0]+150*moves,start[2]);
 await g.world();await g.sleep(2);
}
const near=async(g,radius)=>{const s=await g.state(radius);return s.creatures.filter(c=>c.kind==='rabbit'&&distance(c.position,s.player)<=radius);};
// Spawns a rabbit and answers its id.
async function rabbit(g,name,forward){return (await g.sandbox('spawn',name,forward)).data;}
const find=async(g,id)=>(await g.state(200)).creatures.find(c=>c.kind==='rabbit'&&c.id===id);
// The states rabbit `id` shows over `seconds`, in order, without repeats.
async function states(g,id,seconds,until){
 const seen=[],end=Date.now()+seconds*1000;
 while(Date.now()<end){const r=await find(g,id);if(r&&seen.at(-1)!==r.state)seen.push(r.state);if(until?.(seen))break;await g.sleep(0.25);}
 return seen;
}
// A gun in the hand: a revolver if the export has one.
async function gun(g){
 const names=await g.sandbox('equip','?').then(()=>[],e=>e.message.replace(/^.*Names: /,'').split(', '));
 const name=names.find(n=>/revolver/i.test(n))??names[0];
 if(!name)throw Error('No gun names.');
 await g.sandbox('equip',name);
 return name;
}
async function census(g){
 const all=await near(g,150);
 return {alive:all.filter(r=>r.alive).length,dead:all.filter(r=>!r.alive).length,evil:all.filter(r=>r.evil).length,noEyes:all.filter(r=>!r.eyes).length,noEars:all.filter(r=>!r.ears).length};
}

export default {name:'Creatures',query:'date=10-20',rows:{
 1:{title:'Planes cross the sky',async run(g){
  await g.sandbox('garage');await g.world();
  let planes=[];const end=Date.now()+90_000;
  while(Date.now()<end&&!planes.length){planes=(await g.state(1)).creatures.filter(c=>c.kind==='plane');await g.sleep(1);}
  if(!planes.length)return fail('No plane within 90 s of the start.');
  const s=await g.state(1),heights=planes.map(p=>Math.round(p.position[1]-s.player[1]));
  await g.sandbox('aim',...planes[0].position);await g.sleep(1);await g.shot('creatures-1-plane');
  if(planes.length>3)return fail(planes.length+' planes, at most 3.');
  return look(`${planes.length} plane(s), heights above the player ${heights.join(', ')} m. Check the contrail in the screenshot; listen for the engine.`);
 }},
 2:{title:'Bats at gas stations in season',seconds:240,async run(g){
  const moved=await g.sandbox('building','Benzinkut');await g.world();await g.sleep(10);
  const s=await g.state(120),bats=s.creatures.filter(c=>c.kind==='bat');
  if(!bats.length){await g.shot('creatures-2-station');return look(moved.message+' No bats: 35% of stations have none. Try another station by hand.');}
  await g.sandbox('aim',...bats[0].position);await g.sleep(1);await g.shot('creatures-2-bats');
  const ok=bats.length>=5&&bats.length<=30;
  return ok?look(`${moved.message} ${bats.length} bats. Check flapping in the screenshot. Pick one up by hand: it squeaks and stays in the hand.`)
   :fail(`${moved.message} ${bats.length} bats, expected 5 to 30.`);
 }},
 3:{title:'Rabbits spawn 30 to 100 m away, at most 10',seconds:240,async run(g){
  await away(g);
  const first=new Map();let most=0;const end=Date.now()+150_000,t0=Date.now();
  while(Date.now()<end){
   const s=await g.state(300),list=s.creatures.filter(c=>c.kind==='rabbit');most=Math.max(most,list.length);
   for(const r of list)if(!first.has(r.id))first.set(r.id,{t:Math.round((Date.now()-t0)/1000),d:Math.round(distance(r.position,s.player)),size:r.size,big:r.big});
   await g.sleep(2);
  }
  const fresh=[...first.values()].filter(r=>r.t>0);
  const bad=fresh.filter(r=>r.d<25||r.d>110);
  const times=fresh.map(r=>r.t).sort((a,b)=>a-b),gaps=times.slice(1).map((t,i)=>t-times[i]);
  const evidence=`At most ${most} rabbits. New in 150 s: ${fresh.map(r=>`${r.d} m at ${r.t} s size ${r.size.toFixed(2)}${r.big?' big':''}`).join('; ')||'none'}. Gaps ${gaps.join(', ')||'-'} s.`;
  if(most>10||bad.length||!fresh.length)return fail(evidence);
  return pass(evidence+' Big ones slower: not measured.');
 }},
 4:{title:'Rabbits roam and flee from the player and shots',async run(g){
  await away(g);
  const close=await rabbit(g,'rabbit',5);
  const fromPlayer=await states(g,close,8,s=>s.includes('flee'));
  const far=await rabbit(g,'rabbit',30);
  const before=await states(g,far,6,s=>s.includes('roam'));
  const name=await gun(g);
  await g.eval(()=>window.__keys.look(0,-300,4));await g.click(0);
  const afterShot=await states(g,far,6,s=>s.includes('flee'));
  await g.shot('creatures-4');
  const evidence=`Rabbit at 5 m: ${fromPlayer.join(' > ')}. Rabbit at 30 m: ${before.join(' > ')}, after a ${name} shot: ${afterShot.join(' > ')}.`;
  return check(fromPlayer.includes('flee')&&before.includes('roam')&&afterShot.includes('flee'),evidence+' Jumps and ear twitch: check by eye.');
 }},
 5:{title:'Rabbits die from a shot, eyes or ears can go',async run(g){
  await away(g);const name=await gun(g);const tries=[];
  for(let i=0;i<4;i++){
   const id=await rabbit(g,'rabbit',4);await g.sleep(0.3);
   const r=await find(g,id);await g.sandbox('aim',r.position[0],r.position[1]+0.05*i,r.position[2]);await g.click(0);await g.sleep(2);
   const after=await find(g,id);tries.push(after);
   if(after&&!after.alive)break;
  }
  await g.shot('creatures-5');
  const dead=tries.filter(r=>r&&!r.alive);
  const evidence=`${name}: ${tries.map(r=>r?`${r.alive?'alive':'dead'} eyes ${r.eyes} ears ${r.ears}`:'gone').join('; ')}.`;
  if(!dead.length)return look(evidence+' No kill: check the aim in the screenshot.');
  return pass(evidence+' Falls and hard hits: not checked. Listen for the die sound.');
 }},
 6:{title:'Dead rabbit ragdoll carries by a bone',async run(g){
  const s=await g.state(30),dead=s.creatures.find(r=>r.kind==='rabbit'&&!r.alive);
  if(!dead)return skip('No dead rabbit near. Run row 5 first.');
  await g.sandbox('aim',...dead.position);await g.sleep(0.5);await g.tap('KeyF');await g.sleep(1);
  const carrying=(await g.game()).carrying;await g.shot('creatures-6');
  return look(`Carrying after F: ${carrying}. Check the ragdoll pose and that the body follows the bone.`);
 }},
 7:{title:'Save and load keeps live and dead rabbits and lost eyes',seconds:400,async run(g){
  const before=await census(g);
  await g.saveLoad('roadmap creatures');
  const after=await census(g);
  return check(JSON.stringify(before)===JSON.stringify(after),`Before ${JSON.stringify(before)}, after ${JSON.stringify(after)}.`);
 }},
 8:{title:'Big rabbits are evil, small ones are not',async run(g){
  await away(g);
  await rabbit(g,'evil rabbit',25);await rabbit(g,'rabbit',25);await g.sleep(1);
  const list=await near(g,300);
  const wrong=list.filter(r=>r.big!==Boolean(r.evil));
  return check(!wrong.length&&list.some(r=>r.evil),`${list.length} rabbits, ${list.filter(r=>r.big).length} big, ${list.filter(r=>r.evil).length} evil (${[...new Set(list.map(r=>r.evil).filter(Boolean))].join(', ')}). Mismatched: ${wrong.length}. Check the red eyes by eye.`);
 }},
 9:{title:'Evil rabbit roars, then attacks',async run(g){
  await away(g);
  const id=await rabbit(g,'evil rabbit',15);
  const seen=await states(g,id,30,s=>s.includes('attack'));
  await g.shot('creatures-9');
  const roar=seen.indexOf('roar'),attack=seen.indexOf('attack');
  return check(roar>=0&&attack>roar,`States: ${seen.join(' > ')}. Listen for the roar and bite sounds.`);
 }},
 10:{title:'Big evil rabbit bite hurts the player',async run(g){
  await away(g);
  const hp=(await g.game()).needs.hp;
  const id=await rabbit(g,'evil rabbit',8);
  // Stop at the first bite, before the rabbit kills the player.
  let after=hp;const seen=[],end=Date.now()+30_000;
  while(Date.now()<end&&after>=hp){const r=await find(g,id);if(r&&seen.at(-1)!==r.state)seen.push(r.state);after=(await g.game()).needs.hp;await g.sleep(0.25);}
  await away(g);
  return check(after<hp,`HP ${hp} > ${after}. States: ${seen.join(' > ')}. Damage to items and other rabbits: not checked.`);
 }},
 11:{title:'Evil rabbit growls near the player',run:async()=>skip('Sound only. Stand 4 m from an evil rabbit for 30 s and listen.')},
 12:{title:'Rabbits flee from an engine or the horn',run:async()=>skip('Needs the engine and horn keys. Check by hand.')},
 13:{title:'Rabbits react to a portable radio',run:async()=>skip('Needs the radio. Check by hand.')},
 14:{title:'Save and load keeps evil rabbits',seconds:400,async run(g){
  await away(g);await rabbit(g,'evil rabbit',30);await g.sleep(1);
  const before=await census(g);
  await g.saveLoad('roadmap evil');
  const after=await census(g);
  return check(before.evil>0&&before.evil===after.evil,`Evil before ${before.evil}, after ${after.evil}.`);
 }},
}};
