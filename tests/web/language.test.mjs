import test from 'node:test';
import assert from 'node:assert/strict';
import {languageField,languageNames,wordTable} from '../../web/language.js';
import {HELP,helpText} from '../../web/help-text.js';

const record={names:['mback','myes','mno'],english:['Back','Yes','No'],languages:[{name:'Deutsch',words:['Zurück',null,'']}]};

test('a_missing_or_empty_word_shows_the_english_word',()=>{
 assert.deepEqual(wordTable(record,'Deutsch'),{mback:'Zurück',myes:'Yes',mno:'No'});
 assert.deepEqual(wordTable(record,'English'),{mback:'Back',myes:'Yes',mno:'No'});
 assert.deepEqual(languageNames(record),['English','Deutsch']);
 assert.equal(languageField(record).valid('Klingon'),false);
});

test('help_text_follows_the_vehicle_and_the_assists',()=>{
 const word=w=>({ignition:'Ignition',swhandbrake:'Handbrake',stand:'Stand',accelerate:'Accelerate',swbrake:'Brake',swsteer:'Steer',upshift:'Up Shift',downshift:'Down Shift',
  reverse:'Reverse',neutral:'Neutral',shiftgear:'Shift Gear',lean:'Lean',leanupdown:'Lean Up/Down',lights:'Lights',mouse:'Mouse'})[w];
 const names={ignition:'I',handbrake:'Space',clutch:'LeftShift',gearr:'R',gearn:'N',gear1:'Alpha1',gear4:'Alpha4',looklean:'Q',scrollup:'Scroll',lights:'L',gas:'W',brake:'S',
  steerleft:'A',steerright:'D',shiftup:'E',shiftdown:'F',lookright:'Mouse X'};
 const car=helpText(HELP.SHOWN,0,word,names,true).split('\n');
 assert.equal(car.length,9);
 assert.equal(car[2],'Reverse: [LeftShift]+[R]  ');
 assert.equal(car[4],'Shift Gear: [LeftShift]+[Alpha1-Alpha4]  ');
 assert.equal(car[5],'Lean: [Q]+[Mouse]  ');
 assert.equal(helpText(HELP.SHOWN,1,word,names,true).split('\n')[2],'Up Shift: [E]  ');
 // Auto shift needs auto clutch.
 assert.equal(helpText(HELP.SHOWN,2,word,names,true),helpText(HELP.SHOWN,0,word,names,true));
 // Source bikes set isMotorcycle and isBike: isMotorcycle wins. ForceAuto takes the auto shift set.
 const bike=helpText(HELP.SHOWN|HELP.MOTORCYCLE|HELP.BIKE|HELP.FORCE_AUTO,0,word,names,true);
 assert.equal(bike,'Ignition: [I]  \nStand: [Space]  \nAccelerate: [W]  \nBrake: [S]  \nSteer: [A]/[D]  \n');
 assert.equal(helpText(HELP.SHOWN|HELP.BIKE,3,word,names,true).split('\n').length,5);
});
