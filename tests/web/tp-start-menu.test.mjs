// Roadmap test pass rows of the Start menu figure section.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createMenuFigure} from '../../web/menu-figure.js';
import {MenuState} from '../../web/menu.js';

// Source `menuIkScript` runs on the start menu only. menu.js `paint` toggles the figure with
// `state==='start-menu'`. Rust despawns the figure stage while `gameMenuFigure` returns false.
test('tp_sm5_the_figure_shows_only_on_the_start_menu',()=>{
 const target={addEventListener(){}},figure=createMenuFigure({target,canvas:{}}),out=new Float32Array(4);
 const model=new MenuState(),states=[];
 const paint=()=>{figure.toggle(model.state==='start-menu');states.push([model.state,target.gameMenuFigure(out)]);};
 model.ready();paint();
 model.appearance();paint();
 model.back();paint();
 model.started=true;model.state='playing';paint();
 model.ready();paint();
 model.appearance();paint();
 assert.deepEqual(states,[['start-menu',true],['appearance',false],['start-menu',true],['playing',false],['pause-menu',false],['appearance',false]]);
});
