// The player list that the join loading card and the multiplayer screen share.
// `list`: `[{name,host,you,ping}]`. `ping`: the round trip in ms, or null when not known.
export function playerRows(document,list){
 return list.map(p=>{
  const item=document.createElement('li');item.textContent=p.name;
  if(p.host)item.className='host';
  if(p.you){const you=document.createElement('span');you.className='you';you.textContent='(you)';item.append(you);}
  if(p.ping!=null){
   const ping=document.createElement('span');
   ping.className='ping '+(p.ping<80?'good':p.ping<200?'fair':'bad');ping.textContent=p.ping+' ms';item.append(ping);
  }
  return item;
 });
}
