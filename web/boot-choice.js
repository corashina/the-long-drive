// Boot choice: Singleplayer or Multiplayer, before a world loads. A join then needs no page reload.
// `preset`: the choice of a game reload (`{mode:'single'}` or `{mode:'join',room}`), or null.
// `invite`: the room code of an invite link, or ''. `name`: the stored player name. `storeName(name)` keeps it.
export function createBootChoice({document,preset=null,invite='',name='',storeName=()=>{},now=()=>performance.now()}){
 const $=selector=>document.querySelector(selector);
 const root=$('#boot-choice'),modes=$('#boot-modes'),form=$('#boot-join'),error=$('#boot-error');
 const nameInput=$('#boot-name'),code=$('#boot-room'),password=$('#boot-password');
 let next=preset,settle=null,shownAt=0,waited=0;
 nameInput.value=preset?.room?.name??name;code.value=preset?.room?.code??invite;password.value=preset?.room?.password??'';
 function show(join,message){
  root.hidden=false;modes.hidden=join;form.hidden=!join;error.textContent=message;error.hidden=!message;
  queueMicrotask(()=>(join?(code.value?$('#boot-join-start'):code):$('#boot-single')).focus({preventScroll:true}));
 }
 function choose(value){
  if(!settle)return;
  waited+=now()-shownAt;root.hidden=true;
  const resolve=settle;settle=null;resolve(value);
 }
 $('#boot-single').addEventListener('click',()=>choose({mode:'single'}));
 $('#boot-multi').addEventListener('click',()=>show(true,''));
 $('#boot-back').addEventListener('click',()=>show(false,''));
 form.addEventListener('submit',event=>{
  event.preventDefault();storeName(nameInput.value);
  choose({mode:'join',room:{code:code.value.trim().toUpperCase(),name:nameInput.value,password:password.value}});
 });
 return {
  // Resolves with the next choice. `failure`: why the last join failed. The join form then shows it.
  pick(failure=''){
   if(next&&!failure){const value=next;next=null;return Promise.resolve(value);}
   next=null;
   return new Promise(resolve=>{settle=resolve;shownAt=now();show(Boolean(failure||invite),failure);});
  },
  // A boot failure hides the card.
  close(){settle=null;root.hidden=true;},
  // The time the card waited for the player, in ms.
  get waitedMs(){return waited;},
 };
}
