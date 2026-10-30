import {installSceneResolution} from './scene-resolution.js';
const sceneResolution=document.querySelector('#scene-resolution');
installSceneResolution(window,sceneResolution);
import {installShadowQuality} from './shadow-quality.js';
installShadowQuality(window,()=>localStorage,document.querySelector('#shadow-quality'));
import {installGrassQuality} from './grass-quality.js';
installGrassQuality(window,()=>localStorage,document.querySelector('#grass-quality'));
import {gameFields,installGameMode,installGameOptions,installSeed,installStartCar} from './game-mode.js';
// New game, Load and Join reload the page, as the source loads the game scene again. The simulation
// then starts in the menu game mode, from the save or from the host world, so one world exists at a time.
const NEW_GAME_KEY='tls-new-game',LOAD_KEY='tls-load',JOIN_KEY='tls-join';
const takeSession=key=>{try{const v=sessionStorage.getItem(key);sessionStorage.removeItem(key);return v;}catch{return null;}};
const reloadWith=(key,value)=>{try{sessionStorage.setItem(key,value);}catch{}location.reload();return new Promise(()=>{});};
const newGameSeed=(()=>{const v=Number(takeSession(NEW_GAME_KEY)??NaN);return Number.isInteger(v)?v:undefined;})();
const loadName=takeSession(LOAD_KEY);
const joinRoom=(()=>{try{return JSON.parse(takeSession(JOIN_KEY)??'null');}catch{return null;}})();
const seed=installSeed(window,document.querySelector('#seed'),Math.random,newGameSeed);
import {installSunglassesEffects} from './sunglasses.js';
import {installScreenEffects} from './screen-effects.js';
installScreenEffects(window,()=>localStorage,{heatHaze:document.querySelector('#heat-haze'),oldHeatHaze:document.querySelector('#old-heat-haze'),lut:document.querySelector('#lut'),grayscale:document.querySelector('#grayscale')});
const sunglasses=document.querySelector('#sunglasses-effects');
installSunglassesEffects(window,()=>localStorage,sunglasses);
import {installMotionBlur} from './motion-blur.js';
installMotionBlur(window,()=>localStorage,document.querySelector('#motion-blur'));
import {installAntialiasing} from './antialiasing.js';
installAntialiasing(window,()=>localStorage,document.querySelector('#antialiasing'));
import {installAmbientOcclusion} from './ambient-occlusion.js';
installAmbientOcclusion(window,()=>localStorage,document.querySelector('#ambient-occlusion'));
import {installLampShadows} from './lamp-shadows.js';
installLampShadows(window,()=>localStorage,document.querySelector('#lamp-shadows'));
import {installDepthOfField} from './depth-of-field.js';
installDepthOfField(window,()=>localStorage,document.querySelector('#depth-of-field'));
import {installReflections} from './reflections.js';
installReflections(window,()=>localStorage,document.querySelector('#reflections'));
import {installMirrors} from './mirrors.js';
const mirrors=installMirrors(window,()=>localStorage,document.querySelector('#mirrors'),document.querySelector('#mirror-draw-distance'),document.querySelector('#mirror-draw-distance-value'));
import {createSettings,frameLimit,installSettingsBridge} from './settings.js';
import {createLanguage,installLanguageSelect,languageField} from './language.js';
import {installFramePacer} from './frame-pacer.js';
import {actionIndex,compileBindings,inputsField} from './bindings.js';
import {createSurvival} from './survival.js';
import {createAudio,installAudioBridge} from './audio.js';
import {manifestStamp,createSaveStorage,verifyRecord,AUTOSAVE} from './save-storage.js';
import {createSaveSession,lastSave} from './save-session.js';
import {createSaveBridge} from './save-bridge.js';
import {createThumbnail} from './thumbnail.js';
import {createAutosave} from './autosave.js';
export const saveRuntime=createSaveBridge();
import {installDrawDistance} from './draw-distance.js';
import {installDrivingSettings} from './driving-settings.js';
import {installOutfit} from './outfit.js';
import {createAppearance} from './appearance.js';
import {createMenuFigure} from './menu-figure.js';
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
 autosaving=true;clearTimeout(noticeTimer);autosaveNotice.textContent='Autosaving...';autosaveNotice.hidden=false;
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
let rustCpuProfile,profileStep=0,startupRenderTrace=startupCpuProfile?observeRenderMainThread():null;
window.gameCpuProfile=enabled=>{cpuProfile.enable(enabled);profileStep=0;rustCpuProfile?.(enabled);simulation.profile(enabled);if(enabled)grassClients.profileHandshake();};
window.gameCpuProfileSnapshot=()=>cpuProfile.snapshot();
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
import {loadUiAssets} from './ui-layout.js';
import {createSimulation,installSimulationBridge} from './simulation.js';
import {createMultiplayer} from './multiplayer.js';
import {ROLE} from './net.js';
import {readSandbox,installSandbox} from './sandbox.js';
import {installGrassBridge} from './grass.js';
import {configureAssetCatalog,readAsset,worldBootstrapMeasurements,worldInputBytes,worldInputMetadataBytes,setAssetCpuProfile} from './world-assets.js';
setAssetCpuProfile(cpuProfile);
// Start the simulation worker and its WASM download early.
const simulation=createSimulation(new Worker(new URL('./simulation-worker.js',import.meta.url),{type:'module'}),message=>fail(message));
simulation.hooks({inputConsumed:()=>window.gameCpuInputConsumed(),clearInput:()=>window.gameCpuClearInput()});
installSimulationBridge(window,simulation);
const grassClients=installGrassBridge(window,cpuProfile);
window.sourceReadAsset=(base,receipt)=>readAsset(base,JSON.parse(receipt));
window.sourceInputBytes=worldInputBytes;
window.sourceInputMetadataBytes=worldInputMetadataBytes;
const canvas=document.querySelector('#game'),status=document.querySelector('#status'),runtimeError=document.querySelector('#runtime-error');
const loading=createLoading(),thumbnail=createThumbnail(canvas);
let lastThumbnail=null;
performance.setResourceTimingBufferSize?.(16000);
let startupFailureDetails=null,startupDone=false,startupReady=false;
function fetched(record){
 const entry=performance.getEntriesByName?.(new URL(record.path,location.href).href)?.at(-1);
 startup.fetched({...record,cache:entry?entry.transferSize===0?'hit':'miss':'unknown',downloadMs:entry?entry.responseEnd-entry.requestStart:null});
}
window.gameStartupTrace=()=>startup.snapshot();
window.gameStartupDone=()=>startupDone;
window.gameStartupFailureDetails=json=>{startupFailureDetails=JSON.parse(json);};
window.gameStartupStage=(stage,started)=>{if(started){startup.begin(stage);cpuProfile.begin(stage);}else{startup.end(stage);cpuProfile.end(stage);}};
window.gameLoadingProgress=(roads,total,rendering,world)=>loading.progress(roads,total,rendering,world);
window.gameLoadingBudget=(name,used,limit,unit)=>loading.budget(name,used,limit,unit);
const bootStarted=performance.now();
let wasmMemory,renderAudit,initialReadyMs=null,peakFrameMs=0,lastFrameTime=null,measureRequested=false;
const performancePanel=createPerformance(()=>wasmMemory?.buffer.byteLength,()=>renderAudit&&JSON.parse(renderAudit()),()=>worldDrawDistance,()=>simulation.stats());
window.gamePerformanceFrame=(physicsMs,worldMs)=>performancePanel.frame(performance.now(),physicsMs,worldMs);
window.gamePerformanceCounters=performancePanel.counters;
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
const sandbox=readSandbox(location.search);installSandbox(window,sandbox!==null);
if(sandbox)window.gameSeed=()=>sandbox.seed;
let input,survival,dead=false,sleeping=false,audio,menu,multiplayer,feedback,failed=false,worldAdmitted=false,actions=0,frame,renderFailure;
const inventory={selected:0,slots:[0,0,0,0],progress:0};
window.gameInventory=(selected,a,b,c,d,progress)=>{
 inventory.selected=selected;inventory.slots[0]=a;inventory.slots[1]=b;inventory.slots[2]=c;inventory.slots[3]=d;inventory.progress=progress;
 window.gameState.inventory=inventory;feedback?.inventory(inventory);
};
window.gameWeaponCrosshair=(icon,color)=>feedback?.weapon(icon,color);
const record={actions:0,context:0,name:'',notice:'',carrying:false,amount:0,capacity:0,hasName:false,noText:true,fluidWord:0,help:0,held:'',throwCharge:0,throwActive:false,radioVolume:undefined,radioFrequency:undefined,radioAm:false,quantities:[0,0,0,0,0,0],fluidOrder:0,hose:0,computer:0,crosshair:0,weight:1};
const settleSave=window.gameSaveResult;
window.gameSaveResult=(id,kind,payload,error)=>{
 if(kind==='Commit'&&!error){input.clear();actions=0;}
 settleSave(id,kind,payload,error);
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
function fail(message,notifyRenderer=true){if(failed)return;failed=true;if(/Draw distance [\d.]+ m does not fit/.test(message))slider?.clear();performancePanel.stop();startup.reconcile(performance.getEntriesByType('resource'));stopStartupEnvironment();startup.fail({...startupFailureDetails,message,loading:loading.snapshot()});startupDone=true;loading.fail();const text='Cannot start the game. '+message+' Reload to try again.';if(menu){menu.fail(text);status.textContent='';}else{status.textContent='';runtimeError.textContent=text;runtimeError.hidden=false;}if(notifyRenderer)renderFailure?.(message);}
function finishStartup(){if(failed||startupDone||!startupReady||!input||!menu)return;startup.reconcile(performance.getEntriesByType('resource'));stopStartupEnvironment();startup.finish({inputReady:true});startupDone=true;}
window.gameFailure=message=>fail(message,false);
window.gameLoading=()=>{if(failed)return;loading.start();loading.stage('Loading the garage...');menu.loading();};
window.gameReady=()=>{if(failed||!worldAdmitted)return;if(initialReadyMs===null)initialReadyMs=performance.now()-bootStarted;loading.finish();menu.ready();multiplayer?.ready();startupReady=true;finishStartup();};
window.addEventListener('error',event=>fail(event.message||'The runtime stopped.'));
window.addEventListener('unhandledrejection',event=>fail(event.reason?.message||String(event.reason)));
async function load(){
 cpuProfile.begin('startup');
 startup.begin('graphics');
 if(!await navigator.gpu?.requestAdapter())throw Error('This browser cannot use WebGPU.');
 startup.end('graphics');startup.begin('manifest');
 // Download and compile the game code while the manifest downloads.
 const code=Promise.all([import('./pkg/the_long_slop.js'),fetch('./pkg/the_long_slop_bg.wasm').then(response=>{
  if(!response.ok)throw Error('Cannot load game code (HTTP '+response.status+').');
  return WebAssembly.compileStreaming(response);
 })]);
 code.catch(()=>{});
 const response=await fetch('./assets/reference/manifest.json');
 if(!response.ok)throw Error('Cannot load reference manifest (HTTP '+response.status+').');
 const bytes=new Uint8Array(await response.arrayBuffer());
 status.textContent='Loading game code...';
 loading.stage(status.textContent);
 const [{default:init,run,manifest_shell,source_render_failure,source_render_audit,cpu_profile,request_thumbnail,wake_frame},module]=await code;
 const exports=await init({module_or_path:module});wasmMemory=exports.memory;renderFailure=source_render_failure;renderAudit=source_render_audit;
 rustCpuProfile=cpu_profile;if(startupCpuProfile)rustCpuProfile(true);
 window.gameThumbnailFrame=()=>thumbnail.frame();thumbnail.connect(request_thumbnail);
 // Rust copies only the shell fields, so JS does not parse the world records.
 const text=manifest_shell(bytes),manifest=JSON.parse(text);
 const {manifest_sha256:manifestHash}=await manifestStamp(bytes,manifest);
 const bindings=manifest.driving.bindings,actionIds=actionIndex(bindings);
 settings=createSettings(()=>localStorage,manifest.settings,{...gameFields(manifest.world.start_cars.length),inputs:inputsField(bindings),language:languageField(manifest.language)});
 const language=createLanguage(manifest.language,settings,document);
 installLanguageSelect(document.querySelector('#language'),settings,language.names);
 installGameMode(window,document.querySelector('#biome'),settings);
 installStartCar(window,document.querySelector('#start-car'),manifest.world.start_cars,settings);
 const gameOptions=installGameOptions(window,{peaceful:document.querySelector('#peaceful'),survival:document.querySelector('#survival-mode'),homesick:document.querySelector('#homesick')},settings,options=>survival?.setSurvival(options.survival));
 installSettingsBridge(window,settings,()=>menu?.model.state==='playing',actionIds);
 installFramePacer(f=>requestAnimationFrame(f),()=>frameLimit(settings.values,menu?.model.state==='playing'),wake_frame);
 const autosaveToggles=v=>{toggles.autosave=v.BAutoSave;toggles.inCar=!v.BDisableAutoSaveInCar;};
 autosaveToggles(settings.values);settings.listen(autosaveToggles);
 const outfit=installOutfit(window,()=>localStorage,manifest.clothing.characters);
 const range=manifest.world.draw_distance;
 slider=installDrawDistance(()=>localStorage,document.querySelector('#draw-distance'),document.querySelector('#draw-distance-value'),{minimum:range.minimum.drawDistance,maximum:range.maximum.drawDistance,fallback:range.default});
 mirrors.range(manifest.world.mirrors.draw_distance);
 // The simulation parses its own manifest copy while this thread loads the interface.
 worldDrawDistance=slider.value();
 const garageMode=[worldDrawDistance,window.gameBiome(),window.gameStartCar(),window.gameSeed()];
 // The world of a Join reload, of a Load reload, or of Load last at start. `save` is its JSON.
 let joined;
 const storedSave=async()=>{
  const name=loadName??(newGameSeed===undefined&&settings.values.BAutoLoad?lastSave(await saveStorage.list(),manifest.build_id):null);
  if(name===null)return null;
  const record=await saveStorage.read(name);if(record===null)throw new Error('No saved game.');
  return {save:new TextEncoder().encode(JSON.stringify(await verifyRecord(record))),operation:'load'};
 };
 const startWorld=joinRoom?new Promise(resolve=>{joined=resolve;}):storedSave().catch(error=>({error:error?.message??String(error),operation:'load'}));
 const simulationStarted=startWorld.then(async world=>({world,...await simulation.start(bytes.slice(),manifestHash,...garageMode,gameOptions.bits(),world?.save??new Uint8Array())}));
 // False: the garage is the new game world.
 const reloadNewGame=started=>{
  const mode=[slider.value(),window.gameBiome(),window.gameStartCar(),window.gameSeed()];
  if(!started&&mode.every((v,i)=>v===garageMode[i]))return false;
  reloadWith(NEW_GAME_KEY,String(window.gameSeed()));return true;
 };
 simulationStarted.catch(()=>{});
 fetched({path:'assets/reference/manifest.json',requiredBytes:bytes.byteLength,receivedBytes:bytes.byteLength});
 startup.build({sourceBuildId:manifest.build_id,manifestSha256:manifestHash,wasm:'release'});
 configureAssetCatalog(manifest.content_base,manifest.file_catalog,fetched);worldAdmitted=true;
 startup.end('manifest');startup.begin('interface');
 drivingSettings=installDrivingSettings(()=>localStorage,[document.querySelector('#mouse-drive-x'),document.querySelector('#mouse-drive-y')],[document.querySelector('#mouse-drive-x-value'),document.querySelector('#mouse-drive-y-value')],manifest.driving);
 steeringWheel=installSteeringWheel(document.querySelector('#steering-wheel'),drivingSettings,manifest.driving.steering_wheel,gamepad,callback=>requestAnimationFrame(callback));
 input=createInput(canvas,()=>menu?.model.state==='playing' && document.pointerLockElement===canvas && !multiplayer?.typing(),compileBindings(bindings,settings.values.inputs),cpuProfile);
 // Source `S.inputs`: the input maps change once per binding change.
 {let inputs=settings.values.inputs;settings.listen(v=>{if(v.inputs!==inputs){inputs=v.inputs;input.setMaps(compileBindings(bindings,inputs));}});}
 bootstrap.manifestUtf16PayloadBytes=text.length*2;
 status.textContent='Loading interface assets...';
 loading.stage(status.textContent);
 await loadUiAssets(manifest.ui);
  survival=createSurvival(manifest.ui,manifest.sleep.cooldown_seconds,settings,language,actionIds);survival.setSurvival(settings.values.survival);
 audio=createAudio(manifest.audio);installAudioBridge(window,audio);
 const volume=v=>audio.volume(v.FSound);volume(settings.values);settings.listen(volume);
 if(sandbox?.calendar)window.gameAudioCalendar=()=>sandbox.calendar;
 window.addEventListener('pagehide',()=>audio.dispose());
 const appearance=createAppearance({target:window,document,canvas,edit:outfit,characters:manifest.clothing.characters,language});
 const figure=createMenuFigure({target:window,canvas});
 // Source tab resets of the controls that keep their own storage.
 const choose=(node,property,value,event)=>{if(node.disabled)return;node[property]=value;node.dispatchEvent(new Event(event));};
 const select=id=>document.querySelector(id);
 const resets={graphics:[()=>choose(sunglasses,'checked',true,'change'),()=>choose(select('#heat-haze'),'checked',true,'change'),()=>choose(select('#old-heat-haze'),'checked',false,'change'),()=>choose(select('#lut'),'value','default','change'),()=>choose(select('#grayscale'),'checked',false,'change'),()=>choose(sceneResolution,'value','100','change'),()=>choose(select('#shadow-quality'),'value','0','change'),
  ()=>choose(select('#grass-quality'),'value','0','change'),()=>choose(select('#motion-blur'),'checked',true,'change'),()=>choose(select('#depth-of-field'),'checked',false,'change'),()=>choose(select('#lamp-shadows'),'checked',true,'change'),()=>choose(select('#antialiasing'),'checked',true,'change'),()=>choose(select('#ambient-occlusion'),'checked',true,'change'),()=>choose(select('#reflections'),'checked',true,'change'),()=>mirrors.reset(),()=>slider.reset()],controls:[()=>drivingSettings.resetMouse()]};
 menu=createMenu({canvas,language,appearance,figure,settings,resets,defaultAssists:manifest.driving.default_assists,ui:manifest.ui,bindings,saveSession,buildId:manifest.build_id,thumbnail:()=>lastThumbnail,sleeping:()=>sleeping,seated:()=>window.gameState.seated,guest:()=>multiplayer?.role===ROLE.GUEST,slider,audio,onNewGame:()=>seed.reroll(),reloadNewGame,autoNewGame:newGameSeed!==undefined,onPause:()=>thumbnail.capture().then(blob=>{lastThumbnail=blob;}),clearInput:()=>input.clear(),queueAction:a=>{actions=a;}});
 multiplayer=createMultiplayer({document,target:window,Peer:window.peerjs.Peer,buildId:manifest.build_id,exportHash:manifestHash,simulation,saveSession,menu,outfit,storage:()=>localStorage,
  room:joinRoom,joined:(save,error)=>joined({save,error,operation:'join'}),reloadJoin:room=>reloadWith(JOIN_KEY,JSON.stringify(room))});
 {const changed=outfit.changed;outfit.changed=()=>{changed();multiplayer.look(outfit.outfit);};}
 feedback=createFeedback(manifest.ui,manifest,settings,actionIds,language);
 startup.end('interface');
 if(failed)return;status.textContent='Loading the garage...';loading.stage(status.textContent);
 startup.begin('generation');startup.begin('decodeUpload');startup.begin('pipeline');
 const {world,startup:startupBytes,loadError}=await simulationStarted;
 if(world)menu.started(world.error||loadError,world.operation);
 if(world?.operation==='join'&&(world.error||loadError))multiplayer.leave();
 if(failed)return;run(bytes,startupBytes);
 cpuProfile.end('startup');
}
load().catch(error=>fail(error?.message??String(error)));
