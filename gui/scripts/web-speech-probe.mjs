/* global fetch, navigator, performance, process, setTimeout */

import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { chromium, firefox } from "@playwright/test";

const args = process.argv.slice(2);
const option = (name, fallback) => {
	const index = args.indexOf(name);
	return index >= 0 ? args[index + 1] : fallback;
};
const browserOption = option("--browser", "all");
const jsonOut = option("--json-out", null);
const audioOption = option("--audio", null);
const audioPath = audioOption ? resolve("..", audioOption) : null;
const audioBytes = audioPath ? await readFile(audioPath) : null;
const startMs = Number(option("--start-ms", "0"));
const endMsOption = option("--end-ms", null);
const endMs = endMsOption === null ? null : Number(endMsOption);
const reference = option("--reference", null);
const supported = new Set(["all", "chrome", "edge", "firefox"]);
if (!supported.has(browserOption)) {
	throw new Error(`unknown browser: ${browserOption}`);
}

const normalizeText = (value) =>
	value
		.normalize("NFKC")
		.toLowerCase()
		.replace(/[\s、。,.!?！？「」『』（）()]/gu, "");

const characterErrorRate = (expected, actual) => {
	const left = [...normalizeText(expected)];
	const right = [...normalizeText(actual)];
	const previous = Array.from({ length: right.length + 1 }, (_, index) => index);
	for (let leftIndex = 1; leftIndex <= left.length; leftIndex += 1) {
		const current = [leftIndex];
		for (let rightIndex = 1; rightIndex <= right.length; rightIndex += 1) {
			current[rightIndex] = Math.min(
				current[rightIndex - 1] + 1,
				previous[rightIndex] + 1,
				previous[rightIndex - 1] + (left[leftIndex - 1] === right[rightIndex - 1] ? 0 : 1)
			);
		}
		previous.splice(0, previous.length, ...current);
	}
	return previous[right.length] / Math.max(left.length, 1);
};

const candidates = [
	{
		name: "chrome",
		launcher: chromium,
		executablePath: "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
	},
	{
		name: "edge",
		launcher: chromium,
		executablePath: "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
	},
	{
		name: "firefox",
		launcher: firefox,
		executablePath: "C:\\Program Files\\Mozilla Firefox\\firefox.exe",
	},
].filter(({ name }) => browserOption === "all" || browserOption === name);

const server = createServer((request, response) => {
	if (request.url === "/audio.wav" && audioBytes) {
		response.writeHead(200, {
			"Content-Type": "audio/wav",
			"Content-Length": audioBytes.length,
			"Cache-Control": "no-store",
		});
		response.end(audioBytes);
		return;
	}
	response.writeHead(200, {
		"Content-Type": "text/html; charset=utf-8",
		"Permissions-Policy": "microphone=(self), on-device-speech-recognition=(self)",
		"Cache-Control": "no-store",
	});
	response.end("<!doctype html><meta charset=utf-8><title>UNVAC Web Speech probe</title>");
});
await new Promise((resolveListen) => server.listen(0, "127.0.0.1", resolveListen));
const address = server.address();
const url = `http://127.0.0.1:${address.port}/`;

const results = [];
for (const candidate of candidates) {
	let browser;
	try {
		browser = await candidate.launcher.launch({
			executablePath: candidate.executablePath,
			headless: true,
		});
		const browserVersion = browser.version();
		const page = await browser.newPage();
		await page.goto(url);
		const capabilities = await page.evaluate(
			async ({ audioUrl, startMs, endMs }) => {
				const Recognition = globalThis.SpeechRecognition ?? globalThis.webkitSpeechRecognition;
				const base = {
					userAgent: navigator.userAgent,
					secureContext: globalThis.isSecureContext,
					constructor: Recognition?.name ?? null,
					unprefixedConstructor: typeof globalThis.SpeechRecognition === "function",
					prefixedConstructor: typeof globalThis.webkitSpeechRecognition === "function",
					mediaDevices: typeof navigator.mediaDevices === "object",
					getUserMedia: typeof navigator.mediaDevices?.getUserMedia === "function",
					audioContext: typeof (globalThis.AudioContext ?? globalThis.webkitAudioContext) === "function",
					startArity: Recognition?.prototype?.start?.length ?? null,
					interimResults: Recognition ? "interimResults" in Recognition.prototype : false,
					continuous: Recognition ? "continuous" in Recognition.prototype : false,
					processLocally: Recognition ? "processLocally" in Recognition.prototype : false,
					available: typeof Recognition?.available === "function",
					install: typeof Recognition?.install === "function",
					availability: {},
					audioTrackStart: { attempted: false, accepted: false, event: null, error: null },
					recognition: null,
				};
				if (base.available) {
					const withTimeout = (promise) =>
						Promise.race([promise, new Promise((_, reject) => setTimeout(() => reject(new Error("timeout")), 5000))]);
					for (const quality of ["command", "dictation", "conversation"]) {
						for (const processLocally of [true, false]) {
							const key = `${quality}:${processLocally ? "local" : "remote-allowed"}`;
							try {
								base.availability[key] = await withTimeout(
									Recognition.available({ langs: ["ja-JP"], quality, processLocally })
								);
							} catch (error) {
								base.availability[key] = `${error.name}: ${error.message}`;
							}
						}
					}
				}
				if (!Recognition || !base.audioContext) {
					return base;
				}

				let context;
				let oscillator;
				let source;
				let track;
				let audioDurationMs = null;
				const audioStartDelayMs = 2000;
				if (audioUrl) {
					const AudioContext = globalThis.AudioContext ?? globalThis.webkitAudioContext;
					context = new AudioContext();
					await context.resume();
					const buffer = await context.decodeAudioData(await (await fetch(audioUrl)).arrayBuffer());
					const destination = context.createMediaStreamDestination();
					source = context.createBufferSource();
					source.buffer = buffer;
					source.connect(destination);
					audioDurationMs = Math.max(0, (endMs ?? buffer.duration * 1000) - startMs);
					source.start(context.currentTime + audioStartDelayMs / 1000, startMs / 1000, audioDurationMs / 1000);
					track = destination.stream.getAudioTracks()[0];
				} else {
					const AudioContext = globalThis.AudioContext ?? globalThis.webkitAudioContext;
					context = new AudioContext();
					oscillator = context.createOscillator();
					const gain = context.createGain();
					const destination = context.createMediaStreamDestination();
					gain.gain.value = 0.001;
					oscillator.connect(gain).connect(destination);
					oscillator.start();
					track = destination.stream.getAudioTracks()[0];
				}
				const recognition = new Recognition();
				recognition.lang = "ja-JP";
				recognition.interimResults = true;
				recognition.continuous = true;
				base.audioTrackStart.attempted = true;
				try {
					const events = [];
					const startedAt = performance.now();
					const event = await new Promise((resolveEvent) => {
						let settled = false;
						const finish = (kind, error = null) => {
							if (!settled) {
								settled = true;
								resolveEvent({ kind, error });
							}
						};
						recognition.onstart = () => {
							base.audioTrackStart.accepted = true;
							base.audioTrackStart.event = "start";
							if (!audioUrl) {
								finish("start");
							}
						};
						recognition.onresult = (value) => {
							for (let index = value.resultIndex; index < value.results.length; index += 1) {
								const result = value.results[index];
								events.push({
									elapsedMs: Math.round(performance.now() - startedAt),
									final: result.isFinal,
									transcript: result[0]?.transcript ?? "",
									confidence: result[0]?.confidence ?? null,
								});
							}
						};
						recognition.onerror = (value) => finish("error", value.error ?? String(value));
						recognition.onend = () => finish("end");
						recognition.start(track);
						if (audioDurationMs !== null) {
							setTimeout(() => recognition.stop(), audioStartDelayMs + audioDurationMs + 2000);
						}
						const timeoutMs = audioDurationMs !== null ? Math.max(audioStartDelayMs + audioDurationMs + 10_000, 10_000) : 2500;
						setTimeout(() => finish("timeout"), timeoutMs);
					});
					base.audioTrackStart.accepted ||= event.kind !== "error";
					base.audioTrackStart.event ??= event.kind;
					base.audioTrackStart.error = event.error;
					if (audioUrl) {
						base.recognition = {
							completion: event.kind,
							scheduledAudioStartMs: audioStartDelayMs,
							audioDurationMs,
							events,
						};
					}
				} catch (error) {
					base.audioTrackStart.error = `${error.name}: ${error.message}`;
				} finally {
					try {
						recognition.abort();
					} catch {
						// Recognition may already be inactive.
					}
					try {
						oscillator?.stop();
						source?.stop();
					} catch {
						// The scheduled source may already have ended.
					}
					track.stop();
					await context?.close();
				}
				return base;
			},
			{
				audioUrl: audioBytes ? `${url}audio.wav` : null,
				startMs,
				endMs,
			}
		);
		if (capabilities.recognition) {
			const events = capabilities.recognition.events;
			const finals = events.filter((event) => event.final);
			const recognized = finals.map((event) => event.transcript.trim()).join(" ");
			const firstPartial = events.find((event) => !event.final);
			const lastFinal = finals.at(-1);
			capabilities.recognition.summary = {
				recognized,
				firstPartialAfterAudioStartMs: firstPartial
					? firstPartial.elapsedMs - capabilities.recognition.scheduledAudioStartMs
					: null,
				lastFinalAfterAudioStartMs: lastFinal ? lastFinal.elapsedMs - capabilities.recognition.scheduledAudioStartMs : null,
				lastFinalAfterAudioEndMs: lastFinal
					? lastFinal.elapsedMs - (capabilities.recognition.scheduledAudioStartMs + capabilities.recognition.audioDurationMs)
					: null,
				characterErrorRate: reference && recognized ? characterErrorRate(reference, recognized) : null,
			};
		}
		results.push({
			browser: candidate.name,
			browserVersion,
			status: "ok",
			reference,
			...capabilities,
		});
	} catch (error) {
		results.push({
			browser: candidate.name,
			status: "launch-error",
			error: `${error.name}: ${error.message}`,
		});
	} finally {
		await browser?.close();
	}
}
server.close();

const report = {
	generatedAt: new Date().toISOString(),
	scope: audioBytes ? "saved WAV via MediaStreamTrack; no microphone capture" : "capability-only; no microphone capture",
	results,
};
const json = `${JSON.stringify(report, null, 2)}\n`;
process.stdout.write(json);
if (jsonOut) {
	const outputPath = resolve("..", jsonOut);
	await mkdir(dirname(outputPath), { recursive: true });
	await writeFile(outputPath, json, "utf8");
	process.stdout.write(`report: ${outputPath}\n`);
}
