import type { TableEntryDto } from "../../types";

/** Row positions change after deletion/sorting. Never resolve a conflict by index alone. */
export function findConflictRow(base: TableEntryDto, rows: TableEntryDto[]): TableEntryDto | null {
	const keys = ["surface", "created_at", "by"] as const;
	if (typeof base.values.surface !== "string" || !base.values.surface) return null;
	const matches = rows.filter((row) => keys.every((key) =>
		(row.values[key] ?? "") === (base.values[key] ?? "")));
	// TSV has no immutable row ID. Missing/renamed/ambiguous identities require re-selection.
	return matches.length === 1 ? matches[0] : null;
}
