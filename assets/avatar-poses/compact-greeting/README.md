# Compact greeting prototype

`greeting_hand` raises the left hand beside the face, holds it, and returns it to neutral. It is a greeting gesture, not a repeated wave. The current camera and stream layout are unchanged.

The catalog owns only `LeftUpperArm`, `LeftLowerArm`, and `LeftHand`. The upper arm uses a 70-degree Z rotation and the forearm a -150-degree Z rotation; the hand remains at identity. The release asset restores the upper arm to the existing 90-degree neutral rotation and the other two bones to identity. Attack/hold/release durations are 400/900/400 ms.

## Verification, 2026-09-14

Tested against the running Kaltsit Renderer using an isolated VAC profile and the existing pose Flowgraph/compositor route. Capture `target/avatar-chest-20260914/frames-compositor-d/` shows the raised hand in frame 012, lowering in frame 018, and neutral in frame 027. The hand is visible beside the face in the current camera. These sampled images establish pose, intermediate lowering, and return; they do not establish perceptually smooth animation at full frame rate.

The static direct replay in `frames-a/` is separate evidence and bypasses the compositor. Runtime events are saved in `compositor-d-events.json`; an earlier successful compositor run is recorded in `diagnostics-after.json`.

AI selection was tested in an isolated profile: greeting/farewell emitted the preset, ordinary explanation and silence did not. A captured AI-selected pose reached the Renderer and returned to neutral (`target/greeting-selection-20260914/frames/`, frames 027 and 060). The four-case run still failed one speech-length check. Later corrected runtime tests showed raised hand, open mouth, and changed head posture together during speech, followed by neutral restoration; graph-reload cancellation also restored neutral. Exact expression strength, acoustic timing, live conversational naturalness, and other avatar rigs remain ungraded. See `docs/maintenance/compact-greeting-evaluation-2026-09-14.md` for evidence and limitations.

On 2026-09-14, the preset was added to the personal production profile alongside its unchanged existing presets, with speech synchronization enabled and greeting/farewell guidance. Original config and catalog backups are in `target/profile-greeting-integration-20260914/`. Configuration and graph validation passed; the next ordinary desktop startup loads the changes. A full-profile live stream test remains outstanding. Do not replace an existing production catalog wholesale with this single-preset catalog.
