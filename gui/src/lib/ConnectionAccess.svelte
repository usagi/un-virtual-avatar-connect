<script lang="ts">
	import { setBearerToken } from "./auth";
	import { api } from "./api";
	let token = $state("");
	let busy = $state(false);
	let error = $state("");
	async function connect() {
		busy = true;
		error = "";
		try {
			await api.checkConnectionToken(token.trim());
			setBearerToken(token.trim() || null);
			const url = new URL(window.location.href);
			url.searchParams.delete("token");
			window.history.replaceState(null, "", url);
			window.location.reload();
		} catch {
			error = "接続できません。接続先の認証トークンと起動状態を確認してください。";
		} finally {
			busy = false;
		}
	}
</script>

<details class="border-b border-surface-200-800 px-3 py-3">
	<summary class="cursor-pointer text-xs">このVACの接続認証</summary>
	<form
		class="mt-2"
		onsubmit={(event) => {
			event.preventDefault();
			void connect();
		}}
	>
		<label class="text-xs"
			>認証トークン<input
				type="password"
				autocomplete="off"
				bind:value={token}
				class="mt-1 w-full rounded border border-surface-300-700 bg-surface-50-950 p-2"
			/></label
		>
		<p class="mt-2 text-[10px] opacity-60">
			この接続先だけに保存します。認証不要の構成では空欄にできます。成功すると画面を再読込します。
		</p>
		<button type="submit" disabled={busy} class="mt-2 rounded border border-surface-300-700 px-3 py-1 text-xs"
			>接続を確認して適用</button
		>
		{#if error}<p role="alert" class="mt-2 text-xs text-warning-500">{error}</p>{/if}
	</form>
</details>
