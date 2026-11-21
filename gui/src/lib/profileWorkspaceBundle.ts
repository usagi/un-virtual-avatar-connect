import { zip } from "fflate";
import { parse as parseToml } from "smol-toml";

const MAX_FILE_COUNT = 1024;
const MAX_FILE_BYTES = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES = 64 * 1024 * 1024;
const MAX_ARCHIVE_BYTES = 32 * 1024 * 1024;
const FORBIDDEN_EXTENSIONS = new Set(["exe", "dll", "com", "bat", "cmd", "ps1", "msi", "scr", "lnk"]);

export type ProfileImportSource = { kind: "file"; file: File } | { kind: "workspace"; files: File[]; configPath: string };

export type PreparedProfileImport = {
	name: string;
	body: Blob;
	bundle: boolean;
	includedFileCount: number;
};

type WorkspaceEntry = {
	path: string;
	file: File;
};

export function workspaceConfigCandidates(files: readonly File[]): string[] {
	return normalizeWorkspaceEntries(files)
		.map((entry) => entry.path)
		.filter((path) => !path.includes("/") && /^conf(?:[._-].*)?\.toml$/i.test(path))
		.sort((left, right) => {
			if (left.toLowerCase() === "conf.toml") return -1;
			if (right.toLowerCase() === "conf.toml") return 1;
			return left.localeCompare(right);
		});
}

export function profileImportDisplayName(source: ProfileImportSource | null): string {
	if (!source) return "未選択";
	if (source.kind === "file") return source.file.name;
	return `${workspaceRootName(source.files)} / ${source.configPath}`;
}

export function profileImportBaseName(source: ProfileImportSource): string {
	const name = source.kind === "file" ? source.file.name : workspaceRootName(source.files);
	return name.replace(/\.(toml|zip)$/i, "") || "Imported";
}

export async function prepareProfileImport(source: ProfileImportSource): Promise<PreparedProfileImport> {
	if (source.kind === "file") {
		return {
			name: source.file.name,
			body: source.file,
			bundle: source.file.name.toLowerCase().endsWith(".zip"),
			includedFileCount: 1,
		};
	}
	const prepared = await buildWorkspaceBundle(source.files, source.configPath);
	return {
		name: `${profileImportBaseName(source)}.zip`,
		body: prepared.body,
		bundle: true,
		includedFileCount: prepared.includedFileCount,
	};
}

async function buildWorkspaceBundle(files: readonly File[], configPath: string): Promise<{ body: Blob; includedFileCount: number }> {
	const entries = normalizeWorkspaceEntries(files);
	const byPath = new Map(entries.map((entry) => [entry.path, entry]));
	const config = byPath.get(normalizeRelativePath(configPath));
	if (!config) throw new Error("選択した設定ファイルがワークスペース内にありません");

	const included = new Map<string, WorkspaceEntry>();
	const pending: Array<{ entry: WorkspaceEntry; archivePath: string }> = [];
	addEntry(config, "conf.toml", included, pending);
	const parsedConfig = parseStructuredFile(await config.file.text(), config.path);
	includeReferences(parsedConfig, "", byPath, included, pending);

	for (let index = 0; index < pending.length; index += 1) {
		const { entry, archivePath } = pending[index];
		if (archivePath === "conf.toml") continue;
		if (!/\.(toml|json)$/i.test(entry.path)) continue;
		try {
			const parsed = parseStructuredFile(await entry.file.text(), entry.path);
			const base = entry.path.includes("/") ? entry.path.slice(0, entry.path.lastIndexOf("/")) : "";
			includeReferences(parsed, base, byPath, included, pending);
		} catch {
			// Related data may use a .json suffix without being configuration. The server validates conf.toml.
		}
	}

	if (included.size > MAX_FILE_COUNT) throw new Error(`関連ファイルが多すぎます: ${included.size} / ${MAX_FILE_COUNT}`);
	let totalBytes = 0;
	const archive: Record<string, Uint8Array> = {};
	for (const [archivePath, entry] of included) {
		if (entry.file.size > MAX_FILE_BYTES) throw new Error(`ファイルが大きすぎます: ${entry.path}`);
		totalBytes += entry.file.size;
		if (totalBytes > MAX_TOTAL_BYTES) throw new Error("関連ファイルの合計サイズが64 MiBを超えています");
		archive[archivePath] = new Uint8Array(await entry.file.arrayBuffer());
	}
	const bytes = await zipArchive(archive);
	if (bytes.byteLength > MAX_ARCHIVE_BYTES) throw new Error("圧縮後のワークスペースが32 MiBを超えています");
	return {
		body: new Blob([bytes as BlobPart], { type: "application/zip" }),
		includedFileCount: included.size,
	};
}

function normalizeWorkspaceEntries(files: readonly File[]): WorkspaceEntry[] {
	const raw = files.map((file) => ({ path: normalizeRelativePath(file.webkitRelativePath || file.name), file }));
	const firstSegments = raw.map((entry) => entry.path.split("/", 1)[0]);
	const commonRoot = firstSegments.length > 0 && firstSegments.every((segment) => segment === firstSegments[0]) ? firstSegments[0] : null;
	return raw.map((entry) => ({
		path: commonRoot && entry.path.startsWith(`${commonRoot}/`) ? entry.path.slice(commonRoot.length + 1) : entry.path,
		file: entry.file,
	}));
}

function workspaceRootName(files: readonly File[]): string {
	const path = files[0]?.webkitRelativePath?.replaceAll("\\", "/") ?? "";
	return path.split("/")[0] || "ワークスペース";
}

function parseStructuredFile(source: string, path: string): unknown {
	return path.toLowerCase().endsWith(".json") ? JSON.parse(source) : parseToml(source);
}

function includeReferences(
	value: unknown,
	basePath: string,
	byPath: ReadonlyMap<string, WorkspaceEntry>,
	included: Map<string, WorkspaceEntry>,
	pending: Array<{ entry: WorkspaceEntry; archivePath: string }>
): void {
	for (const reference of collectStrings(value)) {
		for (const candidate of referenceCandidates(reference, basePath)) {
			const exact = byPath.get(candidate);
			if (exact) addEntry(exact, candidate, included, pending);
			const prefix = `${candidate}/`;
			for (const entry of byPath.values()) {
				if (entry.path.startsWith(prefix)) addEntry(entry, entry.path, included, pending);
			}
		}
	}
}

function collectStrings(value: unknown): string[] {
	if (typeof value === "string") return [value];
	if (Array.isArray(value)) return value.flatMap(collectStrings);
	if (value && typeof value === "object") return Object.values(value).flatMap(collectStrings);
	return [];
}

function referenceCandidates(value: string, basePath: string): string[] {
	const normalized = value.trim().replaceAll("\\", "/").replace(/^\.\//, "");
	if (!normalized || normalized.includes("://") || normalized.startsWith("/") || /^[A-Za-z]:\//.test(normalized)) return [];
	if (normalized.includes("\n") || normalized.includes("\r") || normalized.includes("\0")) return [];
	// Every string is inspected, including labels, dialogue and expressions.
	// A non-path value must not abort discovery of later material references.
	// Actual workspace filenames still pass strict normalizeRelativePath validation.
	if (normalized.split("/").some((part) => part === "" || part === "." || part === "..")) return [];
	const candidates = [normalizeRelativePath(normalized)];
	if (basePath) candidates.push(normalizeRelativePath(`${basePath}/${normalized}`));
	return [...new Set(candidates)];
}

function addEntry(
	entry: WorkspaceEntry,
	archivePath: string,
	included: Map<string, WorkspaceEntry>,
	pending: Array<{ entry: WorkspaceEntry; archivePath: string }>
): void {
	const normalizedArchivePath = normalizeRelativePath(archivePath);
	if (!isAllowedPath(normalizedArchivePath) || included.has(normalizedArchivePath)) return;
	included.set(normalizedArchivePath, entry);
	pending.push({ entry, archivePath: normalizedArchivePath });
}

function isAllowedPath(path: string): boolean {
	const parts = path.toLowerCase().split("/");
	if (parts.some((part) => ["credentials", "tmp.local", "flowgraph.disabled"].includes(part))) return false;
	// Native export skips ignored directories before descending into them.
	// Apply the same rule to every component of the browser's flat file list.
	if (parts.some((name, index) => {
		const currentCatalogue = index === parts.length - 1 && name === ".flowgraph-purposes.json";
		return (name.startsWith(".") && !currentCatalogue)
			|| name.endsWith(".bak")
			|| [".bak-", ".pre-", ".before-", ".old."].some((marker) => name.includes(marker));
	})) return false;
	if (/\.bak-[0-9-]+$/i.test(path) || /\.unvac-(?:tmp|previous)-/i.test(path)) return false;
	const extension = path.includes(".") ? path.slice(path.lastIndexOf(".") + 1).toLowerCase() : "";
	return !FORBIDDEN_EXTENSIONS.has(extension);
}

function normalizeRelativePath(path: string): string {
	const parts = path.replaceAll("\\", "/").split("/");
	if (parts.some((part) => part === "" || part === "." || part === "..")) throw new Error(`不正なワークスペースpathです: ${path}`);
	return parts.join("/");
}

function zipArchive(files: Record<string, Uint8Array>): Promise<Uint8Array> {
	return new Promise((resolve, reject) => {
		zip(files, { level: 6 }, (error, data) => {
			if (error) reject(error);
			else resolve(data);
		});
	});
}
