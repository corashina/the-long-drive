// Source `ls` words. `record` is the exported `language`: `ls.w` names, English words and the language files.
// The settings store field `language` holds the language name (source PlayerPrefs `SelectedLanguage`).
export const ENGLISH='English';
// Source `ls.ScanLanguages`: English, then the files in directory order.
export function languageNames(record){return [ENGLISH,...record.languages.map(l=>l.name)];}
export function languageField(record){const names=languageNames(record);return {default:ENGLISH,valid:v=>names.includes(v)};}
// Source `ls.menu` dropdown.
export function installLanguageSelect(select,settings,names){
 for(const name of names)select.append(new Option(name,name));
 const paint=v=>{select.value=v.language;};
 select.addEventListener('change',()=>settings.set('language',select.value));
 settings.listen(paint);paint(settings.values);
}
// Source `ls.LoadSelectedLanguage`: a missing or empty word shows the English word. Returns words by `ls.w` name.
export function wordTable(record,name){
 const file=record.languages.find(l=>l.name===name),words={};
 record.names.forEach((n,i)=>{words[n]=file?.words[i]||record.english[i];});
 return words;
}
// Source `ls.UpdLanguage`. Elements with `data-w` show the word that the attribute names.
// `listen` callbacks run after a language change.
export function createLanguage(record,settings,doc){
 let current=settings.values.language,words=wordTable(record,current);
 const listeners=new Set();
 function apply(){for(const n of doc.querySelectorAll('[data-w]'))n.textContent=words[n.dataset.w];}
 settings.listen(v=>{
  if(v.language===current)return;
  current=v.language;words=wordTable(record,current);apply();
  for(const listener of listeners)listener();
 });
 apply();
 return {
  names:languageNames(record),
  word:name=>words[name],
  // The word of an `ls.w` index.
  index:i=>words[record.names[i]]??'',
  // Shows the word `name` in `node`, now and after each language change.
  text(node,name){node.dataset.w=name;node.textContent=words[name];return node;},
  listen(listener){listeners.add(listener);},
 };
}
