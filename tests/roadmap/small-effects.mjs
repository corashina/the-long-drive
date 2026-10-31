// ROADMAP "Small effects" rows. The date puts the run in the pumpkin season.
import {look} from './game.mjs';

export default {name:'Small effects',query:'date=10-20',rows:{
 4:{title:'Start pumpkin glows at night',async run(g){
  await g.sandbox('garage');await g.world();
  await g.eval(()=>window.__night());await g.sleep(3);
  const seen=await g.eval(()=>window.__sweep([10,30],18));
  const pumpkin=seen.find(s=>/pumpkin|tök/i.test(s));
  await g.shot('small-effects-4');
  await g.eval(()=>window.__noon());
  return look(`Night, date 10-20. Pumpkin target: ${pumpkin??'not found in a sweep'}. Check the glow and flicker; hold and wear it by hand.`);
 }},
}};
