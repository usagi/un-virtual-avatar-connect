(() => {
 const requestId = __REQUEST_ID__;
 const root = document.documentElement;
 if (location.origin !== __EXPECTED_ORIGIN__) return {requestId, allowed:false};
 const event = new Event('beforeunload', {cancelable:true});
 window.dispatchEvent(event);
 const allowed = root.dataset.unCloseGuardReady === 'true' && !event.defaultPrevented;
 const wasInert = root.__vacCloseReview?.wasInert ?? root.inert;
 root.__vacCloseReview = {requestId, wasInert};
 if (allowed) root.inert = true;
 else {
  root.inert = wasInert;
  delete root.__vacCloseReview;
  window.dispatchEvent(new Event('vac-close-blocked'));
 }
 return {requestId, allowed};
})();
