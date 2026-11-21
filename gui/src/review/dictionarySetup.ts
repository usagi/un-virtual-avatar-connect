import { mount } from 'svelte';
import DictionarySetupReviewPane from '../lib/control/dictionary/DictionarySetupReviewPane.svelte';
import '../app.css';
import '../styles/un-series.css';
import '../lib/theme.svelte';

document.documentElement.dataset.unProduct='vac';
document.documentElement.dataset.unDesign='hybrid';

// Dedicated comparison entry; production navigation does not link here.
const target=document.getElementById('app');
if (!target) throw new Error('Review mount missing');
target.style.cssText='max-width:1000px;margin:24px auto;padding:16px';
mount(DictionarySetupReviewPane,{target,props:{comparison:new URLSearchParams(location.search).get('normal')!=='1'}});
