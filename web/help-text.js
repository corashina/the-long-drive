// Source `mainscript.RefreshHelpTextLanguage` and `RefreshHelpTexts`: the help lines of the `helptextscript` box under the use ray.
// Template tokens: `<word>` is an `ls.w` word, `{action}` is the bound key name of a source action in brackets.
// `{szamok}` is source `Szamok` and `{mouse}` is source `Mouse`.
/** Rust `interaction::help_texts` bits. */
export const HELP={SHOWN:1,FORCE_AUTO:2,MOTORCYCLE:4,BIKE:8,ALWAYS:16};
const IGNITION='<ignition>: {ignition} ',HANDBRAKE='<swhandbrake>: {handbrake} ',STAND='<stand>: {handbrake} ',ACCELERATE='<accelerate>: {gas} ',BRAKE='<swbrake>: {brake} ',
 STEER='<swsteer>: {steerleft}/{steerright} ',NEUTRAL='<neutral>: {clutch}+{gearn} ',SHIFT='<shiftgear>: {clutch}+{szamok} ',UP='<upshift>: {shiftup} ',DOWN='<downshift>: {shiftdown} ',
 LEAN='<lean>: {looklean}+{mouse} ',LEAN_UP_DOWN='<leanupdown>: {looklean}+{scrollup} ',LIGHTS='<lights>: {lights} ';
// Sets by vehicle, then by assists: manual, auto clutch, auto shift.
export const SETS={
 car:[[IGNITION,HANDBRAKE,'<reverse>: {clutch}+{gearr} ',NEUTRAL,SHIFT,LEAN,LEAN_UP_DOWN,LIGHTS],
  [IGNITION,HANDBRAKE,UP,DOWN,LEAN,LEAN_UP_DOWN,LIGHTS],
  [HANDBRAKE,ACCELERATE,'<swbrake>/<reverse>: {brake} ',STEER,IGNITION,LEAN,LEAN_UP_DOWN,LIGHTS]],
 motorcycle:[[IGNITION,STAND,ACCELERATE,BRAKE,STEER,NEUTRAL,SHIFT],[IGNITION,STAND,ACCELERATE,BRAKE,STEER,UP,DOWN],[IGNITION,STAND,ACCELERATE,BRAKE,STEER]],
 bike:[[STAND,ACCELERATE,BRAKE,STEER,SHIFT],[STAND,ACCELERATE,BRAKE,STEER,UP,DOWN],[STAND,ACCELERATE,BRAKE,STEER]],
};
// The template lines for help bits `help` and the assist bits (1 auto clutch, 2 auto shift). `ForceAuto` takes the auto shift set.
// The source reads `isMotorcycle` before `isBike`.
export function helpLines(help,assists){
 const sets=help&HELP.MOTORCYCLE?SETS.motorcycle:help&HELP.BIKE?SETS.bike:SETS.car;
 return sets[help&HELP.FORCE_AUTO?2:assists&1?(assists&2?2:1):0];
}
// Fills the template tokens. `names` are the bound key names by action (`boundNames`), `inputs` the settings store keys.
export function fillHelp(template,word,names,mouseLook){
 return template.replace(/<(\w+)>/g,(_,w)=>word(w)).replace(/\{(\w+)\}/g,(_,a)=>a==='szamok'?'['+names.gear1+'-'+names.gear4+']':a==='mouse'?'['+(mouseLook?word('mouse'):names.lookright)+']':'['+names[a]+']');
}
// Source ThelpText: each line, a space and a line break.
export function helpText(help,assists,word,names,mouseLook){
 return helpLines(help,assists).map(line=>fillHelp(line,word,names,mouseLook)+' \n').join('');
}
