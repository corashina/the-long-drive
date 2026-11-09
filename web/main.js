import {gameFields,installGameMode,installGameOptions,installSeed,installStartCar} from './game-mode.js';
// New game, Load and Join reload the page, as the source loads the game scene again. The simulation
// then starts in the menu game mode, from the save or from the host world, so one world exists at a time.
// A fresh page asks Singleplayer or Multiplayer first, so a join from it loads only the host world.
const NEW_GAME_KEY='tls-new-game',LOAD_KEY='tls-load',JOIN_KEY='tls-join';
const takeSession=key=>{try{const v=sessionStorage.getItem(key);sessionStorage.removeItem(key);return v;}catch{return null;}};
// A browser shortcut (Ctrl+W while the crouch key is held) must not close a running game. The game's own reloads skip the prompt.
let reloading=false;
const reloadWith=(key,value)=>{try{sessionStorage.setItem(key,value);}catch{}reloading=true;location.reload();return new Promise(()=>{});};
const newGameSeed=(()=>{const v=Number(takeSession(NEW_GAME_KEY)??NaN);return Number.isInteger(v)?v:undefined;})();
const loadName=takeSession(LOAD_KEY);
const joinRoom=(()=>{try{return JSON.parse(takeSession(JOIN_KEY)??'null');}catch{return null;}})();
const storedName=(()=>{try{return localStorage.getItem(NAME_KEY)??'';}catch{return '';}})();
const bootChoice=createBootChoice({document,preset:joinRoom?{mode:'join',room:joinRoom}:newGameSeed!==undefined||loadName!==null?{mode:'single'}:null,
 invite:(new URLSearchParams(location.search).get(ROOM_PARAM)??'').trim().toUpperCase().slice(0,6),name:storedName,
 storeName:value=>{try{localStorage.setItem(NAME_KEY,value);}catch{}}});
const firstChoice=bootChoice.pick();
const seed=installSeed(window,document.querySelector('#seed'),Math.random,newGameSeed);
import {createSettings,frameLimit,installSettingsBridge} from './settings.js';
import {displayFields,installDisplaySettings} from './display-settings.js';
import {PRESETS,firstQuality,installQualityPresets,qualityField} from './quality-presets.js';
import {createLanguage,installLanguageSelect,languageField} from './language.js';
import {installFramePacer} from './frame-pacer.js';
import {actionIndex,compileBindings,inputsField} from './bindings.js';
import {createSurvival} from './survival.js';
import {createAudio,installAudioBridge} from './audio.js';
import {manifestStamp,createSaveStorage,AUTOSAVE} from './save-storage.js';
import {createSaveSession,startSave} from './save-session.js';
import {createSaveBridge} from './save-bridge.js';
import {createThumbnail} from './thumbnail.js';
import {createAutosave} from './autosave.js';
const saveRuntime=createSaveBridge();
import {installDrawDistance} from './draw-distance.js';
import {installDrivingSettings} from './driving-settings.js';
import {installOutfit} from './outfit.js';
import {createAppearance} from './appearance.js';
import {createGamepad} from './gamepad.js';
import {installSteeringWheel} from './steering-wheel.js';
const gamepad=createGamepad(()=>navigator.getGamepads?.());
let slider,worldDrawDistance,drivingSettings,steeringWheel,settings;
// The slider is locked while a request is pending, so its value is the one the new world uses.
const saveStorage=createSaveStorage();
const saveSession=createSaveSession({storage:saveStorage,runtime:saveRuntime,reload:name=>reloadWith(LOAD_KEY,name),drawDistance:()=>slider.value(),onStatus:({operation,state})=>{if(state!=='success'||operation==='remove')return;autosave.reset(performance.now()/1000);if(operation!=='save'){worldDrawDistance=slider.value();slider.store();}}});
const autosave=createAutosave(),autosaveNotice=document.querySelector('#autosave-notice');
// Source `BAutoSave` and `BDisableAutoSaveInCar`, kept current by the settings store.
const toggles={autosave:false,inCar:false};
let autosaving=false,noticeTimer=0;
async function runAutosave(){
 autosaving=true;clearTimeout(noticeTimer);autosaveNotice.textContent='Autosaving…';autosaveNotice.hidden=false;
 try{const blob=await thumbnail.capture();if(!saveSession.pending)await saveSession.save(AUTOSAVE,blob,{autosave:true});autosaveNotice.hidden=true;}
 catch(error){autosaveNotice.textContent=error?.message??String(error);noticeTimer=setTimeout(()=>{autosaveNotice.hidden=true;},3000);}
 finally{autosave.reset(performance.now()/1000);autosaving=false;}
}
import {createInput} from './input.js';
import {createCpuProfile,observeRenderMainThread} from './cpu-profile.js';
export const cpuProfile=createCpuProfile(()=>performance.now(),performance.timeOrigin);
const startupCpuProfile=new URL(location.href).searchParams.get('profileCpu')==='1';
const startup=createStartupTrace(()=>performance.now(),{hidden:document.hidden,focused:document.hasFocus(),profileCpu:startupCpuProfile});
const startupVisibilityChanged=()=>startup.environment(document.hidden,document.hasFocus());
const startupBlurred=()=>startup.environment(document.hidden,false);
document.addEventListener('visibilitychange',startupVisibilityChanged);
window.addEventListener('blur',startupBlurred);
function stopStartupEnvironment(){startupVisibilityChanged();document.removeEventListener('visibilitychange',startupVisibilityChanged);window.removeEventListener('blur',startupBlurred);}
cpuProfile.enable(startupCpuProfile);
let rustCpuProfile,rustCpuAudit,profileStep=0,startupRenderTrace=startupCpuProfile?observeRenderMainThread():null;
window.gameCpuProfile=enabled=>{cpuProfile.enable(enabled);profileStep=0;rustCpuProfile?.(enabled);simulation.profile(enabled);};
window.gameCpuProfileSnapshot=()=>cpuProfile.snapshot();
// Main thread Rust stages, in the format of the worker `profile` sandbox reply.
window.gameMainCpuAudit=()=>rustCpuAudit?JSON.parse(rustCpuAudit()):null;
window.gameCpuInstrumentationState=()=>({browserEnabled:cpuProfile.enabled,grass:grassClients.profileState(),startupRenderTraceActive:startupRenderTrace!==null});
window.gameCpuStartupRenderTrace=()=>{const trace=startupRenderTrace?.stop()??null;startupRenderTrace=null;return trace;};
window.gameCpuInputConsumed=()=>input?.consume(++profileStep);
window.gameCpuClearInput=()=>cpuProfile.clearInput();
window.gameCpuSubmitted=token=>cpuProfile.submitFrame(token);
import {createMenu} from './menu.js';
import {createFeedback} from './feedback.js';
import {createLoading} from './loading.js';
import {createStartupTrace} from './startup-trace.js';
import {createPerformance} from './performance.js';
import {createWeatherPanel} from './weather-panel.js';
import {wasmMemoryFailure,wasmMemoryLimit} from './wasm-memory.js';
import {loadUiAssets} from './ui-layout.js';
import {createSimulation,installSimulationBridge} from './simulation.js';
import {NAME_KEY,ROOM_PARAM,createMultiplayer} from './multiplayer.js';
import {createBootChoice} from './boot-choice.js';
import {ROLE} from './net.js';
import {createAnalytics,hostedSite} from './analytics.js';
import {installGrassBridge} from './grass.js';
import {installCrunch} from './crunch.js';
import {configureAssetCatalog,readAsset,releaseStartPacks,startPackMeasurements,worldBootstrapMeasurements,worldInputBytes,setAssetCpuProfile} from './world-assets.js';
setAssetCpuProfile(cpuProfile);
// Start the simulation worker and its WASM download early.
const simulation=createSimulation(new Worker(new URL('./simulation-worker.js',import.meta.url),{type:'module'}),message=>fail(message));
simulation.hooks({inputConsumed:()=>window.gameCpuInputConsumed(),clearInput:()=>window.gameCpuClearInput()});
installSimulationBridge(window,simulation);
const grassClients=installGrassBridge(window,cpuProfile);
window.sourceReadAsset=(base,receipt)=>readAsset(base,JSON.parse(receipt));
window.sourceReleasePacks=releaseStartPacks;
installCrunch(window);
window.sourceInputBytes=worldInputBytes;
const canvas=document.querySelector('#game'),toast=document.querySelector('#toast'),runtimeError=document.querySelector('#runtime-error');
const loading=createLoading(),thumbnail=createThumbnail(canvas);
let lastThumbnail=null;
performance.setResourceTimingBufferSize?.(16000);
let startupFailureDetails=null,startupDone=false,startupReady=false;
function fetched(record){
 const entry=performance.getEntriesByName?.(new URL(record.path,location.href).href)?.at(-1);
 if(!entry){startup.fetched({...record,cache:'unknown',downloadMs:null});return;}
 startup.fetched({...record,cache:entry.transferSize===0?'hit':'miss',downloadMs:entry.responseEnd-entry.requestStart});
}
window.gameStartupTrace=()=>({...startup.snapshot(),startPacks:{...startPackMeasurements}});
window.gameStartupDone=()=>startupDone;
window.gameStartupFailureDetails=json=>{startupFailureDetails=JSON.parse(json);};
window.gameStartupStage=(stage,started)=>{if(started){startup.begin(stage);cpuProfile.begin(stage);}else{startup.end(stage);cpuProfile.end(stage);}};
window.gameLoadingProgress=(...counts)=>loading.progress(...counts);
window.gameLoadingBudget=(name,used,limit,unit)=>loading.budget(name,used,limit,unit);
const bootStarted=performance.now();
const analytics=createAnalytics({enabled:hostedSite(location.hostname),send:body=>navigator.sendBeacon('/api/event',body),session:crypto.randomUUID(),role:()=>multiplayer?.role??ROLE.OFF});
analytics.event('start');
window.addEventListener('pagehide',()=>analytics.end());
let adapterName='';
let wasmMemory,renderAudit,watchRenderAudit,renderAuditWatched=false,initialReadyMs=null,peakFrameMs=0,lastFrameTime=null,measureRequested=false;
const wasmLimit=wasmMemoryLimit(navigator.deviceMemory),wasmTotal=()=>(wasmMemory?.buffer.byteLength??0)+simulation.wasmBytes()+grassClients.wasmBytes();
const performancePanel=createPerformance(wasmTotal,wasmLimit,()=>renderAudit&&JSON.parse(renderAudit()),on=>{renderAuditWatched=on;watchRenderAudit?.(on);},()=>worldDrawDistance,()=>simulation.stats());
window.gamePerformanceFrame=performancePanel.frame;
window.gamePerformanceBudget=performancePanel.budget;
window.gamePerformanceCounters=performancePanel.counters;
window.gamePerformance=since=>performancePanel.snapshot(since);
createWeatherPanel(simulation,()=>multiplayer?.role===ROLE.GUEST);
// The failure message after a startup or runtime failure, else null. `gameFailure` reports a failure.
window.gameFailureMessage=()=>failure;
// The start menu model, for `tests/bench/fps.mjs`. Null before the interface loads.
window.gameMenu=()=>menu?{state:menu.model.state,started:menu.model.started,pending:menu.model.pending,ready:menu.model.isReady(),status:menu.model.status,error:menu.model.error}:null;
// Test sandbox (`simulation::sandbox`), for `tests/bench/fps.mjs`. Only a `?sandbox` page has it.
if(new URLSearchParams(location.search).has('sandbox'))window.gameSandbox=actions=>simulation.sandbox(JSON.stringify(actions)).then(JSON.parse);
const measurementWaiters=[];
const bootstrap={manifestUtf16PayloadBytes:0,dependencyGate:worldBootstrapMeasurements};
window.gameNow=()=>performance.now();
window.measureWorld=()=>new Promise(resolve=>{measurementWaiters.push(resolve);measureRequested=true;});
window.gameMeasureRequested=()=>{const requested=measureRequested;measureRequested=false;return requested;};
window.gameMeasurements=json=>{
 const snapshot=JSON.parse(json);
 snapshot.browser={wasmLinearBytes:wasmMemory?.buffer.byteLength??null,jsHeapBytes:performance.memory?.usedJSHeapSize??null,
  initialStartReadyMs:initialReadyMs,peakFrameIntervalMs:peakFrameMs,bootstrap};
 for(const resolve of measurementWaiters.splice(0))resolve(snapshot);
};
let input,survival,dead=false,sleeping=false,audio,menu,multiplayer,feedback,failed=false,failure=null,worldAdmitted=false,actions=0,frame,renderFailure;
const inventory={selected:0,slots:[0,0,0,0],progress:0};
window.gameInventory=(selected,a,b,c,d,progress)=>{
 inventory.selected=selected;inventory.slots[0]=a;inventory.slots[1]=b;inventory.slots[2]=c;inventory.slots[3]=d;inventory.progress=progress;
 window.gameState.inventory=inventory;feedback?.inventory(inventory);
};
window.gameWeaponCrosshair=(icon,color)=>feedback?.weapon(icon,color);
const record={actions:0,context:0,name:'',notice:'',carrying:false,amount:0,capacity:0,hasName:false,noText:true,fluidWord:0,help:0,held:'',throwCharge:0,throwActive:false,radioVolume:undefined,radioFrequency:undefined,radioAm:false,quantities:[0,0,0,0,0,0],fluidOrder:0,hose:0,computer:0,crosshair:0,weight:1};
const settleSave=window.gameSaveResult;
window.gameSaveResult=(id,kind,payload,save,error)=>{
 if(kind==='Commit'&&!error){input.clear();actions=0;}
 settleSave(id,kind,payload,save,error);
};
window.gameTypingKeys=()=>input.state.typingKeys();
window.gameTypedText=buffer=>input.state.takeText(buffer);
window.gameInventoryKeys=()=>frame.inventoryHeld|(frame.inventoryReleased<<4);
window.gamePlayerKeys=()=>(frame.playerHeld|(frame.playerPressed<<16))>>>0;window.gamePlayerReleased=()=>frame.playerReleased;
window.gameKeys=()=>{frame=input.takeFrame();return frame.held;};
window.gamePressed=()=>frame.pressed;window.gameReleased=()=>frame.released;
window.gameMouseX=()=>frame.mouseX;window.gameMouseY=()=>frame.mouseY;window.gameWheel=()=>frame.wheel;window.gameDriveMouseX=()=>frame.driveX;window.gameDriveMouseY=()=>frame.driveY;
window.gameDrivingAssists=()=>menu?.takeDrivingAssists()??0;
window.gameDrivingSettings=()=>drivingSettings?.take();
// Rust passes its `GamepadFrame` arrays once per frame.
window.gameGamepad=(axes,buttons)=>{gamepad.poll();axes.set(gamepad.axes);buttons.set(gamepad.buttons);};
// `steer` to `handbrake` are the last Rust `DrivingControls`, shown only on the Controls screen.
window.gameDrivingState=(gear,assists,seated,steer,throttle,brake,clutch,handbrake)=>{window.gameState.gear=gear;window.gameState.drivingAssists=assists;menu?.drivingState(assists);feedback?.drivingState(gear,seated,assists);if(menu?.model.state==='settings')steeringWheel?.live(steer,throttle,brake,clutch,handbrake);};
window.gameActions=()=>{const a=actions;actions=0;return a;};
const CAMERA_VIEWS=['First person','Outside','Drop cam','Map'];
window.gameState = { speedKmh: 0, camera: 'First person', x: 0, y: 0, z: 0, contacts: 0,
  frameSteps: 0, totalSteps: 0, activeSurfaces: 0, colliderCount: 0, absoluteX: 0, absoluteY: 0, absoluteZ: 0,
  originX: 0, originY: 0, originZ: 0, rebases: 0, travelM: 0, preparedRoads: 0, fuelLiters: 0,
  paused: false, started: false, target: '', targetAction: 0, carrying:false, heldItem:'', rpm:0, ignition:false, running:false, starter:false, seated:false, handbrake:true, throwCharge:0, throwActive:false, autosaveSafe:false, fluidVolumes:record.quantities };
window.gameUpdate = (kmh, mode, x, y, z, contacts, steps, surfaces, colliders, absoluteX, absoluteY, absoluteZ,
  originX, originY, originZ, rebases, travelM, preparedRoads, fuelLiters, paused, started, autosaveSafe, seated) => {
  const now=performance.now();
  const memoryFailure=wasmMemoryFailure(wasmTotal(),wasmLimit);
  if(memoryFailure)fail(memoryFailure);
  if(lastFrameTime!==null)peakFrameMs=Math.max(peakFrameMs,now-lastFrameTime);
  lastFrameTime=now;
  const state = window.gameState;
  state.speedKmh = kmh; state.camera = CAMERA_VIEWS[mode];
  state.x = x; state.y = y; state.z = z; state.contacts = contacts;
  state.frameSteps = steps; state.totalSteps += steps; state.activeSurfaces = surfaces; state.colliderCount = colliders;
  state.absoluteX = absoluteX; state.absoluteY = absoluteY; state.absoluteZ = absoluteZ;
  state.originX = originX; state.originY = originY; state.originZ = originZ;
  state.rebases = rebases; state.travelM = travelM; state.preparedRoads = preparedRoads;
  state.fuelLiters = fuelLiters; state.paused = paused; state.started = started;
  state.autosaveSafe = autosaveSafe; state.seated = seated;
  menu?.sync(started,paused,travelM);
  const playing=menu?.model.state==='playing';
  multiplayer?.frame(playing&&started&&!paused);
  analytics.frame(now,playing&&started&&!paused);
  // Source guests have no save.
  if(autosave.tick(now/1000,playing,autosaveSafe,seated,toggles)&&!autosaving&&!saveSession.pending&&multiplayer?.role!==ROLE.GUEST)runAutosave();
  feedback?.setVisible(started&&!paused&&!dead&&!sleeping&&mode===0&&menu?.model.state==='playing');
  survival?.setVisible(started&&!paused&&!sleeping&&menu?.model.state==='playing');
};
window.gameRadioFeedback=(volume,frequency,am)=>{record.radioVolume=volume;record.radioFrequency=frequency;record.radioAm=am;if(record.context===11||record.context===12)feedback?.update(record);};
window.gameFeedback=(mask,context,name,notice,carrying,amount,capacity,hasName,noText,fluidWord,held,charge,throwing,rpm,ignition,running,starter,seated,handbrake,gas,oil,alcohol,water,blood,diesel,fluidOrder,control,lmbUse,prompts,computer,crosshair,weight,help)=>{
 record.actions=mask;record.prompts=prompts;record.computer=computer;record.crosshair=crosshair;record.weight=weight;input?.state.setTyping((computer&32)!==0);record.context=context;record.lmbUse=lmbUse;record.name=name;record.notice=notice;record.carrying=carrying;record.amount=amount;record.capacity=capacity;record.hasName=hasName;record.noText=noText;record.fluidWord=fluidWord;record.help=help;
 record.held=held;record.throwCharge=charge;record.throwActive=throwing;
 const q=record.quantities;q[0]=gas;q[1]=oil;q[2]=alcohol;q[3]=water;q[4]=blood;q[5]=diesel;record.fluidOrder=fluidOrder;
 const state=window.gameState;
 state.feedbackContext=context;state.availableActions=mask;state.lightControl=control;state.lmbUse=lmbUse;
 state.target=name;state.targetAction=mask;state.carrying=carrying;state.heldItem=held;state.throwCharge=charge;state.throwActive=throwing;
 state.rpm=rpm;state.ignition=ignition;state.running=running;state.starter=starter;state.seated=seated;state.handbrake=handbrake;
 feedback?.update(record);
};
const needs={hp:0,food:0,water:0,bowel:0,bladder:0,opacity:0,dead:false,lookScale:1,menuDue:false};
window.gameSurvival=(hp,food,water,bowel,bladder,opacity,isDead,lookScale,menuDue)=>{
 needs.hp=hp;needs.food=food;needs.water=water;needs.bowel=bowel;needs.bladder=bladder;needs.opacity=opacity;needs.dead=isDead;needs.lookScale=lookScale;needs.menuDue=menuDue;dead=isDead;survival?.update(needs);menu?.survival(isDead,menuDue);
};
const sleep={phase:0,eligible:false,showHint:false,charge:0,cooldownRemaining:0,progress:0,held:false,dead:false};
window.gameSleep=(phase,eligible,showHint,charge,cooldownRemaining,progress)=>{
 sleep.phase=phase;sleep.eligible=eligible;sleep.showHint=showHint;sleep.charge=charge;sleep.cooldownRemaining=cooldownRemaining;sleep.progress=progress;sleep.held=(input.state.held&262144)!==0;sleep.dead=dead;sleeping=phase===2;survival?.updateSleep(sleep,feedback?.state.enabled);
};
function fail(message,notifyRenderer=true){if(failed)return;failed=true;failure=message;bootChoice.close();analytics.error(message);if(message.startsWith('WASM memory '))slider?.clear();performancePanel.stop();startup.reconcile(performance.getEntriesByType('resource'));stopStartupEnvironment();startup.fail({...startupFailureDetails,message,loading:loading.snapshot()});startupDone=true;loading.fail();const text='Cannot start the game. '+message+' Reload to try again.';toast.classList.remove('shown');if(menu)menu.fail(text);else{runtimeError.textContent=text;runtimeError.hidden=false;}if(notifyRenderer)renderFailure?.(message);}
function finishStartup(){if(failed||startupDone||!startupReady||!input||!menu)return;startup.reconcile(performance.getEntriesByType('resource'));stopStartupEnvironment();startup.finish({inputReady:true});startupDone=true;}
window.gameFailure=message=>fail(message,false);
window.gameLoading=()=>{if(failed)return;loading.start(true);menu.loading();};
window.gameReady=()=>{if(failed||!worldAdmitted)return;if(initialReadyMs===null){initialReadyMs=performance.now()-bootStarted-bootChoice.waitedMs;analytics.event('ready',{loadMs:Math.round(initialReadyMs),gpu:adapterName,quality:settings.values.quality});}releaseStartPacks();loading.finish();menu.ready();multiplayer?.ready();startupReady=true;finishStartup();};
window.addEventListener('error',event=>fail(event.message||'The runtime stopped.'));
window.addEventListener('unhandledrejection',event=>fail(event.reason?.message||String(event.reason)));
const mebibytes=bytes=>Math.round(bytes/1048576).toLocaleString('en-US');
// Brotli hides the decoded size, so then the step shows only the received size.
async function downloadManifest(response){
 const encoding=response.headers.get('content-encoding')?.trim().toLowerCase();
 const declared=Number(response.headers.get('content-length'));
 const total=(!encoding||encoding==='identity')&&declared>0?declared:0;
 let received=0;
 const show=()=>loading.step('download',received,total,mebibytes(received)+(total?' / '+mebibytes(total):'')+' MiB');
 const counted=response.body.pipeThrough(new TransformStream({transform(chunk,stream){received+=chunk.byteLength;stream.enqueue(chunk);}}));
 show();const timer=setInterval(show,250);
 try{
  const packed=new Uint8Array(await new Response(counted).arrayBuffer());
  // The file is gzip at rest. The hosted site sends a Brotli copy of the plain file, and fetch decodes it.
  // A host that sends Content-Encoding: gzip also gives the plain file here.
  const bytes=packed[0]===0x1f&&packed[1]===0x8b?new Uint8Array(await new Response(new Blob([packed]).stream().pipeThrough(new DecompressionStream('gzip'))).arrayBuffer()):packed;
  loading.step('download',1,1,mebibytes(bytes.byteLength)+' MiB');
  return bytes;
 }finally{clearInterval(timer);}
}
const NO_WEBGPU='This browser cannot use WebGPU. Use desktop Chrome or Edge 113 or later.';
async function load(){
 cpuProfile.begin('startup');
 // Check the device before the downloads start. Phones and tablets have no keyboard and mouse.
 if(!matchMedia('(any-pointer: fine)').matches)throw Error('The game needs a keyboard and mouse. Open it on a desktop computer.');
 if(!navigator.gpu)throw Error(NO_WEBGPU);
 // Download and compile the game code and download the manifest while the browser opens the adapter.
 const code=Promise.all([import('./pkg/the_long_slop.js'),fetch('./pkg/the_long_slop_bg.wasm').then(response=>{
  if(!response.ok)throw Error('Cannot load game code (HTTP '+response.status+').');
  return WebAssembly.compileStreaming(response);
 })]);
 const manifestBytes=fetch('./assets/reference/manifest.bin.gz').then(response=>{
  if(!response.ok)throw Error('Cannot load reference manifest (HTTP '+response.status+').');
  return downloadManifest(response);
 });
 code.catch(()=>{});manifestBytes.catch(()=>{});
 startup.begin('graphics');
 // Bevy asks for the high performance adapter, so this is the GPU that renders.
 const adapter=await navigator.gpu?.requestAdapter({powerPreference:'high-performance'});
 if(!adapter)throw Error(NO_WEBGPU);
 const quality=firstQuality(adapter.info,navigator);
 adapterName=[adapter.info.vendor,adapter.info.architecture,adapter.info.description].filter(Boolean).join(' ');
 startup.end('graphics');startup.begin('manifest');
 const bytes=await manifestBytes;
 // The worker parses and checks its copy while this thread loads the interface.
 simulation.load(bytes.slice());
 loading.step('code',0);
 const [{default:init,prepare,run,source_render_failure,source_render_audit,watch_render_audit,cpu_profile,cpu_profile_audit,request_thumbnail,wake_frame,prewarm_pipeline_list,set_depth_prepass},module]=await code;
 const exports=await init({module_or_path:module});wasmMemory=exports.memory;loading.step('code',1);renderFailure=source_render_failure;renderAudit=source_render_audit;watchRenderAudit=watch_render_audit;watch_render_audit(renderAuditWatched);
 rustCpuProfile=cpu_profile;rustCpuAudit=cpu_profile_audit;if(startupCpuProfile)rustCpuProfile(true);
 // Hidden bench toggle: ?prepass=0 draws the world view opaque pass without the depth prepass.
 if(new URLSearchParams(location.search).get('prepass')==='0')set_depth_prepass(false);
 window.gameThumbnailFrame=()=>thumbnail.frame();thumbnail.connect(request_thumbnail);
 // The bundled pipeline prewarm list plus this run's play pipelines. Save it as src/presentation/prewarm_pipelines.json.
 window.gamePrewarmList=()=>prewarm_pipeline_list();
 // Rust parses the manifest once and returns only the shell fields, so JS does not parse the rest.
 startup.begin('prepare');const text=prepare(bytes),manifest=JSON.parse(text);startup.end('prepare');analytics.build(manifest.build_id);
 const manifestHash=manifestStamp(manifest);
 const bindings=manifest.driving.bindings,actionIds=actionIndex(bindings);
 settings=createSettings(()=>localStorage,manifest.settings,{...displayFields(manifest.world.mirrors.draw_distance,PRESETS[quality]),quality:qualityField(quality),...gameFields(manifest.world.start_cars.length),inputs:inputsField(bindings),language:languageField(manifest.language)});
 const language=createLanguage(manifest.language,settings,document);
 installLanguageSelect(document.querySelector('#language'),settings,language.names);
 installGameMode(window,document.querySelector('#biome'),settings);
 installStartCar(window,document.querySelector('#start-car'),manifest.world.start_cars,settings);
 const gameOptions=installGameOptions(window,{peaceful:document.querySelector('#peaceful'),survival:document.querySelector('#survival-mode'),homesick:document.querySelector('#homesick')},settings,options=>survival?.setSurvival(options.survival));
 installSettingsBridge(window,settings,()=>menu?.model.state,actionIds);
 installDisplaySettings(window,settings,document,manifest.world.mirrors.draw_distance);
 installQualityPresets(settings,document.querySelector('#display-quality'));
 installFramePacer(f=>requestAnimationFrame(f),()=>frameLimit(settings.values,menu?.model.state),wake_frame);
 const autosaveToggles=v=>{toggles.autosave=v.BAutoSave;toggles.inCar=!v.BDisableAutoSaveInCar;};
 autosaveToggles(settings.values);settings.listen(autosaveToggles);
 const outfit=installOutfit(window,()=>localStorage,manifest.clothing.characters);
 const range=manifest.world.draw_distance;
 slider=installDrawDistance(()=>localStorage,()=>sessionStorage,document.querySelector('#draw-distance'),document.querySelector('#draw-distance-value'),{minimum:range.minimum.drawDistance,maximum:range.maximum.drawDistance,fallback:range.default});
 worldDrawDistance=slider.value();
 const garageMode=[worldDrawDistance,window.gameBiome(),window.gameStartCar(),window.gameSeed()];
 // The world of the boot choice: the host world of a join, the save of a Load reload, or of Load last. `save` is its JSON.
 // A failed join asks again, so no world loads before the player has one.
 let multiplayerReady;const multiplayerCreated=new Promise(resolve=>{multiplayerReady=resolve;});
 const storedSave=async()=>{
  const name=await startSave({loadName,newGame:newGameSeed!==undefined,autoLoad:settings.values.BAutoLoad,list:()=>saveStorage.list(),buildId:manifest.build_id});
  if(name===null)return null;
  const save=await saveStorage.read(name);if(save===null)throw new Error('No saved game.');
  return {save,operation:'load'};
 };
 const startWorld=(async()=>{
  for(let choice=await firstChoice;;){
   if(choice.mode==='single'){loading.join('');return storedSave().catch(error=>({error:error?.message??String(error),operation:'load'}));}
   const result=await (await multiplayerCreated).join(choice.room);
   if(result.save)return {save:result.save,operation:'join'};
   choice=await bootChoice.pick(result.error);
  }
 })();
 const simulationStarted=startWorld.then(async world=>({world,...await simulation.start(manifestHash,...garageMode,gameOptions.bits(),world?.save??new Uint8Array())}));
 // False: the garage is the new game world.
 const reloadNewGame=started=>{
  const mode=[slider.value(),window.gameBiome(),window.gameStartCar(),window.gameSeed()];
  if(!started&&mode.every((v,i)=>v===garageMode[i]))return false;
  reloadWith(NEW_GAME_KEY,String(window.gameSeed()));return true;
 };
 simulationStarted.catch(()=>{});
 fetched({path:'assets/reference/manifest.bin.gz',requiredBytes:bytes.byteLength,receivedBytes:bytes.byteLength});
 startup.build({sourceBuildId:manifest.build_id,manifestSha256:manifestHash,wasm:'release'});
 configureAssetCatalog(manifest.content_base,manifest.file_catalog,fetched);worldAdmitted=true;
 startup.end('manifest');startup.begin('interface');
 drivingSettings=installDrivingSettings(()=>localStorage,[document.querySelector('#mouse-drive-x'),document.querySelector('#mouse-drive-y')],[document.querySelector('#mouse-drive-x-value'),document.querySelector('#mouse-drive-y-value')],manifest.driving);
 steeringWheel=installSteeringWheel(document.querySelector('#steering-wheel'),drivingSettings,manifest.driving.steering_wheel,gamepad,callback=>requestAnimationFrame(callback));
 input=createInput(canvas,()=>menu?.model.state==='playing' && document.pointerLockElement===canvas && !multiplayer?.typing(),compileBindings(bindings,settings.values.inputs),cpuProfile);
 // Source `S.inputs`: the input maps change once per binding change.
 {let inputs=settings.values.inputs;settings.listen(v=>{if(v.inputs!==inputs){inputs=v.inputs;input.setMaps(compileBindings(bindings,inputs));}});}
 bootstrap.manifestUtf16PayloadBytes=text.length*2;
 loading.step('interface',0);
 await loadUiAssets(manifest.ui);
  survival=createSurvival(manifest.ui,manifest.sleep.cooldown_seconds,settings,language,actionIds);survival.setSurvival(settings.values.survival);
 audio=createAudio(manifest.audio);installAudioBridge(window,audio);
 const volume=v=>audio.volume(v.FSound);volume(settings.values);settings.listen(volume);
 window.addEventListener('pagehide',()=>audio.dispose());
 window.addEventListener('beforeunload',event=>{if(menu?.model.started&&!reloading)event.preventDefault();});
 const appearance=createAppearance({target:window,document,canvas,edit:outfit,characters:manifest.clothing.characters,language});
 // Source tab resets of the controls that keep their own storage.
 const resets={graphics:[()=>slider.reset()],controls:[()=>drivingSettings.resetMouse()],steering:[()=>steeringWheel.reset()]};
 menu=createMenu({canvas,language,appearance,settings,resets,defaultAssists:manifest.driving.default_assists,ui:manifest.ui,bindings,saveSession,buildId:manifest.build_id,thumbnail:()=>lastThumbnail,sleeping:()=>sleeping,seated:()=>window.gameState.seated,guest:()=>multiplayer?.role===ROLE.GUEST,online:()=>Boolean(multiplayer)&&multiplayer.role!==ROLE.OFF,worldDrawDistance:()=>worldDrawDistance,slider,audio,onNewGame:()=>seed.reroll(),reloadNewGame,autoNewGame:newGameSeed!==undefined,onPause:()=>thumbnail.capture().then(blob=>{lastThumbnail=blob;}),clearInput:()=>input.clear(),queueAction:a=>{actions=a;}});
 multiplayer=createMultiplayer({document,target:window,Peer:window.peerjs.Peer,buildId:manifest.build_id,exportHash:manifestHash,simulation,saveSession,menu,outfit,storage:()=>localStorage,
  reloadJoin:room=>reloadWith(JOIN_KEY,JSON.stringify(room)),loading,
  audio,media:navigator.mediaDevices,voiceKeys:()=>actionIds.voicechat===undefined?[]:settings.values.inputs[actionIds.voicechat]});
 multiplayerReady(multiplayer);
 {const changed=outfit.changed;outfit.changed=()=>{changed();multiplayer.look(outfit.outfit);};}
 feedback=createFeedback(manifest.ui,manifest,settings,actionIds,language);
 startup.end('interface');
 if(failed)return;loading.step('interface',1);
 startup.begin('generation');startup.begin('decodeUpload');startup.begin('pipeline');
 // The start message goes out in the first callback of `startWorld`, before this await ends.
 await startWorld;if(failed)return;
 // The renderer starts before the worker is ready. The GPU device, pipelines and garage downloads
 // overlap the worker parse and session start. Its first command waits in the worker queue.
 startup.begin('run');run();startup.end('run');
 cpuProfile.end('startup');
 const {world,loadError}=await simulationStarted;
 if(world)menu.started(world.error||loadError,world.operation);
 // A draw distance applied in a game is kept once a world starts with it.
 if(world&&!world.error&&!loadError)slider.store();
 if(world?.operation==='join'&&(world.error||loadError))multiplayer.leave();
}
load().catch(error=>fail(error?.message??String(error)));
