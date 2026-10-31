// Runs ROADMAP test pass rows of one section in one game tab. See tests/roadmap/README.md.
// node tests/roadmap/run.mjs <section> [row ...] [--url=URL] [--chrome=PATH]
import {mkdir,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {chromium} from 'playwright-core';
import {Game} from './game.mjs';

const SECTIONS=['car-camera','creatures','saves','settings','weapons','world'];
const args=process.argv.slice(2),flags=Object.fromEntries(args.filter(a=>a.startsWith('--')).map(a=>a.slice(2).split('=')));
const [name,...wanted]=args.filter(a=>!a.startsWith('--'));
if(!SECTIONS.includes(name)){console.log('Sections: '+SECTIONS.join(', '));process.exit(2);}
const section=(await import('./'+name+'.mjs')).default;
const rows=Object.entries(section.rows).filter(([id])=>!wanted.length||wanted.includes(id));
const outDir=join('tests','roadmap','out',name+'-'+new Date().toISOString().replace(/[:.]/g,'-'));
await mkdir(outDir,{recursive:true});

// Installed Chrome, headed: WebGPU needs the real GPU.
const browser=await chromium.launch({...(flags.chrome?{executablePath:flags.chrome}:{channel:'chrome'}),headless:false,args:['--enable-unsafe-webgpu']});
const page=await browser.newPage({viewport:{width:1600,height:900}});
const game=new Game(page,flags.url??'http://127.0.0.1:4174/?sandbox',outDir);
const results=[];
try{
 await game.open(section.query);
 await game.newGame();
 for(const [id,row] of rows){
  const started=Date.now();game.shots=[];game.errors.length=0;
  console.log(`${section.name} ${id}: ${row.title} ...`);
  let result;
  try{result=await Promise.race([row.run(game),game.sleep(row.seconds??300).then(()=>({status:'FAIL',evidence:'Row timed out.'}))]);}
  catch(e){result={status:'FAIL',evidence:e.message};await game.shot(id+'-error').catch(()=>{});}
  result.shots=game.shots;result.errors=game.errors.slice(0,5);result.seconds=Math.round((Date.now()-started)/1000);
  results.push({id,title:row.title,...result});
  console.log(`${result.status} ${section.name} ${id}: ${result.evidence}`+result.shots.map(s=>'\n  screenshot '+s).join('')+result.errors.map(e=>'\n  console '+e).join(''));
  // A failed game cannot run the next rows.
  if(await game.failure().then(()=>false,()=>true))break;
 }
}catch(e){console.log('FAIL '+section.name+' setup: '+e.message);results.push({id:'setup',title:'Load and New game',status:'FAIL',evidence:e.message,shots:[],errors:game.errors.slice(0,5)});}
const report=['# '+section.name+' run '+new Date().toISOString(),'','| Row | Status | Evidence |','| --- | --- | --- |',
 ...results.map(r=>`| ${r.id} ${r.title} | ${r.status} | ${r.evidence.replace(/\|/g,'/').replace(/\n/g,' ')}${r.shots.map(s=>' '+s).join('')} |`)].join('\n');
await writeFile(join(outDir,'report.md'),report+'\n');
await writeFile(join(outDir,'report.json'),JSON.stringify(results,null,1));
console.log('\n'+report+'\n\nReport: '+join(outDir,'report.md'));
await browser.close();
