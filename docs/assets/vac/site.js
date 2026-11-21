// Preserve bookmarks to the original public v1 guide.
const legacySections = new Set([
  "usage", "download-and-install", "first-run", "speech-recognition", "image-recognition",
  "image-recognition-processor", "text-to-speech-os-tts", "text-to-speech-coeiroink",
  "text-to-speech-bouyomichan", "language-translation-gas", "ai-pseudo-personality-openai",
  "modify", "output-screen", "input-screen", "system-and-keywords"
]);
if (legacySections.has(location.hash.slice(1))) {
  location.replace(new URL("v1.html" + location.hash, location.href));
}

const examples = {
  follow: ["♥", "NEW FOLLOWER", "ゲストさん、ようこそ！", "ゲストさんがフォローしました。"],
  raid: ["✦", "INCOMING RAID", "8人の皆さん、ようこそ！", "ゲストさんが8人でRaidしました。"],
  reward: ["◆", "CHANNEL POINTS", "ゲストさんが「ひとこと」を交換！", "ゲストさんの「ひとこと」交換を受け付けました。"]
};
const selector = document.querySelector(".event-select");
const targets = ["event-icon", "event-title", "alert-message", "chat-message"].map(id => document.getElementById(id));

function renderExample() {
  const selected = selector.querySelector("input:checked").value;
  targets.forEach((target, index) => { target.textContent = examples[selected][index]; });
  document.querySelector(".demo").dataset.example = selected;
}

selector.addEventListener("change", renderExample);
renderExample();
selector.hidden = false;
