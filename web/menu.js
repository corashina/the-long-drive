import {createSaveScreen} from './save-screen.js';
import {createSettingsScreen} from './settings-screen.js';
import {createControlsScreen} from './controls-screen.js';
import {canLoadLast} from './save-session.js';
import {AUTOSAVE} from './save-storage.js';
export const ACTIONS={PAUSE:2,CONTINUE:4,RESPAWN:8,RECOVER:16};
const pendingText={save:'Saving…',load:'Loading…',loadLast:'Loading…',remove:'Deleting…',newGame:'Starting a new game…'};
// Result texts confirm a menu action. They clear when play continues.
const resultText={join:'Joined the game.',save:'Save complete.',load:'Game loaded.',loadLast:'Game loaded.',remove:'Save deleted.',newGame:''};
const SUBSCREENS=['settings','game-mode','saves','appearance','multiplayer'];
// The start menus show the live road view of the simulation (no art). Appearance has its own preview.
const ROAD_STATES=['start-menu','settings','game-mode','saves','multiplayer'];
function operationError(error){
 const message=error?.message??String(error);
 if(message==='No saved game.')return 'No saved game is available.';
 if(message.startsWith('Incompatible save:'))return 'This save uses a different schema or source export.';
 return message;
}
export class MenuState {
 constructor(){this.state='loading';this.readyFlag=false;this.started=false;this.actions=0;this.error='';this.status='';this.generation=0;this.pending=false;this.operation='';this.origin='start-menu';this.dead=false;this.deathComplete=false;this.deathHint='';}
 ready(){if(this.state==='error')return;this.readyFlag=true;this.state=this.started?'pause-menu':'start-menu';}
 loading(){if(this.state==='error')return;if(!this.operation)this.generation++;this.pending=Boolean(this.operation);this.readyFlag=false;this.actions=0;this.state='loading';this.error='';this.status=this.operation?pendingText[this.operation]:'';}
 fail(message){this.generation++;this.pending=false;this.operation='';this.readyFlag=false;this.state='error';this.error=message;this.status='';this.actions=this.started?ACTIONS.PAUSE:0;}
 pause(saveSession){if(this.operation&&this.operation!=='save'&&saveSession?.cancel()){this.generation++;this.pending=false;this.operation='';this.status='';this.error='';}if(!this.operation)this.generation++;this.pending=Boolean(this.operation);this.actions=this.started?ACTIONS.PAUSE:0;if(this.state==='error')return;if(SUBSCREENS.includes(this.state)){this.origin=this.started?'pause-menu':'start-menu';return;}this.state=this.readyFlag?(this.started?'pause-menu':'start-menu'):'loading';}
 settings(){if(this.pending)return;if(this.state!=='start-menu' && this.state!=='pause-menu')return;this.origin=this.state;this.state='settings';}
 gameMode(){if(this.pending||!['start-menu','pause-menu'].includes(this.state))return;this.origin=this.state;this.state='game-mode';}
 saves(){if(this.pending||!['start-menu','pause-menu'].includes(this.state))return;this.origin=this.state;this.state='saves';}
 appearance(){if(this.pending||!['start-menu','pause-menu'].includes(this.state))return;this.origin=this.state;this.state='appearance';}
 multiplayer(){if(this.pending||!['start-menu','pause-menu'].includes(this.state))return;this.origin=this.state;this.state='multiplayer';}
 back(){if(SUBSCREENS.includes(this.state))this.state=this.origin;}
 recover(){if(this.state==='pause-menu'&&!this.pending&&!this.dead)this.actions=ACTIONS.RECOVER;}
 // Multiplayer: the dead player lives again. The next frame of the simulation confirms it.
 respawn(){if(this.state!=='pause-menu'||this.pending||!this.dead)return false;this.dead=false;this.deathComplete=false;return true;}
 takeActions(){const a=this.actions;this.actions=0;return a;}
 async run(operation,saveSession,...args){
  if(!this.readyFlag||this.pending||!['start-menu','pause-menu','saves','multiplayer'].includes(this.state)||(operation==='save'&&!this.started))return;
  const token=++this.generation;this.pending=true;this.operation=operation;this.error='';this.status=pendingText[operation];
  try {
   await saveSession[operation](...args);if(token!==this.generation)return;
   if(operation!=='save'&&operation!=='remove'){this.started=true;this.state=this.readyFlag?'pause-menu':'loading';this.dead=false;this.deathComplete=false;}
   this.status=resultText[operation];return true;
  } catch(error){if(token===this.generation){this.error=operationError(error);this.status='';}}
  finally {if(token===this.generation){this.pending=false;this.operation='';}}
 }
 async capture(action,request,unlock){
  if(this.deathComplete || !this.readyFlag || !this.started || this.pending || this.state!=='pause-menu')return false;
  const token=++this.generation;this.pending=true;this.error='';
  try {try{unlock?.();}catch{}await request();if(token!==this.generation)return false;this.actions=action;this.state='playing';this.status='';return true;}
  catch(error){if(token===this.generation){this.error='Mouse capture failed. '+error.message+' Select Continue to try again.';this.state='pause-menu';}return false;}
  finally {if(token===this.generation)this.pending=false;}
 }
}
export async function captureWithAudio(model,action,request,audio){const success=await model.capture(action,request,()=>audio?.unlock());if(!success)audio?.pause();return success;}
export function createMenu({canvas,language,ui,clearInput,queueAction,saveSession,buildId,thumbnail=()=>null,sleeping=()=>false,seated=()=>false,guest=()=>false,online=()=>false,worldDrawDistance=()=>NaN,slider,audio,bindings,defaultAssists,onPause,onNewGame,reloadNewGame=()=>false,autoNewGame=false,appearance,settings,resets}) {
 const model=new MenuState(),menu=document.querySelector('#menu'),settingsNode=document.querySelector('#settings-screen'),gameMode=document.querySelector('#game-mode-screen'),savesNode=document.querySelector('#save-screen'),appearanceNode=document.querySelector('#appearance-screen'),multiplayerNode=document.querySelector('#multiplayer-screen'),startLast=document.querySelector('#start-load-last');
 const initial=document.querySelector('#start-actions'),paused=document.querySelector('#pause-actions'),error=document.querySelector('#capture-error'),status=document.querySelector('#status'),distance=document.querySelector('#session-distance'),distanceLabel=document.querySelector('#session-distance-label'),distanceValue=document.querySelector('#session-distance-value'),deathCard=document.querySelector('#death-card'),deathHint=document.querySelector('#death-hint');
 const art=document.querySelector('#menu-art'),frame=document.querySelector('#menu-frame'),hud=document.querySelector('#hud');
 // The menus have their own layout (style.css), not the source sprites.
 const initialNodes=['#new-game','#start-load','#start-load-last','#start-settings','#start-game-mode','#start-appearance','#start-multiplayer'];
 const pauseNodes=['#pause-respawn','#resume','#save','#load','#pause-new-game','#recover','#settings','#pause-game-mode','#appearance','#multiplayer'];
 const actionNodes=[...initialNodes,...pauseNodes],backNodes=['#back','#game-mode-back','#save-back','#appearance-back','#multiplayer-back'];
 let lastState='',lastError='',lastStatus='',lastTravel=-1,pendingDriving=0,canLast=false,startResult=null;
 const screen=createSaveScreen({settings,word:language.word,paint:()=>paint(),run:(operation,name)=>operate(operation,...(operation==='save'?[name,thumbnail()]:operation==='loadLast'?[buildId]:[name]))});
 const clutch=document.querySelector('#automatic-clutch'),gears=document.querySelector('#automatic-gears');
 function drivingState(assists){clutch.checked=Boolean(assists&1);gears.checked=Boolean(assists&2)&&clutch.checked;gears.disabled=!clutch.checked||model.pending;clutch.disabled=model.pending;}
 function requestDriving(){
  if(model.pending||!['settings','pause-menu','start-menu'].includes(model.state))return;
  if(!clutch.checked)gears.checked=false;
  gears.disabled=!clutch.checked;
  pendingDriving=4|Number(clutch.checked)|(Number(gears.checked)<<1);
 }
 clutch.addEventListener('change',requestDriving);gears.addEventListener('change',requestDriving);
 // Source `tabControls` reset: the assist toggles take their defaults.
 const assistReset=()=>{clutch.checked=defaultAssists.automatic_clutch;gears.checked=defaultAssists.automatic_gears;requestDriving();};
 settings.listen(()=>{if(model.state==='saves')paint();});
 const applyDistance=document.querySelector('#draw-distance-apply');
 const canApply=()=>model.started&&model.readyFlag&&!model.pending&&!model.dead&&!online()&&model.state==='settings'&&model.origin==='pause-menu';
 const settingsScreen=createSettingsScreen(settingsNode,settings,language,()=>!model.started,{...resets,controls:[assistReset,...resets.controls??[]]});
 const controlsScreen=createControlsScreen(document.querySelector('#key-bindings'),document.querySelector('#key-bindings-reset'),settings,language,bindings);
 language.listen(()=>{lastTravel=-1;});
 function resize(){menu.classList.toggle('narrow',innerWidth<800);}
 function paint(){
  const state=model.state;menu.hidden=state==='playing';settingsNode.hidden=state!=='settings';gameMode.hidden=state!=='game-mode';savesNode.hidden=state!=='saves';appearanceNode.hidden=state!=='appearance';multiplayerNode.hidden=state!=='multiplayer';initial.hidden=!['start-menu','error'].includes(state);menu.classList.toggle('covered',state==='loading');paused.hidden=state!=='pause-menu';distance.hidden=paused.hidden;
  if(hud&&state!=='playing')hud.hidden=true;
  // Port choice: the start menu shows the road ahead, not the source figure and art.
  menu.classList.toggle('road-view',!model.started&&ROAD_STATES.includes(state));
  clutch.disabled=model.pending;gears.disabled=model.pending||!clutch.checked;
  slider.lock(model.pending||(model.started?!canApply():!(state==='start-menu'||state==='settings'&&model.origin==='start-menu')));
  applyDistance.hidden=!canApply()||slider.value()===worldDrawDistance();
  if(state==='settings')settingsScreen.paint();else controlsScreen.cancel();
  for(const selector of actionNodes)document.querySelector(selector).disabled=!model.readyFlag||model.pending;
  document.querySelector('#recover').disabled ||= model.dead;
  // After death the card above the buttons says what happened. Continue and Recover cannot help, so they go.
  for(const id of ['#resume','#recover'])document.querySelector(id).hidden=model.deathComplete;
  deathCard.hidden=!model.deathComplete;if(deathHint.textContent!==model.deathHint)deathHint.textContent=model.deathHint;
  // Source guests have no save. The host world is the one world.
  for(const id of ['#save','#load'])document.querySelector(id).hidden=guest();
  // The player already started, so the pause menu offers New game only after death.
  document.querySelector('#pause-new-game').hidden=guest()||!model.dead;
  // In a multiplayer session a dead player respawns at once, also before the death card. The world goes on.
  document.querySelector('#pause-respawn').hidden=!online()||!model.dead;
  startLast.hidden=!canLast;
  if(state==='saves')screen.paint(!model.readyFlag||model.pending);
  error.hidden=!model.error;if(lastError!==model.error){error.textContent=model.error;lastError=model.error;}
  if(lastStatus!==model.status){status.textContent=model.status;lastStatus=model.status;}
  if(state!==lastState){
   // The preview and the save follow the screen, whatever closes it.
   if(lastState==='appearance')appearance?.close();
   if(state==='appearance')appearance?.open();
   // The art picked for the loading cover stays when the menu opens.
   if(state==='loading'||(state==='start-menu'||state==='pause-menu')&&lastState!=='loading'){const pair=ui.pairUrls[Math.floor(Math.random()*ui.pairUrls.length)];art.style.backgroundImage='url("'+pair.background+'")';frame.style.backgroundImage='url("'+pair.frame+'")';}
   if(state==='start-menu'||state==='pause-menu'){
    queueMicrotask(()=>document.querySelector(state==='pause-menu'?'#resume':'#new-game').focus({preventScroll:true}));
   } else if(SUBSCREENS.includes(state))queueMicrotask(()=>document.querySelector({settings:'#back','game-mode':'#game-mode-back',saves:'#save-back',appearance:'#appearance-back',multiplayer:'#multiplayer-back'}[state]).focus({preventScroll:true}));
   lastState=state;
  }
 }
 function flush(){const a=model.takeActions();if(a)queueAction(a);}
 function pause(cancelPending=false){if(model.state==='playing')onPause?.();audio?.pause();clearInput();model.pause(cancelPending===true?saveSession:undefined);if(document.pointerLockElement===canvas)document.exitPointerLock();flush();paint();}
 async function refresh(){
  try{const list=await saveSession.list();canLast=canLoadLast(list,buildId);screen.update(list,canLast);}
  catch(error){if(model.state==='saves'&&!model.pending)model.error=error?.message??String(error);}
  paint();
 }
 async function operate(operation,...args){if(operation==='newGame'&&model.readyFlag&&!model.pending&&reloadNewGame(model.started)){model.status=pendingText.newGame;paint();return;}audio?.pause();clearInput();const pending=model.run(operation,saveSession,...args);paint();if(await pending&&operation==='newGame')onNewGame?.();flush();if(operation==='save'||operation==='remove')await refresh();paint();}
 function openSaves(){model.saves();if(model.state!=='saves')return;screen.open({inMenu:!model.started,dead:model.dead,sleeping:sleeping(),seated:seated()});paint();refresh();}
 // Continue enters fullscreen, where Keyboard Lock keeps Ctrl+W (crouch + forward) in the game. Request pointer lock first, because fullscreen takes the click.
 // The settings Exit button keeps the game in a window for this page load.
 let windowed=false;
 async function capture(action=ACTIONS.CONTINUE){clearInput();const pending=captureWithAudio(model,action,async()=>{canvas.focus();const lock=canvas.requestPointerLock();if(!windowed&&!document.fullscreenElement&&document.fullscreenEnabled)document.querySelector('main').requestFullscreen().catch(()=>{});await lock;if(document.pointerLockElement!==canvas)throw Error('The browser did not capture the mouse.');},audio);paint();const success=await pending;if(!success && document.pointerLockElement===canvas)document.exitPointerLock();flush();paint();}
 document.querySelector('#new-game').addEventListener('click',()=>operate('newGame'));
 // Port choice: source `onlyFromMainMenu` locks the draw distance in a game. Here Apply saves the
 // game to the autosave and loads it again with the new distance, as a return to the main menu would.
 document.querySelector('#draw-distance').addEventListener('input',paint);
 applyDistance.addEventListener('click',async()=>{
  if(!canApply())return;
  model.back();
  await operate('save',AUTOSAVE,thumbnail());
  if(model.error||model.state!=='pause-menu')return;
  slider.pend();
  await operate('load',AUTOSAVE);
 });
 document.querySelector('#pause-new-game').addEventListener('click',()=>operate('newGame'));
 for(const id of ['#save','#load','#start-load'])document.querySelector(id).addEventListener('click',openSaves);
 startLast.addEventListener('click',()=>operate('loadLast',buildId));
 document.querySelector('#resume').addEventListener('click',()=>capture());
 document.querySelector('#pause-respawn').addEventListener('click',()=>{if(model.respawn())capture(ACTIONS.RESPAWN|ACTIONS.CONTINUE);});
 for(const id of ['#settings','#start-settings'])document.querySelector(id).addEventListener('click',()=>{model.settings();paint();});
 for(const id of ['#start-game-mode','#pause-game-mode'])document.querySelector(id).addEventListener('click',()=>{model.gameMode();paint();});
 for(const id of ['#appearance','#start-appearance'])document.querySelector(id).addEventListener('click',()=>{model.appearance();paint();});
 for(const id of ['#multiplayer','#start-multiplayer'])document.querySelector(id).addEventListener('click',()=>{model.multiplayer();paint();});
 for(const id of backNodes)document.querySelector(id).addEventListener('click',()=>{model.back();paint();});
 document.querySelector('#recover').addEventListener('click',()=>{clearInput();model.recover();flush();paint();});
 const fullscreen=document.querySelector('#fullscreen');fullscreen.disabled=!document.fullscreenEnabled;
 fullscreen.addEventListener('click',async()=>{windowed=Boolean(document.fullscreenElement);try{if(windowed)await document.exitFullscreen();else await document.querySelector('main').requestFullscreen();}catch{model.error='Fullscreen is unavailable.';paint();}});
 // In fullscreen, Keyboard Lock gives Ctrl+W/N/T to the game. Hold Escape to leave fullscreen.
 // Lock only these keys: a full lock also takes the Windows key, so Win+Shift+S and other system shortcuts stop.
 document.addEventListener('fullscreenchange',()=>{fullscreen.textContent=document.fullscreenElement?'Exit':'Enter';if(document.fullscreenElement)navigator.keyboard?.lock(['Escape','KeyW','KeyN','KeyT']).catch(()=>{});resize();});
 window.addEventListener('keydown',event=>{if(event.code!=='Escape'||event.repeat)return;event.preventDefault();if(model.state==='saves'&&!model.pending){if(screen.state.escape()==='back')model.back();paint();}else if(['settings','game-mode','appearance','multiplayer'].includes(model.state)){model.back();paint();}else if(model.state==='playing'||model.pending)pause(true);else if(model.state==='pause-menu')capture();});
 document.addEventListener('pointerlockchange',()=>{if(document.pointerLockElement!==canvas && model.state==='playing')pause();});
 window.addEventListener('blur',pause);document.addEventListener('visibilitychange',()=>{if(document.hidden)pause();});
 window.addEventListener('resize',resize);resize();paint();
 return {model,drivingState,paint,started(error,operation){startResult={error,operation};},takeDrivingAssists(){const request=pendingDriving;pendingDriving=0;return request;},survival(dead,due){if(dead===model.dead&&due===model.deathComplete)return;model.dead=dead;model.deathComplete=due;if(due){pause();model.status='';model.deathHint=online()?'Respawn to keep driving.':'Start a new game or load a save.';queueMicrotask(()=>document.querySelector(online()?'#pause-respawn':'#pause-new-game').focus({preventScroll:true}));}paint();},loading(){pendingDriving=0;audio?.pause();clearInput();model.loading();if(document.pointerLockElement===canvas)document.exitPointerLock();paint();},async ready(){model.ready();paint();await refresh();if(startResult){if(startResult.error)model.error=operationError(startResult.error);else model.status=resultText[startResult.operation];startResult=null;paint();}if(autoNewGame){autoNewGame=false;await operate('newGame');}},sync(started,stopped,travelM){model.started=started;if(stopped && started && model.state==='playing'){onPause?.();model.pause();paint();}const km=Math.round(travelM/10)/100;if(lastTravel!==km){distanceLabel.textContent=language.word('mdistancedriven');distanceValue.textContent=km.toFixed(2)+' '+language.word('mkm');lastTravel=km;}},fail(message){saveSession.cancel();clearInput();model.fail(message);if(document.pointerLockElement===canvas)document.exitPointerLock();flush();paint();},dispose(){window.removeEventListener('resize',resize);}};
}
