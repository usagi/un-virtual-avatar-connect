import { defineConfig, type Plugin } from "vite";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve, relative } from "node:path";

// Release reuse must identify the actual generated UI, not just its timestamp.
function releaseIdentity(): Plugin {
	let output = "";
	let preview = false;
	return {
		name: "unvac-release-identity",
		configResolved(config) {
			output = resolve(config.root, config.build.outDir);
			preview = config.env.VITE_UN_DESIGN_PREVIEW === "1";
		},
		writeBundle() {
			const files: Record<string, string> = {};
			function visit(directory: string) {
				for (const entry of readdirSync(directory, { withFileTypes: true })) {
					const path = join(directory, entry.name);
					if (entry.isDirectory()) visit(path);
					else if (/\.(?:html|js|css)$/.test(entry.name)) {
						files[relative(output, path).replaceAll("\\", "/")] = createHash("sha256").update(readFileSync(path)).digest("hex");
					}
				}
			}
			visit(output);
			writeFileSync(join(output, "unvac-ui-build.json"), JSON.stringify({ schema_version: 1, design_preview: preview, files }, null, 2));
		},
	};
}
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

// ---------------------------------------------------------------------------
// Phase VI-β: VAC GUI Vite 設定
//
// - `base: '/gui/'` にして、本番は VAC 本体の Actix-web (`/gui/*`) で静的配信する。
// - dev サーバ (`npm run dev`) は 5173 を使い、Control API (`/api/v1/control`)
//   と既存 WS (`/websocket`) を VAC (127.0.0.1:57000) に HTTP/WS proxy する。
//   これで GUI 開発中も VAC を再起動せず、ホットリロードだけで動作を追える。
// - Tailwind v4 は plugin として組み込み、設定ファイルは廃止（v4 式）。
// ---------------------------------------------------------------------------

const VAC_BACKEND = process.env.VAC_BACKEND_URL ?? "http://127.0.0.1:57000";

export default defineConfig({
	base: "/gui/",
	plugins: [svelte(), tailwindcss(), releaseIdentity()],
	server: {
		port: 5173,
		strictPort: true,
		proxy: {
			"/api/v1/control": {
				target: VAC_BACKEND,
				changeOrigin: false,
				ws: true,
			},
			"/websocket": {
				target: VAC_BACKEND,
				changeOrigin: false,
				ws: true,
			},
			"/browser-output": {
				target: VAC_BACKEND,
				changeOrigin: false,
			},
			"/resources": {
				target: VAC_BACKEND,
				changeOrigin: false,
			},
		},
	},
	build: {
		outDir: "dist",
		emptyOutDir: true,
		sourcemap: true,
	},
});
