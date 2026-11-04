import js from '@eslint/js';
import globals from 'globals';

export default [
 {ignores:['web/pkg/','web/vendor/','web/assets/']},
 {files:['web/**/*.js'],languageOptions:{globals:{...globals.browser,...globals.worker}}},
 {files:['tests/web/**/*.mjs'],languageOptions:{globals:globals.node}},
 // The bench runs in Node. Its page callbacks run in the browser.
 {files:['tests/bench/**/*.mjs'],languageOptions:{globals:{...globals.node,...globals.browser}}},
 // Real bugs only: style rules stay off.
 {files:['**/*.js','**/*.mjs'],rules:{...js.configs.recommended.rules,'no-unused-vars':'off','no-empty':'off'}},
];
