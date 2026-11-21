import { expect, test } from "@playwright/test";

import { tokenQuery } from "./fixtures";

test.describe("Live: OBS control widget", () => {
	test("scene item transform and volume controls send obs-websocket requests", async ({ page }) => {
		await page.addInitScript(() => {
			type RequestPayload = {
				op: number;
				d?: {
					requestType?: string;
					requestId?: string;
					requestData?: Record<string, unknown>;
				};
			};
			const win = window as Window & { __obsRequests?: RequestPayload[] };
			win.__obsRequests = [];
			const NativeWebSocket = window.WebSocket;

			class FakeObsWebSocket {
				static CONNECTING = 0;
				static OPEN = 1;
				static CLOSING = 2;
				static CLOSED = 3;
				readyState = FakeObsWebSocket.OPEN;
				onmessage: ((event: MessageEvent) => void) | null = null;
				onclose: ((event: CloseEvent) => void) | null = null;
				onerror: ((event: Event) => void) | null = null;

				constructor(url: string | URL, protocol?: string | string[]) {
					if (!String(url).startsWith("ws://127.0.0.1:4455")) {
						return new NativeWebSocket(url, protocol);
					}
					setTimeout(() => {
						this.onmessage?.(
							new MessageEvent("message", {
								data: JSON.stringify({ op: 0, d: { rpcVersion: 1 } }),
							})
						);
					}, 0);
				}

				send(data: string) {
					const msg = JSON.parse(data) as RequestPayload;
					win.__obsRequests!.push(msg);
					if (msg.op === 1) {
						setTimeout(() => {
							this.onmessage?.(new MessageEvent("message", { data: JSON.stringify({ op: 2, d: {} }) }));
						}, 0);
						return;
					}
					if (msg.op !== 6 || !msg.d?.requestType || !msg.d.requestId) return;
					const requestType = msg.d.requestType;
					const requestId = msg.d.requestId;
					const responseData =
						requestType === "GetVersion"
							? { obsVersion: "30.2.0" }
							: requestType === "GetSceneList"
								? {
										currentProgramSceneName: "Main",
										scenes: [{ sceneName: "Main", sceneIndex: 0 }],
									}
								: requestType === "GetSceneItemList"
									? {
											sceneItems: [
												{
													sceneItemId: 7,
													sourceName: "Browser",
													sceneItemEnabled: true,
													sceneItemIndex: 0,
												},
											],
										}
									: requestType === "GetSceneItemTransform"
										? { sceneItemTransform: { positionX: 100, positionY: 200, scaleX: 1, scaleY: 1 } }
										: requestType === "GetInputVolume"
											? { inputVolumeMul: 1 }
											: requestType === "GetSourceFilterList"
												? {
														filters: [
															{
																filterName: "Chroma",
																filterKind: "chroma_key_filter_v2",
																filterEnabled: true,
															},
														],
													}
												: {};
					setTimeout(() => {
						this.onmessage?.(
							new MessageEvent("message", {
								data: JSON.stringify({
									op: 7,
									d: {
										requestId,
										requestStatus: { result: true, code: 100 },
										responseData,
									},
								}),
							})
						);
					}, 0);
				}

				close() {
					this.readyState = FakeObsWebSocket.CLOSED;
					this.onclose?.(new CloseEvent("close", { code: 1000 }));
				}
			}

			Object.assign(FakeObsWebSocket, {
				CONNECTING: 0,
				OPEN: 1,
				CLOSING: 2,
				CLOSED: 3,
			});
			// @ts-expect-error test-only WebSocket replacement for OBS endpoint only
			window.WebSocket = FakeObsWebSocket;
		});

		await page.route("**/api/v1/control/bos", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({ root: "C:\\vac\\browser_output", entries: [] }),
			});
		});
		await page.route("**/api/v1/control/quick_effects?*", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({ effects: [], limit: 12, capacity: 128 }),
			});
		});

		await page.goto(`/gui/${tokenQuery()}#live`);
		await page.getByRole("button", { name: "接続" }).click();
		await expect(page.getByText("connected")).toBeVisible({ timeout: 10_000 });
		await expect(page.getByText("Browser", { exact: true })).toBeVisible();
		await expect(page.getByText("x=100 y=200 scale=1.00/1.00")).toBeVisible();
		await expect(page.getByRole("button", { name: "fx:Chroma" })).toHaveAttribute("aria-pressed", "true");

		await page.getByTitle("Move source right").click();
		await page.getByTitle("Scale source up").click();
		await page.getByTitle("Set source volume 50%").click();
		await page.getByRole("button", { name: "fx:Chroma" }).click();

		const requests = await page.evaluate(() => (window as Window & { __obsRequests?: unknown[] }).__obsRequests ?? []);
		expect(requests).toEqual(
			expect.arrayContaining([
				expect.objectContaining({
					d: expect.objectContaining({
						requestType: "SetSceneItemTransform",
						requestData: expect.objectContaining({
							sceneName: "Main",
							sceneItemId: 7,
							sceneItemTransform: expect.objectContaining({ positionX: 110, positionY: 200 }),
						}),
					}),
				}),
				expect.objectContaining({
					d: expect.objectContaining({
						requestType: "SetSceneItemTransform",
						requestData: expect.objectContaining({
							sceneItemTransform: expect.objectContaining({ scaleX: 1.1, scaleY: 1.1 }),
						}),
					}),
				}),
				expect.objectContaining({
					d: expect.objectContaining({
						requestType: "SetInputVolume",
						requestData: { inputName: "Browser", inputVolumeMul: 0.5 },
					}),
				}),
				expect.objectContaining({
					d: expect.objectContaining({
						requestType: "SetSourceFilterEnabled",
						requestData: { sourceName: "Browser", filterName: "Chroma", filterEnabled: false },
					}),
				}),
			])
		);

		await page.getByRole("button", { name: "切断" }).click();
		await expect(page.getByText("offline")).toBeVisible();
		await expect(page.getByText("OBS との接続が切れました")).toHaveCount(0);
	});
});
