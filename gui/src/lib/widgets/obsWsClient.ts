/**
 * Phase VI-γ-6a: OBS WebSocket v5 クライアント（最小実装）。
 *
 * obs-websocket v5 仕様（<https://github.com/obsproject/obs-websocket/blob/master/docs/generated/protocol.md>）
 * のうち、本 widget で必要な subset だけサポートする:
 *
 *   - Hello (op 0) / Identify (op 1) / Identified (op 2) ハンドシェイク（パスワード認証対応）
 *   - Request (op 6) / RequestResponse (op 7)
 *   - Event (op 5) を ハンドラに中継
 *
 * RequestBatch / Reidentify は使わない。外部依存を増やさないため `obs-websocket-js` は使わず自前実装する。
 */

type Handler = (eventType: string, data: unknown) => void;
type PendingRequest = {
	resolve: (value: unknown) => void;
	reject: (cause: unknown) => void;
	timer: ReturnType<typeof setTimeout>;
};

const CONNECT_TIMEOUT_MS = 8000;
const REQUEST_TIMEOUT_MS = 8000;

export class ObsWsClient {
	private ws: WebSocket | null = null;
	private url: string;
	private password: string | null;
	private pending = new Map<string, PendingRequest>();
	private handler: Handler | null = null;
	private reqSeq = 0;
	private closeHandler: ((reason: string) => void) | null = null;
	private intentionalClose = false;

	constructor(url: string, password: string | null) {
		this.url = url;
		this.password = password && password.length > 0 ? password : null;
	}

	get connected(): boolean {
		return this.ws !== null && this.ws.readyState === WebSocket.OPEN;
	}

	onEvent(handler: Handler): void {
		this.handler = handler;
	}
	onClose(handler: (reason: string) => void): void {
		this.closeHandler = handler;
	}

	async connect(): Promise<void> {
		return new Promise<void>((resolve, reject) => {
			let ws: WebSocket;
			try {
				ws = new WebSocket(this.url, "obswebsocket.json");
			} catch (e) {
				reject(e);
				return;
			}
			this.ws = ws;
			this.intentionalClose = false;
			let identified = false;
			let settled = false;
			const finish = (cause?: unknown) => {
				if (settled) return;
				settled = true;
				clearTimeout(connectTimer);
				if (cause === undefined) resolve();
				else reject(cause);
			};
			const connectTimer = setTimeout(() => {
				finish(new Error(`OBS接続が${CONNECT_TIMEOUT_MS / 1000}秒以内に完了しませんでした。`));
				try {
					ws.close();
				} catch {
					/* ignore */
				}
			}, CONNECT_TIMEOUT_MS);

			ws.onerror = (ev) => {
				if (!identified) {
					finish(new Error(`WebSocket 接続エラー: ${String((ev as ErrorEvent).message ?? "unknown")}`));
				}
			};
			ws.onclose = (ev) => {
				this.ws = null;
				this.rejectPending(new Error(`OBS接続が切断されました (code=${ev.code})`));
				if (!identified) {
					finish(new Error(`WebSocket が切断されました (code=${ev.code})`));
				} else if (!this.intentionalClose && this.closeHandler) {
					this.closeHandler(`code=${ev.code}`);
				}
			};

			ws.onmessage = async (ev) => {
				let msg: { op: number; d: Record<string, unknown> };
				try {
					msg = JSON.parse(ev.data as string);
				} catch {
					return;
				}
				if (msg.op === 0) {
					// Hello → Identify 送信
					try {
						const identify = await this.buildIdentify(msg.d);
						ws.send(JSON.stringify({ op: 1, d: identify }));
					} catch (e) {
						finish(e);
						ws.close();
					}
				} else if (msg.op === 2) {
					identified = true;
					finish();
				} else if (msg.op === 7) {
					const d = msg.d as {
						requestId: string;
						requestStatus: { result: boolean; code: number; comment?: string };
						responseData?: unknown;
					};
					const pending = this.pending.get(d.requestId);
					if (pending) {
						this.pending.delete(d.requestId);
						clearTimeout(pending.timer);
						if (d.requestStatus.result) {
							pending.resolve(d.responseData ?? {});
						} else {
							pending.reject(
								new Error(`OBS request failed (code=${d.requestStatus.code}): ${d.requestStatus.comment ?? "unknown"}`)
							);
						}
					}
				} else if (msg.op === 5) {
					const d = msg.d as { eventType: string; eventData?: unknown };
					this.handler?.(d.eventType, d.eventData);
				}
			};
		});
	}

	disconnect(): void {
		this.intentionalClose = true;
		this.rejectPending(new Error("OBS接続を切断しました。"));
		const ws = this.ws;
		this.ws = null;
		if (ws) {
			try {
				ws.close();
			} catch {
				/* ignore */
			}
		}
	}

	async request<T = unknown>(requestType: string, requestData?: Record<string, unknown>): Promise<T> {
		if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
			throw new Error("OBS に接続されていません。");
		}
		const requestId = `vac-${++this.reqSeq}`;
		const payload = {
			op: 6,
			d: {
				requestType,
				requestId,
				...(requestData ? { requestData } : {}),
			},
		};
		return await new Promise<T>((resolve, reject) => {
			const timer = setTimeout(() => {
				if (this.pending.has(requestId)) {
					this.pending.delete(requestId);
					reject(new Error(`OBS request ${requestType} timed out`));
				}
			}, REQUEST_TIMEOUT_MS);
			this.pending.set(requestId, {
				resolve: (v) => resolve(v as T),
				reject,
				timer,
			});
			try {
				this.ws!.send(JSON.stringify(payload));
			} catch (cause) {
				clearTimeout(timer);
				this.pending.delete(requestId);
				reject(cause);
			}
		});
	}

	private rejectPending(cause: Error): void {
		for (const pending of this.pending.values()) {
			clearTimeout(pending.timer);
			pending.reject(cause);
		}
		this.pending.clear();
	}

	private async buildIdentify(helloData: Record<string, unknown>): Promise<Record<string, unknown>> {
		const identify: Record<string, unknown> = {
			rpcVersion: helloData.rpcVersion ?? 1,
			// 全イベントを購読（ビットマスク -1 の低 8 bit のみ有効相当）。必要最低限は General=1, Scenes=4, Outputs=0x40。
			// 安全側で General|Scenes|Outputs を購読。
			eventSubscriptions: 0x01 | 0x04 | 0x40,
		};
		const auth = helloData.authentication as { challenge: string; salt: string } | undefined;
		if (auth) {
			if (!this.password) {
				throw new Error("OBS サーバは認証を要求しています。パスワードを設定してください。");
			}
			identify.authentication = await computeAuth(this.password, auth.salt, auth.challenge);
		}
		return identify;
	}
}

async function computeAuth(password: string, salt: string, challenge: string): Promise<string> {
	const enc = new TextEncoder();
	const pw_salt = await crypto.subtle.digest("SHA-256", enc.encode(password + salt));
	const b64secret = bufToBase64(pw_salt);
	const auth = await crypto.subtle.digest("SHA-256", enc.encode(b64secret + challenge));
	return bufToBase64(auth);
}

function bufToBase64(buf: ArrayBuffer): string {
	const bytes = new Uint8Array(buf);
	let bin = "";
	for (let i = 0; i < bytes.byteLength; i++) bin += String.fromCharCode(bytes[i]);
	return btoa(bin);
}
