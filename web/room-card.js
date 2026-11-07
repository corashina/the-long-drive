// The room card parts that the join loading card and the multiplayer screen share.
// `list`: `[{name,you,ping}]`. `ping`: the round trip in ms, or null when not known.
export function playerChips(document,list){
 return list.map(p=>{
  const item=document.createElement('li');item.textContent=p.you?p.name+' (you)':p.name;
  if(p.ping!=null){const ping=document.createElement('span');ping.className='ping';ping.textContent=p.ping+' ms';item.append(ping);}
  return item;
 });
}
