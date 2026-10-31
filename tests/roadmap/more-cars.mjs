// ROADMAP "More cars" rows. Keys come from Settings > Controls.
import {pass,fail,look,check,distance} from './game.mjs';

// Ignition on, then the starter for `seconds`. Answers whether the engine runs.
async function start(g,seconds=2){
 if(!(await g.game()).ignition)await g.press(/ignit/i);
 await g.sleep(0.5);await g.press(/start/i,seconds*1000);await g.sleep(1.5);
 return (await g.game()).running;
}
async function stop(g){if((await g.game()).ignition)await g.press(/ignit/i);await g.sleep(1);}
async function sit(g,car){
 if((await g.game()).seated)await g.sandbox('stand');
 await g.sandbox('seat',car,'driver');await g.sleep(1);
}
// The target text when the head aims at the car named like `name`.
async function hover(g,name){
 const s=await g.state(40),car=s.cars.filter(c=>c.name.toLowerCase().includes(name.toLowerCase())).sort((a,b)=>distance(a.position,s.player)-distance(b.position,s.player))[0];
 if(!car)return name+': not near';
 await g.sandbox('aim',car.position[0],car.position[1]+0.8,car.position[2]);await g.sleep(0.7);
 return `${car.name}: "${(await g.game()).target}"`;
}

export default {name:'More cars',rows:{
 5:{title:'New game: the Lada at its garage spot starts, save and load seated',seconds:500,async run(g){
  await g.sandbox('garage');await g.world();
  await sit(g,'Lada');
  const running=await start(g);await g.shot('more-cars-5');
  await stop(g);
  await g.saveLoad('roadmap lada');
  const after=await g.game();
  return look(`Lada engine runs: ${running}. Seated after save and load: ${after.seated}. Lights, gauges, radio, horn, wheels and trunk: by hand.`);
 }},
 16:{title:'Bus runs on diesel, not on petrol',seconds:400,async run(g){
  await g.sandbox('garage');await g.world();
  if((await g.game()).seated)await g.sandbox('stand');
  await g.sandbox('spawn','Bus02Full',12);
  const end=Date.now()+60_000;
  while(Date.now()<end&&!(await g.state(40)).cars.some(c=>c.name==='Bus02Full'))await g.sleep(1);
  await sit(g,'Bus02Full');
  await g.sandbox('fill','diesel',40);const diesel=await start(g);await stop(g);
  await g.sandbox('fill','petrol',40);const petrol=await start(g,3);await stop(g);
  await g.sandbox('stand');
  return check(diesel&&!petrol,`Bus02Full runs on diesel: ${diesel}. Runs on petrol: ${petrol}. Bus01 and Bus03: by hand.`);
 }},
 32:{title:'Hover text shows the game name',async run(g){
  await g.sandbox('garage');await g.world();
  if((await g.game()).seated)await g.sandbox('stand');
  const lines=[await hover(g,'Lada')];
  await g.shot('more-cars-32');
  const raw=lines.filter(l=>/"[^"]*(Full|Car\d|Bus\d)[^"]*"/.test(l)||/""/.test(l));
  return raw.length?fail(lines.join('; ')+'. Expected a game name.'):look(lines.join('; ')+'. Roadside cars, buses and bikes: by hand.');
 }},
}};
