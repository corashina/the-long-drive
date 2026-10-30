// Source: autoSaveInterval 120 s, canAutosaveTime 5 s. Times are seconds.
export function createAutosave({interval=120,safeTime=5}={}){
 let lastSaved=0,since=null;
 return {
  tick(now,playing,safe,seated,toggles){
   if(!toggles.autosave||!playing||!safe||(seated&&!toggles.inCar)){since=null;return false;}
   if(since===null)since=now;
   return now-since>=safeTime&&now-lastSaved>=interval;
  },
  reset(now){lastSaved=now;}
 };
}
