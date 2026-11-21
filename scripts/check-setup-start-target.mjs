import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';

const source = fs.readFileSync(new URL('../gui/src/lib/SetupExperience.svelte', import.meta.url), 'utf8').match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
const ast = ts.createSourceFile('setup.ts', source, ts.ScriptTarget.Latest, true);
const names = ['activate', 'createAndStart', 'importAndStart', 'useExisting', 'message', 'rememberCreated'];
const functions = ast.statements.filter(n => ts.isFunctionDeclaration(n) && names.includes(n.name.text));
assert.equal(functions.length, names.length);
const code = ts.transpileModule(functions.map(n => n.getText(ast)).join('\n'), { compilerOptions: { target: ts.ScriptTarget.ESNext } }).outputText;

async function run(route, failure) {
    let nextDefault = 'old/conf.toml';
    const calls = [];
    const profile = { id: 'chosen', label: 'Chosen', conf_path: 'stale/client/path.toml' };
    const api = {
        createDesktopProfile: async () => { calls.push('create'); if(failure === 'save') throw Error('save uncertain'); return { profile }; },
        importDesktopProfileBundle: async () => { calls.push('import'); if(failure === 'save') throw Error('save uncertain'); return { profile }; },
        activateDesktopProfile: async id => {
            calls.push(['activate', id]);
            if (failure === 'activate') throw Error('activation uncertain');
            const result = { profile: { ...profile, conf_path: 'authoritative/chosen/conf.toml' } };
            // A second VAC changes the shared default after the first activation.
            nextDefault = 'another/profile/conf.toml';
            return result;
        },
        restart: async request => {
            calls.push(['restart', request.conf ?? nextDefault]);
            assert.equal(request.graceful_ms, 800);
            if (failure === 'restart') throw Error('restart uncertain');
        },
    };
    const setup = new Function('api', 'prepareProfileImport', `
        let busy=false,error=null,label='Chosen',selectedTemplateId='blank',importSource={},createdProfiles=[],restartRequested=null;
        const uniqueProfileId=()=> 'chosen',profileImportBaseName=()=> 'Chosen';
        ${code}
        return {createAndStart,importAndStart,useExisting,state:()=>({busy,error,createdProfiles,restartRequested})};
    `)(api, async () => ({ bundle: route === 'bundle', body: new Blob(['conf']) }));
    if (route === 'create') await setup.createAndStart();
    else if (route === 'existing') await setup.useExisting(profile);
    else await setup.importAndStart();
    const result = { calls: [...calls], ...setup.state() };
    if (failure === 'activate' && route !== 'existing') {
        failure = null;
        await setup.useExisting(result.createdProfiles[0]);
        assert.equal(calls.filter(c => c === 'create' || c === 'import').length, 1, 'Recovery opens saved profile without creating again');
        assert.equal(setup.state().error, null);
        assert.equal(setup.state().createdProfiles.length, 1);
    }
    return result;
}

for (const route of ['create', 'existing', 'bundle', 'toml']) {
    const success = await run(route);
    assert.deepEqual(success.calls.at(-1), ['restart', 'authoritative/chosen/conf.toml'], route);
    assert.equal(success.busy, true);
    assert.equal(success.error, null);
    assert.deepEqual(success.restartRequested, {label:'Chosen',confPath:'authoritative/chosen/conf.toml'});
    const activationFailure = await run(route, 'activate');
    assert.equal(activationFailure.calls.some(c => Array.isArray(c) && c[0] === 'restart'), false);
    assert.equal(activationFailure.busy, false);
    assert.equal(activationFailure.restartRequested, null);
    assert.match(activationFailure.error, /再起動は要求していません/);
    assert.match(activationFailure.error, /activation uncertain/);
    assert.equal(activationFailure.createdProfiles.length, route === 'existing' ? 0 : 1);
    const restartFailure = await run(route, 'restart');
    assert.equal(restartFailure.calls.filter(c => Array.isArray(c) && c[0] === 'restart').length, 1);
    assert.equal(restartFailure.busy, false);
    assert.equal(restartFailure.restartRequested, null);
    assert.match(restartFailure.error, /次回起動先の変更は完了/);
    assert.match(restartFailure.error, /restart uncertain/);
    if (route !== 'existing') {
        const failedSave = await run(route, 'save');
        assert.equal(failedSave.createdProfiles.length, 0, 'Unconfirmed save must not appear as confirmed');
        assert.equal(failedSave.error, 'save uncertain');
        assert.equal(failedSave.calls.length, 1);
    }
}
console.log('PASS: four setup routes keep authoritative selected target despite shared-default change; failures do not retry. Extracted logic, mock API only.');
